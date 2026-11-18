using System.Diagnostics;
using System.Globalization;
using System.Text.Json;
using System.Text.Json.Serialization;
using Mystique.Core.Contracts;
using Mystique.Core.Device;

namespace MystiqueCapture;

/// <summary>
/// Hardware capture harness for the MYSTIQUE LCD.
///
/// Safety model: the only command bytes this tool can emit are the four proven
/// ones (0x01, 0x02, 0x10, 0x12) plus the fixed thirteen-step init sequence,
/// which is built from MystiquePackets.BuildInitSequence and can be emitted only
/// by the `init` verb. There is no path from a script or command line to 0x0A,
/// to a firmware command, or to any other command byte.
/// </summary>
internal static class Program
{
    private const string Usage = """
        MystiqueCapture — live MYSTIQUE protocol capture harness

          info   [--out FILE]                    0x12 on pipe 0x01
          status [--repeat N] [--out FILE]       0x10 on pipe 0x02
          init   [rotation 0-3] [--out FILE]     the fixed 13-command sequence
          send   <cmd> <payloadHex|-> [--out]    one allowed command
          frame  <cpuT> <cpu%> <mem%> <gpuT> <mhz> [--out]
                                                 one 0x01 frame built by MystiquePackets
          script <script.json> [--out FILE]      scripted exchanges
          drain  [--out FILE]                    read-only: unsolicited IN data?
          bad-sync [--out FILE]                  0x01 frame with byte 0 = 0xAB
          map                                    structure-only view of 0x12 body

        Only 0x01, 0x02, 0x10 and 0x12 are accepted. 0x0A and every other init
        command are reachable only through `init`, with the production payload.
        Responses are always emitted with the identifier region redacted and the
        response checksum field withheld; a boolean says whether it validated.
        """;

    private static readonly JsonSerializerOptions JsonOptions = new()
    {
        WriteIndented = true,
        PropertyNamingPolicy = JsonNamingPolicy.CamelCase,
        DefaultIgnoreCondition = JsonIgnoreCondition.WhenWritingNull,
    };

