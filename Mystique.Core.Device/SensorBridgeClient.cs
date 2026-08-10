using System.IO.Pipes;
using System.Security.Principal;
using System.Text;
using System.Text.Json;

namespace Mystique.Core.Device;

/// <summary>
/// Client for the DeepCool Sensor Bridge named-pipe protocol: a control pipe
/// carries a JSON handshake which allocates a per-client data pipe, and the server
/// then pushes framed sensor snapshots on that data pipe.
/// </summary>
public sealed class SensorBridgeClient : ISensorSource
{
    public const string ControlPipeName = "deepcool_sensor_data";

    private const int ConnectTimeoutMilliseconds = 2_000;
    private const int FrameHeaderLength = 6;
    private const int FrameTrailerLength = 2;

    private static readonly byte[] FrameMagic = [0xDE, 0xAD, 0xBE, 0xEF];

    private readonly NamedPipeClientStream control;
    private readonly NamedPipeClientStream data;

    private SensorBridgeClient(NamedPipeClientStream control, NamedPipeClientStream data)
    {
        this.control = control;
        this.data = data;
    }

    public static async Task<SensorBridgeClient> ConnectAsync(CancellationToken cancellationToken)
    {
        // TokenImpersonationLevel.Anonymous is required, not cosmetic: the simple
        // constructor leaves SECURITY_SQOS_PRESENT unset, so NT defaults to
        // SecurityImpersonation and a squatted server could impersonate our token.
        var control = new NamedPipeClientStream(
            ".", ControlPipeName, PipeDirection.InOut,
            PipeOptions.Asynchronous, TokenImpersonationLevel.Anonymous);

        NamedPipeClientStream? data = null;
        try
        {
            await control.ConnectAsync(ConnectTimeoutMilliseconds, cancellationToken);
            control.ReadMode = PipeTransmissionMode.Message;

            string clientId =
                $"{DateTimeOffset.UtcNow.ToUnixTimeMilliseconds()}_mystique_service";
            await SendControlAsync(control,
                new { type = "SERVER_HELLO", seq = 0, payload = clientId }, cancellationToken);

            using (JsonDocument hello = JsonDocument.Parse(
                await ReadControlAsync(control, cancellationToken)))
            {
                if (hello.RootElement.GetProperty("type").GetString() != "CLIENT_HELLO")
                    throw new InvalidDataException("Sensor bridge rejected the CLIENT_HELLO handshake.");
            }

            await SendControlAsync(control,
                new { type = "SETUP_DATA_CHANNEL", seq = 1, payload = new { } }, cancellationToken);

            string dataPipeName;
            using (JsonDocument ready = JsonDocument.Parse(
                await ReadControlAsync(control, cancellationToken)))
            {
                if (ready.RootElement.GetProperty("type").GetString() != "DATA_CHANNEL_READY")
                    throw new InvalidDataException("Sensor bridge did not create a data channel.");

                string dataPath = ready.RootElement.GetProperty("payload")
                    .GetProperty("DataPipeName").GetString()
                    ?? throw new InvalidDataException("Sensor bridge omitted DataPipeName.");
                dataPipeName = dataPath[(dataPath.LastIndexOf('\\') + 1)..];
            }

            data = new NamedPipeClientStream(
                ".", dataPipeName, PipeDirection.In,
                PipeOptions.Asynchronous, TokenImpersonationLevel.Anonymous);
            await data.ConnectAsync(ConnectTimeoutMilliseconds, cancellationToken);

            await SendControlAsync(control,
                new { type = "GET_SENSOR_DATA", seq = 2, payload = new { } }, cancellationToken);

            return new SensorBridgeClient(control, data);
        }
        catch
        {
            data?.Dispose();
            control.Dispose();
            throw;
        }
    }

    public async Task<SensorSample> ReadAsync(CancellationToken cancellationToken)
    {
        byte[] header = new byte[FrameHeaderLength];
        await data.ReadExactlyAsync(header.AsMemory(), cancellationToken);

        int jsonLength = ReadFrameLength(header);
        byte[] jsonBytes = new byte[jsonLength];
        await data.ReadExactlyAsync(jsonBytes.AsMemory(), cancellationToken);

        byte[] trailer = new byte[FrameTrailerLength];
        await data.ReadExactlyAsync(trailer.AsMemory(), cancellationToken);

        return ParseSensorPayload(jsonBytes);
    }

    public static int ReadFrameLength(ReadOnlySpan<byte> header)
    {
        if (header.Length < FrameHeaderLength)
            throw new InvalidDataException(
                $"Sensor frame header truncated: {header.Length} of {FrameHeaderLength} bytes.");

        if (!header[..4].SequenceEqual(FrameMagic))
            throw new InvalidDataException(
                $"Unexpected sensor-frame magic: {Convert.ToHexString(header[..4])}");

        return header[4] | (header[5] << 8);
    }

    public static SensorSample ParseSensorPayload(ReadOnlySpan<byte> jsonBytes)
    {
        using JsonDocument outer = JsonDocument.Parse(jsonBytes.ToArray());
        if (outer.RootElement.GetProperty("type").GetString() != "GET_SENSOR_DATA")
            throw new InvalidDataException("Unexpected sensor-bridge data message.");

        string innerJson = outer.RootElement.GetProperty("payload").GetString()
            ?? throw new InvalidDataException("Sensor data payload was empty.");

        using JsonDocument inner = JsonDocument.Parse(innerJson);
        JsonElement root = inner.RootElement;

        return new SensorSample(
            ReadValue(root, "CPU Temperature"),
            ReadValue(root, "CPU Usage"),
            ReadValue(root, "Memory Load"),
            ReadValue(root, "Gpu temperature"),
            ReadValue(root, "CPU Clock"));
    }

    private static double ReadValue(JsonElement root, string name)
    {
        // ValueKind must be checked first: TryGetDouble throws InvalidOperationException
        // on a non-numeric token rather than returning false, which would escape the
        // read path as an unclassified exception.
        if (!root.TryGetProperty(name, out JsonElement sensor) ||
            !sensor.TryGetProperty("Value", out JsonElement value) ||
            value.ValueKind != JsonValueKind.Number ||
            !value.TryGetDouble(out double result))
            throw new InvalidDataException($"Sensor bridge omitted numeric '{name}'.");

        return result;
    }

    private static async Task SendControlAsync(
        PipeStream pipe, object message, CancellationToken cancellationToken)
    {
        byte[] bytes = Encoding.UTF8.GetBytes(JsonSerializer.Serialize(message));
        await pipe.WriteAsync(bytes.AsMemory(), cancellationToken);
        await pipe.FlushAsync(cancellationToken);
    }

    private static async Task<string> ReadControlAsync(
        PipeStream pipe, CancellationToken cancellationToken)
    {
        using var buffer = new MemoryStream();
        byte[] chunk = new byte[16 * 1024];
        do
        {
            int read = await pipe.ReadAsync(chunk.AsMemory(), cancellationToken);
            if (read == 0)
                throw new EndOfStreamException("Sensor control pipe closed during handshake.");
            buffer.Write(chunk, 0, read);
        } while (!pipe.IsMessageComplete);

        return Encoding.UTF8.GetString(buffer.ToArray());
    }

    public void Dispose()
    {
        data.Dispose();
        control.Dispose();
    }
}
