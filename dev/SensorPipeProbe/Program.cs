using System.IO.Pipes;
using System.Text;
using System.Text.Json;

internal static class Program
{
    private const string ControlPipe = "deepcool_sensor_data";

    private static async Task<int> Main()
    {
        string clientId = $"{DateTimeOffset.UtcNow.ToUnixTimeMilliseconds()}_probe";
        object payload = clientId;
        try
        {
            Console.WriteLine($"Sending SERVER_HELLO payload {JsonSerializer.Serialize(payload)}");
            using var control = new NamedPipeClientStream(
                ".", ControlPipe, PipeDirection.InOut, PipeOptions.Asynchronous);
            await control.ConnectAsync(2_000);
            control.ReadMode = PipeTransmissionMode.Message;

            await SendAsync(control, new { type = "SERVER_HELLO", seq = 0, payload });
            string hello = await ReadMessageAsync(control, 2_000);
            Console.WriteLine($"Received: {hello}");

            await SendAsync(control, new { type = "SETUP_DATA_CHANNEL", seq = 1, payload = new { } });
            string ready = await ReadMessageAsync(control, 2_000);
            Console.WriteLine($"Received: {ready}");

            using JsonDocument readyJson = JsonDocument.Parse(ready);
            string dataPath = readyJson.RootElement.GetProperty("payload")
                .GetProperty("DataPipeName").GetString()
                ?? throw new InvalidDataException("DATA_CHANNEL_READY omitted DataPipeName.");
            string dataPipeName = dataPath[(dataPath.LastIndexOf('\\') + 1)..];
            using var data = new NamedPipeClientStream(
                ".", dataPipeName, PipeDirection.In, PipeOptions.Asynchronous);
            await data.ConnectAsync(2_000);
            Console.WriteLine($"Connected data pipe: {dataPipeName}");

            await SendAsync(control, new { type = "GET_SENSOR_DATA", seq = 2, payload = new { } });
            string sensor = await ReadJsonAsync(data, 3_000);
            Console.WriteLine($"Sensor reply bytes: {Encoding.UTF8.GetByteCount(sensor)}");
            Console.WriteLine(sensor.Length <= 8_000 ? sensor : sensor[..8_000] + "...");
            return 0;
        }
        catch (Exception ex)
        {
            Console.WriteLine($"{ex.GetType().Name}: {ex.Message}");
        }

        return 1;
    }

    private static async Task SendAsync(PipeStream pipe, object message)
    {
        byte[] bytes = Encoding.UTF8.GetBytes(JsonSerializer.Serialize(message));
        await pipe.WriteAsync(bytes);
        await pipe.FlushAsync();
    }

    private static async Task<string> ReadMessageAsync(PipeStream pipe, int timeoutMs)
    {
        using var timeout = new CancellationTokenSource(timeoutMs);
        using var buffer = new MemoryStream();
        byte[] chunk = new byte[16 * 1024];
        do
        {
            int read = await pipe.ReadAsync(chunk, timeout.Token);
            if (read == 0)
                throw new EndOfStreamException("Pipe closed before a response arrived.");
            Console.WriteLine($"Data read: {read} bytes");
            Console.WriteLine($"Hex: {Convert.ToHexString(chunk.AsSpan(0, Math.Min(read, 96)))}");
            string preview = Encoding.UTF8.GetString(chunk, 0, Math.Min(read, 256))
                .Replace("\0", "\\0").Replace("\r", "\\r").Replace("\n", "\\n");
            Console.WriteLine($"Text: {preview}");
            buffer.Write(chunk, 0, read);
        } while (!pipe.IsMessageComplete);
        return Encoding.UTF8.GetString(buffer.ToArray());
    }

    private static async Task<string> ReadJsonAsync(PipeStream pipe, int timeoutMs)
    {
        using var timeout = new CancellationTokenSource(timeoutMs);
        using var buffer = new MemoryStream();
        byte[] chunk = new byte[64 * 1024];
        while (true)
        {
            int read = await pipe.ReadAsync(chunk, timeout.Token);
            if (read == 0)
                throw new EndOfStreamException("Pipe closed before a response arrived.");
            buffer.Write(chunk, 0, read);
            byte[] frame = buffer.ToArray();
            if (frame.Length < 6)
                continue;
            if (!frame.AsSpan(0, 4).SequenceEqual(new byte[] { 0xDE, 0xAD, 0xBE, 0xEF }))
                throw new InvalidDataException($"Unexpected data-frame magic: {Convert.ToHexString(frame.AsSpan(0, 4))}");

            int jsonLength = BitConverter.ToUInt16(frame, 4);
            if (frame.Length < 6 + jsonLength)
                continue;

            string json = Encoding.UTF8.GetString(frame, 6, jsonLength);
            int trailerLength = frame.Length - 6 - jsonLength;
            Console.WriteLine($"Data frame: {frame.Length} bytes; JSON: {jsonLength}; trailer: " +
                (trailerLength == 0 ? "none" : Convert.ToHexString(frame.AsSpan(6 + jsonLength))));
            using JsonDocument _ = JsonDocument.Parse(json);
            return json;
        }
    }
}