    private static int Main(string[] args)
    {
        if (args.Length == 0)
        {
            Console.Error.WriteLine(Usage);
            return 2;
        }

        string verb = args[0].ToLowerInvariant();
        var rest = args.Skip(1).ToList();
        string? outPath = TakeOption(rest, "--out");

        try
        {
            switch (verb)
            {
                case "info":
                    return RunDocument("info", null, outPath, (link, sink) =>
                    {
                        sink.Record(link, "info 0x12", 0x12, [], 0, 46, true, 0, 10);
                    });

                case "status":
                {
                    int repeat = TakeIntOption(rest, "--repeat") ?? 3;
                    return RunDocument("status", null, outPath, (link, sink) =>
                    {
                        for (int i = 0; i < repeat; i++)
                            sink.Record(link, $"status 0x10 #{i + 1}", 0x10, [], 0, 46, true, 0, 250);
                    });
                }

                case "init":
                {
                    byte rotation = 0;
                    if (rest.Count > 0 && !byte.TryParse(rest[0], out rotation))
                    {
                        Console.Error.WriteLine($"rotation must be 0-3, got '{rest[0]}'");
                        return 2;
                    }

                    return RunDocument("init", null, outPath, (link, sink) =>
                    {
                        foreach (InitCommand step in MystiquePackets.BuildInitSequence(rotation))
                            sink.Record(
                                link,
                                $"init 0x{step.Command:X2} {Wire.Hex(step.Payload)}".TrimEnd(),
                                step.Command,
                                step.Payload,
                                0, 46, true,
                                step.SettleMilliseconds, step.SettleMilliseconds);
                        sink.Record(link, "status after init", 0x10, [], 0, 46, true, 0, 100);
                    });
                }

                case "send":
                {
                    if (rest.Count < 2)
                    {
                        Console.Error.WriteLine(Usage);
                        return 2;
                    }

                    byte command = ParseCommand(rest[0]);
                    byte[] payload = ParsePayload(rest[1]);
                    EnsurePayloadShape(command, payload);
                    return RunDocument("send", null, outPath, (link, sink) =>
                        sink.Record(link, $"send 0x{command:X2}", command, payload, 0, 46, true, 0, 250));
                }

                case "frame":
                {
                    if (rest.Count < 5)
                    {
                        Console.Error.WriteLine(Usage);
                        return 2;
                    }

                    var frame = new TelemetryFrame(
                        byte.Parse(rest[0], CultureInfo.InvariantCulture),
                        byte.Parse(rest[1], CultureInfo.InvariantCulture),
                        byte.Parse(rest[2], CultureInfo.InvariantCulture),
                        byte.Parse(rest[3], CultureInfo.InvariantCulture),
                        ushort.Parse(rest[4], CultureInfo.InvariantCulture));

                    byte[] payload = MystiquePackets.BuildDisplayPayload(frame);
                    return RunDocument("frame", null, outPath, (link, sink) =>
                        sink.Record(link, "telemetry frame", 0x01, payload, 0, 46, true, 0, 500));
                }

                case "script":
                {
                    if (rest.Count < 1)
                    {
                        Console.Error.WriteLine(Usage);
                        return 2;
                    }

                    string scriptPath = rest[0];
                    string text = File.ReadAllText(scriptPath);
                    ScriptDocument script = JsonSerializer.Deserialize<ScriptDocument>(text, JsonOptions)
                        ?? throw new InvalidOperationException("script deserialized to null");

                    return RunDocument("script", script.Name, outPath ?? DefaultOutPath(script.Name ?? "script"), (link, sink) =>
                    {
                        foreach (ScriptStep step in script.Steps)
                        {
                            byte command = ParseCommand(step.Cmd ?? "");
                            string payloadText = step.Payload ?? script.BasePayload ?? "-";
                            byte[] payload = ParsePayload(payloadText);
                            ApplyPatch(payload, step.Patch);
                            EnsurePayloadShape(command, payload);
                            for (int i = 0; i < Math.Max(1, step.Repeat); i++)
                            {
                                string name = step.Repeat > 1
                                    ? $"{step.Name} [{i + 1}/{step.Repeat}]"
                                    : step.Name ?? "";
                                sink.Record(
                                    link, name, command, payload,
                                    step.ChecksumAdjust, step.ChecksumBytes, step.Hidc,
                                    step.SettleMs, Math.Max(0, step.SettleMs));
                            }
                        }
                    });
                }

                case "map":
                {
                    // Structure-only view of the 0x12 body: character classes and
                    // run lengths, never the characters themselves.
                    using DeviceLink link = DeviceLink.Open();
                    Console.WriteLine($"device: {link.RedactedPath}");
                    ExchangeOutcome outcome = link.Exchange(
                        MystiquePackets.InitPipeOut, MystiquePackets.InitPipeIn,
                        Wire.Build(new PacketSpec(MystiquePackets.CommandInfo, [])));
                    if (!outcome.Received)
                    {
                        Console.Error.WriteLine($"no response: {outcome.Error}");
                        return 1;
                    }

                    byte[] r = outcome.Response;
                    Console.WriteLine($"length={r.Length} prefix={Wire.HexAt(r, 0, 4)} tail={Wire.HexAt(r, 42, 4)}");
                    int end = Math.Min(42, r.Length);
                    var classes = new char[end - 4];
                    for (int i = 4; i < end; i++)
                        classes[i - 4] = Classify(r[i]);
                    Console.WriteLine($"class map 4..41: {new string(classes)}");
                    Console.WriteLine("  legend: d=0-9 u=A-F l=a-f a=other alpha p=other printable 0=zero X=non-ascii");
                    int runStart = 0;
                    for (int i = 1; i <= classes.Length; i++)
                    {
                        if (i == classes.Length || classes[i] != classes[runStart])
                        {
                            Console.WriteLine($"  {runStart + 4,2}..{i + 3,2}  {classes[runStart]}  x{i - runStart}");
                            runStart = i;
                        }
                    }

                    return 0;
                }

                case "bad-sync":
                {
                    // Framing probe on a display frame only: byte 0 AA -> AB, the
                    // rest of the packet and a valid checksum are unchanged.
                    return RunDocument("bad-sync", null, outPath, (link, sink) =>
                        sink.Record(link, "telemetry with sync 0xAB", 0x01, BaselinePayload(), 0, 46, true, 0, 400, 0xAB));
                }

                case "drain":
                {
                    // Read-only: does the device ever push data unprompted?
                    using DeviceLink link = DeviceLink.Open();
                    Console.WriteLine($"device: {link.RedactedPath}");
                    var reads = new List<object>();
                    foreach (byte pipe in new byte[] { 0x81, 0x82 })
                    {
                        for (int i = 0; i < 2; i++)
                        {
                            ExchangeOutcome outcome = link.Read(pipe);
                            Console.WriteLine(
                                $"  read 0x{pipe:X2} #{i + 1}: " +
                                (outcome.Received ? $"DATA {Wire.Hex(outcome.Response)}" : $"none ({outcome.Win32Error}) {outcome.Error}"));
                            reads.Add(new
                            {
                                pipe = $"0x{pipe:X2}",
                                received = outcome.Received,
                                length = outcome.Response.Length,
                                win32Error = outcome.Win32Error,
                                error = outcome.Error,
                            });
                        }
                    }

                    string path = outPath ?? DefaultOutPath("drain");
                    Directory.CreateDirectory(Path.GetDirectoryName(Path.GetFullPath(path))!);
                    File.WriteAllText(path, JsonSerializer.Serialize(new
                    {
                        tool = "MystiqueCapture/1",
                        mode = "drain",
                        capturedAtUtc = DateTimeOffset.UtcNow.ToString("O", CultureInfo.InvariantCulture),
                        devicePathRedacted = link.RedactedPath,
                        reads,
                    }, JsonOptions));
                    Console.WriteLine($"wrote {path}");
                    return 0;
                }

                case "reset":
                {
                    // Recovery only: pipe abort/flush/reset, then a status probe.
                    // --device additionally issues a USB bus reset.
                    bool busReset = rest.Contains("--device", StringComparer.OrdinalIgnoreCase);
                    using (DeviceLink link = DeviceLink.Open())
                    {
                        Console.WriteLine($"device: {link.RedactedPath}");
                        foreach (string line in link.ResetPipes())
                            Console.WriteLine($"  pipes {line}");
                        Thread.Sleep(500);
                        ExchangeOutcome probe = link.Exchange(
                            MystiquePackets.DataPipeOut, MystiquePackets.DataPipeIn,
                            Wire.Build(new PacketSpec(MystiquePackets.CommandStatus, [])));
                        Console.WriteLine(probe.Received
                            ? $"  status after pipe reset: {Wire.Hex(probe.Response)}"
                            : $"  status after pipe reset: no response ({probe.Win32Error})");

                        if (!probe.Received && busReset)
                        {
                            Console.WriteLine($"  WinUsb_ResetDevice: {link.ResetDevice()}");
                        }
                    }

                    if (busReset)
                    {
                        Thread.Sleep(2000);
                        using DeviceLink link = DeviceLink.Open();
                        ExchangeOutcome probe = link.Exchange(
                            MystiquePackets.DataPipeOut, MystiquePackets.DataPipeIn,
                            Wire.Build(new PacketSpec(MystiquePackets.CommandStatus, [])));
                        Console.WriteLine(probe.Received
                            ? $"  status after bus reset: {Wire.Hex(probe.Response)}"
                            : $"  status after bus reset: no response ({probe.Win32Error})");
                    }

                    return 0;
                }

                case "recover":
                {
                    // Escalating, non-destructive endpoint resets. No firmware, no
                    // command bytes beyond the proven 0x10 status read.
                    bool ok = false;
                    using (DeviceLink link = DeviceLink.Open())
                    {
                        Console.WriteLine($"device: {link.RedactedPath}");
                        foreach (string line in link.ResetPipes())
                            Console.WriteLine($"  pipes {line}");

                        Console.WriteLine($"  SET_INTERFACE(0, alt 0): {link.SetInterfaceZero()}");
                        Thread.Sleep(500);
                        ok = ProbeStatus(link);

                        if (!ok)
                        {
                            Console.WriteLine($"  SET_CONFIGURATION(1): {link.SetConfiguration(1)}");
                        }
                    }

                    Thread.Sleep(1500);
                    if (!ok)
                    {
                        using DeviceLink link = DeviceLink.Open();
                        foreach (string line in link.ResetPipes())
                            Console.WriteLine($"  pipes {line}");
                        ok = ProbeStatus(link);
                    }

                    Console.WriteLine(ok
                        ? "recovered: device answers 0x10 again"
                        : "NOT recovered: needs a physical replug or an elevated port cycle");
                    return ok ? 0 : 4;
                }

                case "poke":
                {
                    // Writes N valid 0x10 packets without waiting to interpret them,
                    // to see whether the OUT endpoint ever starts draining again.
                    using DeviceLink link = DeviceLink.Open();
                    Console.WriteLine($"device: {link.RedactedPath}");
                    byte[] packet = Wire.Build(new PacketSpec(MystiquePackets.CommandStatus, []));
                    for (int i = 0; i < 5; i++)
                    {
                        ExchangeOutcome outcome = link.Exchange(
                            MystiquePackets.DataPipeOut, MystiquePackets.DataPipeIn, packet);
                        Console.WriteLine($"  poke {i + 1}: " +
                            (outcome.Received ? $"ACK {Wire.Hex(outcome.Response)}" : $"no ({outcome.Win32Error})"));
                    }

                    return 0;
                }

                case "desc":
                {
                    // Read-only standard control requests: is the control endpoint
                    // alive, and is the device still configured?
                    using DeviceLink link = DeviceLink.Open();
                    Console.WriteLine($"device: {link.RedactedPath}");
                    Console.WriteLine($"  current alt setting (cached): {link.CurrentAlternateSetting()}");
                    (bool ok, byte[] data, int err) = link.ControlIn(0x80, 0x08, 0, 0, 1); // GET_CONFIGURATION
                    Console.WriteLine(ok
                        ? $"  GET_CONFIGURATION: {Wire.Hex(data)}"
                        : $"  GET_CONFIGURATION failed ({err})");
                    (ok, data, err) = link.ControlIn(0x80, 0x06, 0x0100, 0, 18); // GET_DESCRIPTOR(device)
                    Console.WriteLine(ok
                        ? $"  GET_DESCRIPTOR(device): {Wire.Hex(data)}"
                        : $"  GET_DESCRIPTOR failed ({err})");
                    return ok ? 0 : 5;
                }

                case "pad":
                {
                    // Some firmware buffers a fixed 64-byte frame even though the
                    // proven host packet is 48 bytes. Writes a valid 0x10 request
                    // padded to 64 bytes; harmless if the device ignores the tail.
                    using DeviceLink link = DeviceLink.Open();
                    Console.WriteLine($"device: {link.RedactedPath}");
                    byte[] packet = new byte[64];
                    Wire.Build(new PacketSpec(MystiquePackets.CommandStatus, [])).CopyTo(packet, 0);
                    for (int i = 0; i < 3; i++)
                    {
                        ExchangeOutcome outcome = link.Exchange(
                            MystiquePackets.DataPipeOut, MystiquePackets.DataPipeIn, packet);
                        Console.WriteLine($"  padded 64-byte write {i + 1}: " +
                            (outcome.Received ? $"ACK {Wire.Hex(outcome.Response)}" : $"no ({outcome.Win32Error})"));
                    }

                    return 0;
                }

                case "setconfig":
                {
                    using DeviceLink link = DeviceLink.Open();
                    Console.WriteLine($"device: {link.RedactedPath}");
                    Console.WriteLine($"  CLEAR_FEATURE(ENDPOINT_HALT, 0x02): {link.ClearEndpointHalt(0x02)}");
                    Console.WriteLine($"  CLEAR_FEATURE(ENDPOINT_HALT, 0x01): {link.ClearEndpointHalt(0x01)}");
                    Console.WriteLine("  issuing SET_CONFIGURATION(1)...");
                    Console.WriteLine($"  SET_CONFIGURATION(1): {link.SetConfiguration(1)}");
                    return 0;
                }

                default:
                    Console.Error.WriteLine(Usage);
                    return 2;
            }
        }
        catch (RefusedException ex)
        {
            Console.Error.WriteLine($"REFUSED: {ex.Message}");
            return 3;
        }
        catch (Exception ex)
        {
            Console.Error.WriteLine($"ERROR: {ex.GetType().Name}: {ex.Message}");
            return 1;
        }
    }

    private static int RunDocument(
        string mode, string? scriptName, string? outPath, Action<DeviceLink, Sink> body)
    {
        string path = outPath ?? DefaultOutPath(mode);
        Directory.CreateDirectory(Path.GetDirectoryName(Path.GetFullPath(path))!);

        using DeviceLink link = DeviceLink.Open();
        Console.WriteLine($"device: {link.RedactedPath}");
        Console.WriteLine($"pipes:  {string.Join(", ", link.Pipes.Select(p => $"0x{p.PipeId:X2} {p.Type} mps={p.MaximumPacketSize}"))}");

        var sink = new Sink();
        body(link, sink);

        var document = new CaptureDocument(
            Tool: "MystiqueCapture/1",
            Mode: mode,
            ScriptName: scriptName,
            CapturedAtUtc: DateTimeOffset.UtcNow.ToString("O", CultureInfo.InvariantCulture),
            DevicePathRedacted: link.RedactedPath,
            Pipes: sink.Pipes,
            Exchanges: sink.Exchanges,
            ResponseChecksumNote: "response bytes 4..41 are redacted and bytes 46..47 (device-specific checksum) are withheld; checksumValid is computed in memory.");

        File.WriteAllText(path, JsonSerializer.Serialize(document, JsonOptions));
        Console.WriteLine($"wrote {path} ({sink.Exchanges.Count} exchanges)");

        return 0;
    }

    private static byte ParseCommand(string text)
    {
        string cleaned = text.Trim();
        if (cleaned.StartsWith("0x", StringComparison.OrdinalIgnoreCase))
            cleaned = cleaned[2..];
        if (!byte.TryParse(cleaned, NumberStyles.HexNumber, CultureInfo.InvariantCulture, out byte command))
            throw new RefusedException($"'{text}' is not a byte");

        if (!PipeFor(command).IsAllowed)
            throw new RefusedException(
                $"command 0x{command:X2} is outside the proven allowlist (0x01, 0x02, 0x10, 0x12); " +
                "0x0A and the remaining init commands are reachable only through `init`.");

        return command;
    }

    private static byte[] ParsePayload(string text)
    {
        string trimmed = text.Trim();
        if (trimmed is "" or "-" or "none")
            return [];

        string cleaned = new string(trimmed.Where(c => !char.IsWhiteSpace(c) && c is not '-' and not ',' and not '_').ToArray());
        if (cleaned.StartsWith("0x", StringComparison.OrdinalIgnoreCase))
            cleaned = cleaned[2..];
        if (cleaned.Length % 2 != 0)
            throw new RefusedException($"payload '{text}' has an odd number of hex digits");
        if (!cleaned.All(Uri.IsHexDigit))
            throw new RefusedException($"payload '{text}' contains a non-hex character");

        byte[] payload = new byte[cleaned.Length / 2];
        for (int i = 0; i < payload.Length; i++)
            payload[i] = byte.Parse(cleaned.AsSpan(i * 2, 2), NumberStyles.HexNumber, CultureInfo.InvariantCulture);

        return payload;
    }

    private static void ApplyPatch(byte[] payload, Dictionary<string, string>? patch)
    {
        if (patch is null)
            return;

        foreach ((string indexText, string valueText) in patch)
        {
            if (!int.TryParse(indexText, out int index) || index < 0 || index >= payload.Length)
                throw new RefusedException($"patch index '{indexText}' is outside the payload");
            byte value = ParsePayload(valueText) is [var single]
                ? single
                : throw new RefusedException($"patch value '{valueText}' is not one byte");
            payload[index] = value;
            Console.WriteLine($"      patch [{index}] = 0x{value:X2}");
        }
    }

    private static void EnsurePayloadShape(byte command, byte[] payload)
    {
        if (payload.Length > Wire.MaxPayloadLength)
            throw new RefusedException($"payload is {payload.Length} bytes; maximum is {Wire.MaxPayloadLength}");

        if (command == 0x01 && payload.Length != Wire.MaxPayloadLength)
            throw new RefusedException(
                $"0x01 telemetry frames are exactly {Wire.MaxPayloadLength} bytes; got {payload.Length}");
    }

    private static bool IsScriptable(byte command) =>
        command is MystiquePackets.CommandInfo or 0x02
            or MystiquePackets.CommandStatus or MystiquePackets.CommandTelemetry;

    /// <summary>
    /// Routing only. Init-only commands are reachable exclusively through `init`,
    /// which builds them from MystiquePackets.BuildInitSequence; IsScriptable is
    /// the gate that keeps command lines and scripts off them.
    /// </summary>
    private static (byte Out, byte In, bool IsAllowed) PipeFor(byte command) => command switch
    {
        MystiquePackets.CommandInfo => (MystiquePackets.InitPipeOut, MystiquePackets.InitPipeIn, IsScriptable(command)),
        0x02 or 0x03 or 0x04 or 0x05 or 0x06 or 0x07 or 0x08 or 0x0B or 0x15 or 0x16 or 0x17 or 0x0A
            => (MystiquePackets.InitPipeOut, MystiquePackets.InitPipeIn, IsScriptable(command)),
        MystiquePackets.CommandStatus => (MystiquePackets.DataPipeOut, MystiquePackets.DataPipeIn, IsScriptable(command)),
        MystiquePackets.CommandTelemetry => (MystiquePackets.DataPipeOut, MystiquePackets.DataPipeIn, IsScriptable(command)),
        _ => ((byte)0, (byte)0, false),
    };

    private static string DefaultOutPath(string name)
    {
        string safe = string.Concat(name.Select(c => char.IsLetterOrDigit(c) || c is '-' or '_' ? c : '-'));
        return Path.Combine("re", "tools", "captures",
            $"{DateTimeOffset.UtcNow:yyyyMMddTHHmmssZ}-{safe}.json");
    }

    private static bool ProbeStatus(DeviceLink link)
    {
        ExchangeOutcome probe = link.Exchange(
            MystiquePackets.DataPipeOut, MystiquePackets.DataPipeIn,
            Wire.Build(new PacketSpec(MystiquePackets.CommandStatus, [])));
        Console.WriteLine(probe.Received
            ? $"  status: {Wire.Hex(probe.Response)}"
            : $"  status: no response ({probe.Win32Error}) {probe.Error}");
        return probe.Received;
    }

    private static byte[] BaselinePayload() =>
        MystiquePackets.BuildDisplayPayload(new TelemetryFrame(36, 9, 39, 48, 4300));

    private static char Classify(byte value) => value switch
    {
        0 => '0',
        >= (byte)'0' and <= (byte)'9' => 'd',
        >= (byte)'A' and <= (byte)'F' => 'u',
        >= (byte)'a' and <= (byte)'f' => 'l',
        >= (byte)'A' and <= (byte)'Z' => 'a',
        >= (byte)'a' and <= (byte)'z' => 'a',
        >= 0x20 and <= 0x7E => 'p',
        _ => 'X',
    };

    private static string? TakeOption(List<string> args, string name)
    {
        int index = args.FindIndex(a => string.Equals(a, name, StringComparison.OrdinalIgnoreCase));
        if (index < 0 || index + 1 >= args.Count)
            return null;

        string value = args[index + 1];
        args.RemoveRange(index, 2);
        return value;
    }

    private static int? TakeIntOption(List<string> args, string name)
    {
        string? value = TakeOption(args, name);
        return value is not null && int.TryParse(value, out int parsed) ? parsed : null;
    }

    private sealed record CaptureDocument(
        string Tool,
        string Mode,
        string? ScriptName,
        string CapturedAtUtc,
        string DevicePathRedacted,
        IReadOnlyList<string> Pipes,
        IReadOnlyList<ExchangeView> Exchanges,
        string ResponseChecksumNote);

    private sealed record ExchangeView(
        int Index,
        string Name,
        string StartedAtUtc,
        long ElapsedMs,
        RequestView Request,
        ResponseView Response,
        int SettleMs);

    private sealed record RequestView(
        string PipeOut,
        string PipeIn,
        string Command,
        string PayloadHex,
        string PacketHex,
        int ChecksumBytes,
        int ChecksumAdjust,
        bool Hidc,
        string ChecksumHex);

    private sealed record ResponseView(
        bool Received,
        int Length,
        string PrefixHex,
        string? CommandEcho,
        string? StatusByte,
        string? RotationByte,
        string? ModeByte,
        int BodyNonZeroCount,
        string BodyRedactedReason,
        string TailHex,
        int TrailingNonZeroCount,
        bool? ChecksumValid,
        string ChecksumField,
        bool TimedOut,
        int? Win32Error,
        string? Error,
        string? FullHex);

    /// <summary>Accumulates exchanges and renders the redacted wire view.</summary>
    private sealed class Sink
    {
        private readonly List<ExchangeView> exchanges = [];
        private readonly List<string> pipes = [];

        public IReadOnlyList<ExchangeView> Exchanges => exchanges;

        public IReadOnlyList<string> Pipes => pipes;

        public void Record(
            DeviceLink link,
            string name,
            byte command,
            byte[] payload,
            int checksumAdjust,
            int checksumBytes,
            bool hidc,
            int settleMs,
            int sleepAfterMs,
            byte sync = 0xAA)
        {
            if (pipes.Count == 0)
                pipes.AddRange(link.Pipes.Select(p => $"0x{p.PipeId:X2} {p.Type} mps={p.MaximumPacketSize}"));

            (byte outPipe, byte inPipe, _) = PipeFor(command);
            byte[] packet = Wire.Build(new PacketSpec(command, payload, checksumAdjust, checksumBytes, hidc, sync));

            var clock = Stopwatch.StartNew();
            string startedAt = DateTimeOffset.UtcNow.ToString("O", CultureInfo.InvariantCulture);
            ExchangeOutcome outcome = link.Exchange(outPipe, inPipe, packet);
            clock.Stop();

            ResponseView view = Describe(command, outcome);
            exchanges.Add(new ExchangeView(
                exchanges.Count + 1,
                name,
                startedAt,
                clock.ElapsedMilliseconds,
                new RequestView(
                    $"0x{outPipe:X2}", $"0x{inPipe:X2}", $"0x{command:X2}",
                    Wire.Hex(payload), Wire.Hex(packet), checksumBytes, checksumAdjust, hidc,
                    $"0x{Wire.Sum(packet, 46):X4}"),
                view,
                settleMs));

            Console.WriteLine(
                $"  [{exchanges.Count,3}] {name,-34} len={view.Length,2} " +
                $"cmd={view.CommandEcho ?? "--"} status={view.StatusByte ?? "--"} " +
                $"cksum={(view.ChecksumValid is null ? "n/a" : view.ChecksumValid == true ? "ok" : "BAD")} " +
                $"{clock.ElapsedMilliseconds,5}ms" +
                (view.Received ? "" : $" <{view.Error}>"));

            if (sleepAfterMs > 0)
                Thread.Sleep(sleepAfterMs);
        }

        private static ResponseView Describe(byte command, ExchangeOutcome outcome)
        {
            if (!outcome.Received)
            {
                return new ResponseView(
                    false, 0, "", null, null, null, null, 0,
                    "no response", "", 0, null, "absent",
                    outcome.Win32Error is 121 or 1460, outcome.Win32Error is 0 ? null : outcome.Win32Error,
                    outcome.Error, null);
            }

            byte[] r = outcome.Response;
            int bodyStart = command == MystiquePackets.CommandStatus ? 6 : 4;
            int bodyEnd = Math.Min(42, r.Length);
            int nonZero = 0;
            for (int i = bodyStart; i < bodyEnd; i++)
            {
                if (r[i] != 0)
                    nonZero++;
            }

            int trailing = 0;
            for (int i = 48; i < r.Length; i++)
            {
                if (r[i] != 0)
                    trailing++;
            }

            bool? checksumValid = r.Length >= 48
                ? Wire.Sum(r, 46) == (ushort)(r[46] | r[47] << 8)
                : null;

            // A body that is entirely zero is provably identifier-free, so the
            // whole reply, checksum included, can be published byte-for-byte.
            string? fullHex = nonZero == 0 && trailing == 0 ? Wire.Hex(r) : null;

            return new ResponseView(
                true,
                r.Length,
                Wire.HexAt(r, 0, 4),
                Wire.HexByte(r, 2),
                Wire.HexByte(r, 3),
                command == MystiquePackets.CommandStatus ? Wire.HexByte(r, 4) : null,
                command == MystiquePackets.CommandStatus ? Wire.HexByte(r, 5) : null,
                nonZero,
                command == MystiquePackets.CommandStatus
                    ? "bytes 6..41 withheld (identifier region)"
                    : "bytes 4..41 withheld (identifier region)",
                Wire.HexAt(r, 42, 4),
                trailing,
                checksumValid,
                fullHex is null ? "withheld (device-specific)" : "included in fullHex",
                false,
                null,
                null,
                fullHex);
        }
    }

    /// <summary>Options that let the tests read one byte without reaching elsewhere.</summary>
    private sealed class ScriptDocument
    {
        public string? Name { get; set; }

        public string? Note { get; set; }

        public string? BasePayload { get; set; }

        public List<ScriptStep> Steps { get; set; } = [];
    }

    private sealed class ScriptStep
    {
        public string? Name { get; set; }

        public string? Cmd { get; set; }

        public string? Payload { get; set; }

        public Dictionary<string, string>? Patch { get; set; }

        public int SettleMs { get; set; } = 250;

        public int Repeat { get; set; } = 1;

        public int ChecksumAdjust { get; set; }

        public int ChecksumBytes { get; set; } = 46;

        public bool Hidc { get; set; } = true;
    }

    private sealed class RefusedException(string message) : Exception(message);
}
