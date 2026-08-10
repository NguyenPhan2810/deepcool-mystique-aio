# Mystique Core + Service Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extract the working USB/telemetry code into a reusable Core library, then drive the DeepCool Mystique LCD from a Windows Service that starts at boot, before any user logs on, and recovers from every failure without exiting.

**Architecture:** Two library projects (`Mystique.Core.Contracts`, `Mystique.Core.Device`) hold everything hardware- and protocol-related. `MystiqueService` runs in Session 0 as LocalSystem, owns the WinUSB handle for its entire lifetime, and runs a supervisor state machine that reconnects with two distinct backoff schedules. `MystiqueUsbTool` keeps all six diagnostic verbs and becomes a thin caller of Core. The tray app is **not** in this plan.

**Tech Stack:** C# / .NET 10 (`net10.0-windows`, SDK 10.0.302), WinUSB via P/Invoke, CfgMgr32 for device presence, `System.ServiceProcess.ServiceController` for `ServiceBase`, xUnit for tests.

**Source spec:** `docs/superpowers/specs/2026-08-06-mystique-service-design.md`

**Scope:** Spec phases 1 and 2 only. Phase 3 (tray + status pipe + SCM command codes) and Phase 4 (publishing, history rewrite) are separate plans. The device-ID redaction fix is pulled forward from Phase 4 into Task 9 because it is small, independent, and currently writes the device ID into every log line.

---

## Global Constraints

Every task's requirements implicitly include this section.

- **TFM:** `net10.0-windows` for every project. SDK 10.0.302 is installed; do not add a `global.json`.
- **Package budget:** Shipping projects may reference **only** `System.ServiceProcess.ServiceController` (on `MystiqueService` only) and its transitive `System.Diagnostics.EventLog`. Test-only packages are unconstrained. Do **not** add `Microsoft.Extensions.Hosting.WindowsServices`.
- **`Mystique.Core.*` must never set** `UseWindowsForms`, `UseWPF`, or a `Microsoft.WindowsDesktop.App` `FrameworkReference`. It is consumed by a headless service.
- **`MystiqueService` is `OutputType=Exe`**, never `WinExe` — a service must be a console-subsystem executable.
- **Only `MystiqueService` and `MystiqueUsbTool` ever open the USB device.** Nothing else calls `CreateFile` on a device path.
- **Device open share mode is `FILE_SHARE_READ | FILE_SHARE_WRITE`** — permissive sharing is what makes DeepCool takeover work in our favour. This is an invariant, not an accident.
- **Device identifier redaction is on by default** in every formatting path. Opt out only via an explicit CLI flag.
- **Never call `Environment.Exit` on a failure path** anywhere in `MystiqueService`.
- **Never use `ServiceSidType = restricted`.** `unrestricted` only.
- **No "send arbitrary packet" command** exists anywhere in the service's external surface.
- **Do not commit** `bin/`, `obj/`, `*.log`, or `deepcool-worker.js`.
- Protocol constants, verbatim: frame is 48 bytes, header `0xAA 0x2E <command>`, payload at offset 3 (max 39 bytes), `"HIDC"` = `0x48 0x49 0x44 0x43` at offsets 42-45, checksum = `ushort` sum of bytes 0..45 little-endian at 46-47. Response header is `0x55 0x2E <command|0x00>`. Machine Info mode is confirmed by status response byte 5 == `0xFF`. Rotation is init command `0x02` payload byte index 2, values `0x00`/`0x01`/`0x02`/`0x03` = 0°/90°/180°/270°.
- Device: `VID_3633&PID_0009`, interface GUID `{a5dcbf10-6530-11d2-901f-00c04fb951ed}`.
- Endpoints: init pair out `0x01` / in `0x81`; data pair out `0x02` / in `0x82`.

---

## File Structure

Existing layout is flat (project directories at repo root). Keep it flat.

**Create:**

| Path | Responsibility |
|---|---|
| `.gitignore` | exclude build output, logs, vendor artifacts |
| `Directory.Build.props` | shared TFM / nullable / warnings for all projects |
| `Mystique.slnx` | solution tying the five projects together |
| `Mystique.Core.Contracts/ServiceState.cs` | supervisor state enum |
| `Mystique.Core.Contracts/StatusSnapshot.cs` | immutable status record published to observers |
| `Mystique.Core.Contracts/TelemetryFrame.cs` | the five display values, already clamped |
| `Mystique.Core.Contracts/MystiqueCommand.cs` | SCM custom control code IDs (128-136) |
| `Mystique.Core.Device/Native.cs` | kernel32 / winusb / cfgmgr32 P/Invokes |
| `Mystique.Core.Device/UsbStructs.cs` | interop structs and enums |
| `Mystique.Core.Device/DeviceLocator.cs` | presence + path via `CM_Get_Device_Interface_List` |
| `Mystique.Core.Device/MystiquePackets.cs` | pure packet build/parse/format — no I/O |
| `Mystique.Core.Device/UsbFault.cs` | Win32 error code → handling class |
| `Mystique.Core.Device/MystiqueDevice.cs` | handle lifetime, pipe policy, exchange, init |
| `Mystique.Core.Device/SensorSample.cs` | one sensor reading |
| `Mystique.Core.Device/SensorBridgeClient.cs` | DeepCool sensor bridge named-pipe client |
| `Mystique.Core.Device/BackoffSchedule.cs` | fast/slow retry delays, reset-after-30 |
| `Mystique.Core.Device/FailureEscalator.cs` | log level by elapsed failure duration |
| `MystiqueService/Program.cs` | entry point, verb dispatch (`--service`/`--console`/`install`/`uninstall`) |
| `MystiqueService/MystiqueWindowsService.cs` | `ServiceBase` subclass |
| `MystiqueService/Supervisor.cs` | the state machine |
| `MystiqueService/DeepCoolWatcher.cs` | polls for `DeepCool.exe`, drives yield |
| `MystiqueService/RollingFileLog.cs` | timestamped rolling log + in-memory ring |
| `MystiqueService/ServiceInstaller.cs` | `sc.exe`-driven install/uninstall |
| `MystiqueService/MystiqueService.csproj` | |
| `Mystique.Tests/` | xUnit project, one test file per unit under test |

**Modify:**

| Path | Change |
|---|---|
| `MystiqueUsbTool/Program.cs:1-573` | gutted to a thin CLI over Core; all six verbs preserved |
| `MystiqueUsbTool/MystiqueUsbTool.csproj` | add `ProjectReference` to both Core projects; drop properties now in `Directory.Build.props` |
| `SensorPipeProbe/SensorPipeProbe.csproj` | add to solution; unchanged otherwise (deletion is deferred to the Phase 4 plan) |
| `README.md` | rewritten in Task 14 to document the service |

**Interface boundaries that matter:**

- `MystiquePackets` is **pure** — static methods over `byte[]`, no handles, no I/O, no `Console`. This is what makes protocol correctness unit-testable without hardware.
- `Mystique.Core.Device` never writes to `Console`. Callers decide output. This is what makes redaction a single choke point rather than 12 scattered `Console.WriteLine` calls.
- `Supervisor` depends on `IMystiqueDevice` / `ISensorSource` interfaces, never concrete types, so the state machine is testable with fakes.

---

## Task 1: Repo hygiene and solution scaffolding

Build artifacts and a 5.7 MB log are currently tracked in git. They must stop being tracked before any new code lands, or every subsequent commit carries rebuilt binaries. This task removes them from the **index only** — the existing history still contains them, which is fine while the repo is private and is fixed in the Phase 4 plan.

**Files:**
- Create: `.gitignore`, `Directory.Build.props`, `Mystique.slnx`
- Create: `Mystique.Tests/Mystique.Tests.csproj`, `Mystique.Tests/ScaffoldingTests.cs`
- Modify: `MystiqueUsbTool/MystiqueUsbTool.csproj`, `SensorPipeProbe/SensorPipeProbe.csproj`

**Interfaces:**
- Consumes: nothing
- Produces: a solution that builds, and `dotnet test` as the verification command every later task uses

- [ ] **Step 1: Write `.gitignore`**

```gitignore
# Build output
bin/
obj/
*.user
*.suo
.vs/

# Logs — these contain the raw device identifier in hex byte pairs.
# Never scrub and commit; exclude wholesale.
*.log

# Vendor material extracted from DeepCool's app.asar. Not ours to publish.
deepcool-worker.js
```

- [ ] **Step 2: Untrack build output and logs**

```bash
git rm -r --cached MystiqueUsbTool/bin MystiqueUsbTool/obj SensorPipeProbe/bin SensorPipeProbe/obj
git rm --cached mystique-live.log mystique-live-error.log deepcool-worker.js
```

Note `git rm --cached` leaves the files on disk. `deepcool-worker.js` stays locally for reference; the Phase 4 plan moves it out of the tree entirely.

- [ ] **Step 3: Verify nothing tracked is a build artifact**

Run: `git ls-files | grep -E "(^|/)(bin|obj)/|\.log$|deepcool-worker" ; echo "exit=$?"`
Expected: no output, `exit=1` (grep found nothing).

- [ ] **Step 4: Write `Directory.Build.props`**

```xml
<Project>
  <PropertyGroup>
    <TargetFramework>net10.0-windows</TargetFramework>
    <ImplicitUsings>enable</ImplicitUsings>
    <Nullable>enable</Nullable>
    <LangVersion>latest</LangVersion>
    <TreatWarningsAsErrors>true</TreatWarningsAsErrors>
    <InvariantGlobalization>true</InvariantGlobalization>
    <SatelliteResourceLanguages>en</SatelliteResourceLanguages>
  </PropertyGroup>
</Project>
```

- [ ] **Step 5: Strip now-redundant properties from the two existing csproj files**

`MystiqueUsbTool/MystiqueUsbTool.csproj` becomes:

```xml
<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <OutputType>Exe</OutputType>
    <AssemblyName>MystiqueUsbTool</AssemblyName>
  </PropertyGroup>
</Project>
```

`SensorPipeProbe/SensorPipeProbe.csproj` becomes:

```xml
<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <OutputType>Exe</OutputType>
  </PropertyGroup>
</Project>
```

- [ ] **Step 6: Create the test project**

```bash
dotnet new xunit -n Mystique.Tests -o Mystique.Tests
```

Then open `Mystique.Tests/Mystique.Tests.csproj` and **delete the `<TargetFramework>` line** the template wrote. `Directory.Build.props` is imported before the project body, so a template-written `TargetFramework` would win and pin the tests to the wrong TFM. Also delete the template's generated `UnitTest1.cs`.

- [ ] **Step 7: Write a scaffolding test**

`Mystique.Tests/ScaffoldingTests.cs`:

```csharp
namespace Mystique.Tests;

public class ScaffoldingTests
{
    [Fact]
    public void TestProjectTargetsWindowsTfm()
    {
        Assert.True(OperatingSystem.IsWindows());
    }
}
```

- [ ] **Step 8: Create the solution and add every project**

```bash
dotnet new sln -n Mystique   # .NET 10 emits Mystique.slnx, the XML solution format
dotnet sln add MystiqueUsbTool/MystiqueUsbTool.csproj SensorPipeProbe/SensorPipeProbe.csproj Mystique.Tests/Mystique.Tests.csproj
```

- [ ] **Step 9: Build and test**

Run: `dotnet build Mystique.slnx -c Release && dotnet test Mystique.slnx -c Release`
Expected: build succeeds with zero warnings; 1 test passes.

If `TreatWarningsAsErrors` surfaces pre-existing warnings in `MystiqueUsbTool`, fix them rather than lowering the setting — the current code is small and warning-free apart from anything the stricter settings newly reveal.

- [ ] **Step 10: Commit**

```bash
git add .gitignore Directory.Build.props Mystique.slnx Mystique.Tests MystiqueUsbTool/MystiqueUsbTool.csproj SensorPipeProbe/SensorPipeProbe.csproj
git commit -m "chore: add solution, shared build props, test project; untrack build output"
```

---

## Task 2: Contracts project

Small, dependency-free types shared by the service, the tool, and (later) the tray. Splitting these from `Mystique.Core.Device` is what will make "the tray cannot open the device" a compile error in the Phase 3 plan.

**Files:**
- Create: `Mystique.Core.Contracts/Mystique.Core.Contracts.csproj`, `ServiceState.cs`, `TelemetryFrame.cs`, `StatusSnapshot.cs`, `MystiqueCommand.cs`
- Test: `Mystique.Tests/ContractsTests.cs`

**Interfaces:**
- Consumes: nothing
- Produces:
  - `enum ServiceState { Starting, Streaming, Degraded, Reconnecting, Waiting, Yielded, Stopped }`
  - `readonly record struct TelemetryFrame(byte CpuTemperatureC, byte CpuUsagePercent, byte MemoryLoadPercent, byte GpuTemperatureC, ushort CpuClockMhz)`
  - `sealed record StatusSnapshot(ServiceState State, string Detail, long UpdatesSent, DateTimeOffset SinceUtc, byte Rotation, TelemetryFrame? Last)`
  - `static class MystiqueCommand` with `const int RotateNext = 128, Rotate0 = 129, Rotate90 = 130, Rotate180 = 131, Rotate270 = 132, ReleaseToDeepCool = 133, Reacquire = 134, RetryNow = 135, ReloadConfig = 136`

- [ ] **Step 1: Write the failing test**

`Mystique.Tests/ContractsTests.cs`:

```csharp
using Mystique.Core.Contracts;

namespace Mystique.Tests;

public class ContractsTests
{
    [Fact]
    public void ServiceStateCoversEveryDocumentedState()
    {
        var names = Enum.GetNames<ServiceState>();
        Assert.Equal(
            new[] { "Starting", "Streaming", "Degraded", "Reconnecting", "Waiting", "Yielded", "Stopped" },
            names);
    }

    [Fact]
    public void CommandIdsAreInTheScmCustomControlRange()
    {
        int[] ids =
        [
            MystiqueCommand.RotateNext, MystiqueCommand.Rotate0, MystiqueCommand.Rotate90,
            MystiqueCommand.Rotate180, MystiqueCommand.Rotate270, MystiqueCommand.ReleaseToDeepCool,
            MystiqueCommand.Reacquire, MystiqueCommand.RetryNow, MystiqueCommand.ReloadConfig,
        ];

        Assert.All(ids, id => Assert.InRange(id, 128, 255));
        Assert.Equal(ids.Length, ids.Distinct().Count());
    }

    [Fact]
    public void TelemetryFrameIsValueEqual()
    {
        var a = new TelemetryFrame(55, 12, 40, 48, 4600);
        var b = new TelemetryFrame(55, 12, 40, 48, 4600);
        Assert.Equal(a, b);
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `dotnet test Mystique.slnx -c Release`
Expected: FAIL — `CS0246: The type or namespace name 'Mystique' could not be found`.

- [ ] **Step 3: Create the project**

```bash
dotnet new classlib -n Mystique.Core.Contracts -o Mystique.Core.Contracts
rm Mystique.Core.Contracts/Class1.cs
dotnet sln add Mystique.Core.Contracts/Mystique.Core.Contracts.csproj
dotnet add Mystique.Tests/Mystique.Tests.csproj reference Mystique.Core.Contracts/Mystique.Core.Contracts.csproj
```

Delete the `<TargetFramework>` line from the generated `Mystique.Core.Contracts.csproj` for the same reason as Task 1 Step 6. The final file:

```xml
<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <RootNamespace>Mystique.Core.Contracts</RootNamespace>
  </PropertyGroup>
</Project>
```

- [ ] **Step 4: Write the implementation**

`Mystique.Core.Contracts/ServiceState.cs`:

```csharp
namespace Mystique.Core.Contracts;

/// <summary>Closed set of supervisor states. Order matches the documented lifecycle.</summary>
public enum ServiceState
{
    /// <summary>Process started, nothing acquired yet.</summary>
    Starting,

    /// <summary>Device owned, in Machine Info mode, frames flowing.</summary>
    Streaming,

    /// <summary>Device owned but the sensor source is unavailable; no frames.</summary>
    Degraded,

    /// <summary>Device lost; actively retrying the open/init sequence.</summary>
    Reconnecting,

    /// <summary>Device not present at all. Waiting for arrival notification.</summary>
    Waiting,

    /// <summary>DeepCool.exe is running. We hold no handle by choice.</summary>
    Yielded,

    /// <summary>Gave up or was stopped. Never entered silently.</summary>
    Stopped,
}
```

`Mystique.Core.Contracts/TelemetryFrame.cs`:

```csharp
namespace Mystique.Core.Contracts;

/// <summary>
/// The five values the Machine Info display shows, already clamped to the
/// ranges the wire format allows. Constructing one cannot produce an
/// out-of-range packet.
/// </summary>
public readonly record struct TelemetryFrame(
    byte CpuTemperatureC,
    byte CpuUsagePercent,
    byte MemoryLoadPercent,
    byte GpuTemperatureC,
    ushort CpuClockMhz)
{
    public static TelemetryFrame FromRaw(
        double cpuTemperature,
        double cpuUsage,
        double memoryLoad,
        double gpuTemperature,
        double cpuClock) =>
        new(
            Clamp(cpuTemperature, 0, 127),
            Clamp(cpuUsage, 0, 100),
            Clamp(memoryLoad, 0, 100),
            Clamp(gpuTemperature, 0, 127),
            (ushort)Math.Clamp((int)Math.Round(cpuClock), 0, ushort.MaxValue));

    private static byte Clamp(double value, int minimum, int maximum) =>
        (byte)Math.Clamp((int)Math.Round(value), minimum, maximum);
}
```

`Mystique.Core.Contracts/StatusSnapshot.cs`:

```csharp
namespace Mystique.Core.Contracts;

/// <summary>
/// Immutable point-in-time view of the supervisor, published to observers.
/// The USB loop constructs one of these and never touches an observer directly,
/// so a hung observer can never block a frame.
/// </summary>
public sealed record StatusSnapshot(
    ServiceState State,
    string Detail,
    long UpdatesSent,
    DateTimeOffset SinceUtc,
    byte Rotation,
    TelemetryFrame? Last)
{
    public static StatusSnapshot Initial(DateTimeOffset nowUtc, byte rotation) =>
        new(ServiceState.Starting, "starting", 0, nowUtc, rotation, null);
}
```

`Mystique.Core.Contracts/MystiqueCommand.cs`:

```csharp
namespace Mystique.Core.Contracts;

/// <summary>
/// SCM custom control codes the tray may send. Windows restricts these to
/// principals holding SERVICE_USER_DEFINED_CONTROL, so the service parses no
/// untrusted bytes — only an integer in this fixed set.
///
/// Deliberately absent: any command carrying a payload, a file path, or a raw
/// packet. Such a command would be a local privilege-escalation primitive
/// against a LocalSystem service.
/// </summary>
public static class MystiqueCommand
{
    public const int RotateNext = 128;
    public const int Rotate0 = 129;
    public const int Rotate90 = 130;
    public const int Rotate180 = 131;
    public const int Rotate270 = 132;
    public const int ReleaseToDeepCool = 133;
    public const int Reacquire = 134;
    public const int RetryNow = 135;
    public const int ReloadConfig = 136;

    /// <summary>Maps a rotation command to its wire value, or null if not a rotation command.</summary>
    public static byte? ToRotation(int command) => command switch
    {
        Rotate0 => 0,
        Rotate90 => 1,
        Rotate180 => 2,
        Rotate270 => 3,
        _ => null,
    };
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `dotnet test Mystique.slnx -c Release`
Expected: PASS, no failures.

- [ ] **Step 6: Commit**

```bash
git add Mystique.Core.Contracts Mystique.Tests Mystique.slnx
git commit -m "feat: add Mystique.Core.Contracts with state, telemetry, and command types"
```

---

## Task 3: MystiquePackets — the pure protocol layer

Everything about the wire format, with zero I/O. This is the only part of the USB path that can be tested exhaustively without hardware, so it gets the most tests.

**Files:**
- Create: `Mystique.Core.Device/Mystique.Core.Device.csproj`, `Mystique.Core.Device/MystiquePackets.cs`
- Test: `Mystique.Tests/MystiquePacketsTests.cs`
- Reference: current implementation at `MystiqueUsbTool/Program.cs:187-209` (payload), `:300-321` (packet), `:323-332` (validation), `:334-347` (status/format), `:138-153` (init table)

**Interfaces:**
- Consumes: `TelemetryFrame` from Task 2
- Produces:
  - `static byte[] MystiquePackets.BuildPacket(byte command, ReadOnlySpan<byte> payload)`
  - `static byte[] MystiquePackets.BuildDisplayPayload(in TelemetryFrame frame)`
  - `static void MystiquePackets.ValidateResponse(ReadOnlySpan<byte> response, byte command)`
  - `static DeviceStatus MystiquePackets.ParseStatus(ReadOnlySpan<byte> response)`
  - `static bool MystiquePackets.IsInMachineInfoMode(ReadOnlySpan<byte> statusResponse)`
  - `static string MystiquePackets.FormatResponse(ReadOnlySpan<byte> response, bool showDeviceId = false)`
  - `static IReadOnlyList<InitCommand> MystiquePackets.BuildInitSequence(byte rotation)`
  - `readonly record struct InitCommand(byte Command, byte[] Payload, int SettleMilliseconds)`
  - `readonly record struct DeviceStatus(DisplayMode Mode, byte RawMode, byte Rotation)` with `int RotationDegrees`
  - `enum DisplayMode { Unknown, MachineInfo, ImageOrGif }`
  - constants `InitPipeOut = 0x01`, `InitPipeIn = 0x81`, `DataPipeOut = 0x02`, `DataPipeIn = 0x82`, `CommandInfo = 0x12`, `CommandStatus = 0x10`, `CommandTelemetry = 0x01`

- [ ] **Step 1: Write the failing tests**

`Mystique.Tests/MystiquePacketsTests.cs`:

```csharp
using Mystique.Core.Contracts;
using Mystique.Core.Device;

namespace Mystique.Tests;

public class MystiquePacketsTests
{
    [Fact]
    public void PacketHasFixedFrameStructure()
    {
        byte[] packet = MystiquePackets.BuildPacket(0x12, []);

        Assert.Equal(48, packet.Length);
        Assert.Equal(0xAA, packet[0]);
        Assert.Equal(0x2E, packet[1]);
        Assert.Equal(0x12, packet[2]);
        Assert.Equal(new byte[] { 0x48, 0x49, 0x44, 0x43 }, packet[42..46]);
    }

    [Fact]
    public void ChecksumIsLittleEndianSumOfFirst46Bytes()
    {
        // 0xAA + 0x2E + 0x12 + 'H' + 'I' + 'D' + 'C' = 514 = 0x0202
        byte[] packet = MystiquePackets.BuildPacket(0x12, []);
        Assert.Equal(0x02, packet[46]);
        Assert.Equal(0x02, packet[47]);
    }

    [Fact]
    public void ChecksumAccountsForPayloadBytes()
    {
        // 514 - 0x12 + 0x01 + 55 = 552 = 0x0228
        byte[] packet = MystiquePackets.BuildPacket(0x01, [55]);
        Assert.Equal(0x28, packet[46]);
        Assert.Equal(0x02, packet[47]);

        int recomputed = 0;
        for (int i = 0; i < 46; i++)
            recomputed += packet[i];
        Assert.Equal(recomputed & 0xFFFF, packet[46] | (packet[47] << 8));
    }

    [Fact]
    public void PayloadLandsAtOffsetThree()
    {
        byte[] packet = MystiquePackets.BuildPacket(0x01, [0x11, 0x22, 0x33]);
        Assert.Equal(0x11, packet[3]);
        Assert.Equal(0x22, packet[4]);
        Assert.Equal(0x33, packet[5]);
        Assert.Equal(0x00, packet[6]);
    }

    [Fact]
    public void PayloadLongerThanThirtyNineBytesIsRejected()
    {
        Assert.Throws<ArgumentOutOfRangeException>(
            () => MystiquePackets.BuildPacket(0x01, new byte[40]));
    }

    [Fact]
    public void DisplayPayloadPlacesEveryFieldAtItsDocumentedOffset()
    {
        var frame = new TelemetryFrame(
            CpuTemperatureC: 55, CpuUsagePercent: 12, MemoryLoadPercent: 40,
            GpuTemperatureC: 48, CpuClockMhz: 4600);

        byte[] payload = MystiquePackets.BuildDisplayPayload(frame);

        Assert.Equal(39, payload.Length);
        Assert.Equal(55, payload[0]);    // CPU temperature
        Assert.Equal(12, payload[3]);    // CPU usage
        Assert.Equal(40, payload[6]);    // memory load
        Assert.Equal(0x09, payload[8]);  // GPU icon bucket for >= 38 C
        Assert.Equal(0x03, payload[9]);
        Assert.Equal(48, payload[11]);   // GPU temperature
        Assert.Equal(0x05, payload[12]);
        Assert.Equal(0x06, payload[14]);
        Assert.Equal(0x0C, payload[15]);
        Assert.Equal(0x07, payload[17]);
        Assert.Equal(4, payload[18]);    // GHz integer part
        Assert.Equal(60, payload[20]);   // hundredths, floored at 0x0A
        Assert.Equal(0x70, payload[21]); // 6000 low byte
        Assert.Equal(0x17, payload[22]); // 6000 high byte
        Assert.Equal(payload[21], payload[24]);
        Assert.Equal(payload[22], payload[25]);
    }

    [Theory]
    [InlineData((byte)30, (byte)0x05)]
    [InlineData((byte)36, (byte)0x06)]
    [InlineData((byte)48, (byte)0x09)]
    public void GpuIconBucketFollowsTemperatureThresholds(byte gpuTemp, byte expected)
    {
        var frame = new TelemetryFrame(50, 10, 40, gpuTemp, 4000);
        Assert.Equal(expected, MystiquePackets.BuildDisplayPayload(frame)[8]);
    }

    [Fact]
    public void ClockHundredthsNeverDropBelowTheFloor()
    {
        // 4005 MHz -> 4005 % 1000 / 10 == 0, which must be raised to 0x0A.
        var frame = new TelemetryFrame(50, 10, 40, 40, 4005);
        Assert.Equal(0x0A, MystiquePackets.BuildDisplayPayload(frame)[20]);
    }

    [Fact]
    public void InitSequenceHasThirteenCommandsInTheVerifiedOrder()
    {
        var sequence = MystiquePackets.BuildInitSequence(rotation: 0);

        Assert.Equal(13, sequence.Count);
        Assert.Equal(
            new byte[] { 0x12, 0x02, 0x03, 0x04, 0x07, 0x08, 0x05, 0x0B, 0x06, 0x15, 0x16, 0x17, 0x0A },
            sequence.Select(c => c.Command).ToArray());
    }

    [Theory]
    [InlineData((byte)0)]
    [InlineData((byte)1)]
    [InlineData((byte)2)]
    [InlineData((byte)3)]
    public void RotationIsPayloadByteTwoOfCommand02(byte rotation)
    {
        var command02 = MystiquePackets.BuildInitSequence(rotation).Single(c => c.Command == 0x02);

        Assert.Equal(new byte[] { 0x01, 0x00, rotation, 0x01, 0x24 }, command02.Payload);
    }

    [Fact]
    public void RotationAboveThreeIsRejected()
    {
        Assert.Throws<ArgumentOutOfRangeException>(() => MystiquePackets.BuildInitSequence(4));
    }

    [Fact]
    public void CommandZeroAGetsTheLongerSettleDelay()
    {
        var sequence = MystiquePackets.BuildInitSequence(0);
        Assert.Equal(100, sequence.Single(c => c.Command == 0x0A).SettleMilliseconds);
        Assert.All(sequence.Where(c => c.Command != 0x0A), c => Assert.Equal(10, c.SettleMilliseconds));
    }

    [Fact]
    public void ValidResponseIsAccepted()
    {
        MystiquePackets.ValidateResponse([0x55, 0x2E, 0x10, 0x00], 0x10);
        MystiquePackets.ValidateResponse([0x55, 0x2E, 0x00, 0x00], 0x10); // 0x00 is a wildcard ack
    }

    [Fact]
    public void ShortResponseIsRejected()
    {
        Assert.Throws<InvalidDataException>(() => MystiquePackets.ValidateResponse([0x55, 0x2E], 0x10));
    }

    [Fact]
    public void WrongResponseHeaderIsRejected()
    {
        Assert.Throws<InvalidDataException>(
            () => MystiquePackets.ValidateResponse([0xAA, 0x2E, 0x10], 0x10));
    }

    [Fact]
    public void MismatchedResponseCommandIsRejected()
    {
        Assert.Throws<InvalidDataException>(
            () => MystiquePackets.ValidateResponse([0x55, 0x2E, 0x11], 0x10));
    }

    [Fact]
    public void StatusParsesModeAndRotation()
    {
        // byte 4 = rotation index, byte 5 = mode
        var status = MystiquePackets.ParseStatus([0x55, 0x2E, 0x10, 0x00, 0x02, 0xFF]);

        Assert.Equal(DisplayMode.MachineInfo, status.Mode);
        Assert.Equal(2, status.Rotation);
        Assert.Equal(180, status.RotationDegrees);
        Assert.True(MystiquePackets.IsInMachineInfoMode([0x55, 0x2E, 0x10, 0x00, 0x02, 0xFF]));
    }

    [Fact]
    public void StatusRecognisesImageMode()
    {
        var status = MystiquePackets.ParseStatus([0x55, 0x2E, 0x10, 0x00, 0x00, 0x00]);
        Assert.Equal(DisplayMode.ImageOrGif, status.Mode);
    }

    [Fact]
    public void StatusTooShortToCarryModeIsUnknown()
    {
        var status = MystiquePackets.ParseStatus([0x55, 0x2E, 0x10]);
        Assert.Equal(DisplayMode.Unknown, status.Mode);
        Assert.False(MystiquePackets.IsInMachineInfoMode([0x55, 0x2E, 0x10]));
    }

    [Fact]
    public void FormatRedactsTheDeviceIdentifierByDefault()
    {
        // Real capture: bytes 4-11 are ASCII "AABBCCDD", a per-device identifier.
        byte[] response = [0x55, 0x2E, 0x12, 0x00, 0x41, 0x41, 0x42, 0x42, 0x43, 0x43, 0x44, 0x44];

        string formatted = MystiquePackets.FormatResponse(response);

        Assert.Equal("55 2E 12 00 .. .. .. .. .. .. .. ..", formatted);
        Assert.DoesNotContain("41 41", formatted);
    }

    [Fact]
    public void FormatCanShowTheDeviceIdentifierWhenExplicitlyRequested()
    {
        byte[] response = [0x55, 0x2E, 0x12, 0x00, 0x41, 0x41, 0x42, 0x42, 0x43, 0x43, 0x44, 0x44];

        string formatted = MystiquePackets.FormatResponse(response, showDeviceId: true);

        Assert.Equal("55 2E 12 00 41 41 42 42 43 43 44 44", formatted);
    }

    [Fact]
    public void FormatHandlesResponsesShorterThanTheRedactionWindow()
    {
        Assert.Equal("55 2E 10 00 ..", MystiquePackets.FormatResponse([0x55, 0x2E, 0x10, 0x00, 0x07]));
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `dotnet test Mystique.slnx -c Release`
Expected: FAIL — `CS0246: The type or namespace name 'Device' does not exist in the namespace 'Mystique.Core'`.

- [ ] **Step 3: Create the project**

```bash
dotnet new classlib -n Mystique.Core.Device -o Mystique.Core.Device
rm Mystique.Core.Device/Class1.cs
dotnet sln add Mystique.Core.Device/Mystique.Core.Device.csproj
dotnet add Mystique.Core.Device/Mystique.Core.Device.csproj reference Mystique.Core.Contracts/Mystique.Core.Contracts.csproj
dotnet add Mystique.Tests/Mystique.Tests.csproj reference Mystique.Core.Device/Mystique.Core.Device.csproj
```

Delete the generated `<TargetFramework>` line. Final `Mystique.Core.Device.csproj`:

```xml
<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <RootNamespace>Mystique.Core.Device</RootNamespace>
  </PropertyGroup>
  <ItemGroup>
    <ProjectReference Include="..\Mystique.Core.Contracts\Mystique.Core.Contracts.csproj" />
  </ItemGroup>
</Project>
```

- [ ] **Step 4: Write the implementation**

`Mystique.Core.Device/MystiquePackets.cs`:

```csharp
using Mystique.Core.Contracts;

namespace Mystique.Core.Device;

public enum DisplayMode
{
    Unknown,
    MachineInfo,
    ImageOrGif,
}

/// <summary>One step of the Machine Info initialization handshake.</summary>
public readonly record struct InitCommand(byte Command, byte[] Payload, int SettleMilliseconds);

/// <summary>Decoded reply to status command 0x10.</summary>
public readonly record struct DeviceStatus(DisplayMode Mode, byte RawMode, byte Rotation)
{
    public int RotationDegrees => Rotation * 90;
}

/// <summary>
/// Pure wire-format layer: builds and parses packets, performs no I/O and never
/// writes to the console. Every constant here was confirmed against a live device.
/// </summary>
public static class MystiquePackets
{
    public const int PacketLength = 48;
    public const int MaxPayloadLength = 39;

    /// <summary>Endpoint pair used for initialization and info commands.</summary>
    public const byte InitPipeOut = 0x01;
    public const byte InitPipeIn = 0x81;

    /// <summary>Endpoint pair used for status and telemetry.</summary>
    public const byte DataPipeOut = 0x02;
    public const byte DataPipeIn = 0x82;

    public const byte CommandInfo = 0x12;
    public const byte CommandStatus = 0x10;
    public const byte CommandTelemetry = 0x01;

    /// <summary>
    /// Response bytes 4-11 carry a per-device identifier. They are masked in every
    /// formatting path unless a caller explicitly opts in.
    /// </summary>
    private const int DeviceIdOffset = 4;
    private const int DeviceIdLength = 8;
    private const int FormatPrefixLength = 12;

    public static byte[] BuildPacket(byte command, ReadOnlySpan<byte> payload)
    {
        if (payload.Length > MaxPayloadLength)
            throw new ArgumentOutOfRangeException(
                nameof(payload),
                payload.Length,
                $"Payload must be at most {MaxPayloadLength} bytes.");

        byte[] packet = new byte[PacketLength];
        packet[0] = 0xAA;
        packet[1] = 0x2E;
        packet[2] = command;
        payload.CopyTo(packet.AsSpan(3));
        packet[42] = 0x48; // 'H'
        packet[43] = 0x49; // 'I'
        packet[44] = 0x44; // 'D'
        packet[45] = 0x43; // 'C'

        ushort checksum = 0;
        for (int i = 0; i < 46; i++)
            checksum += packet[i];
        packet[46] = (byte)(checksum & 0xFF);
        packet[47] = (byte)(checksum >> 8);
        return packet;
    }

    public static byte[] BuildDisplayPayload(in TelemetryFrame frame)
    {
        byte[] payload = new byte[MaxPayloadLength];
        payload[0] = frame.CpuTemperatureC;
        payload[3] = frame.CpuUsagePercent;
        payload[6] = frame.MemoryLoadPercent;
        payload[8] = frame.GpuTemperatureC < 35 ? (byte)0x05
                   : frame.GpuTemperatureC < 38 ? (byte)0x06
                   : (byte)0x09;
        payload[9] = 0x03;
        payload[11] = frame.GpuTemperatureC;
        payload[12] = 0x05;
        payload[14] = 0x06;
        payload[15] = 0x0C;
        payload[17] = 0x07;
        payload[18] = (byte)(frame.CpuClockMhz / 1000);
        payload[20] = (byte)Math.Max(0x0A, frame.CpuClockMhz % 1000 / 10);

        ushort decimalDisplay = (ushort)(frame.CpuClockMhz % 1000 / 10 * 100);
        payload[21] = (byte)(decimalDisplay & 0xFF);
        payload[22] = (byte)(decimalDisplay >> 8);
        payload[24] = payload[21];
        payload[25] = payload[22];
        return payload;
    }

    /// <summary>
    /// The 13-command Machine Info handshake, in the order confirmed against the
    /// device. The order is not ascending by command byte; do not "tidy" it.
    /// </summary>
    public static IReadOnlyList<InitCommand> BuildInitSequence(byte rotation)
    {
        if (rotation > 3)
            throw new ArgumentOutOfRangeException(
                nameof(rotation), rotation, "Rotation must be 0-3 (0/90/180/270 degrees).");

        return
        [
            new(0x12, [], 10),
            new(0x02, [0x01, 0x00, rotation, 0x01, 0x24], 10),
            new(0x03, [0x01], 10),
            new(0x04, [0x05, 0x00, 0x00, 0x01], 10),
            new(0x07, [0x00, 0x02], 10),
            new(0x08, [0x00, 0x04], 10),
            new(0x05, [0x01, 0x01], 10),
            new(0x0B, [], 10),
            new(0x06, [0x01], 10),
            new(0x15, [0x2D, 0x2D], 10),
            new(0x16, [0x2D, 0x2D], 10),
            new(0x17, [0x2D, 0x2D], 10),
            new(0x0A, [0xEA, 0x07, 0x02, 0x02, 0x02, 0x27, 0x21], 100),
        ];
    }

    public static void ValidateResponse(ReadOnlySpan<byte> response, byte command)
    {
        if (response.Length < 3)
            throw new InvalidDataException(
                $"Short response to command 0x{command:X2}: {response.Length} bytes");
        if (response[0] != 0x55 || response[1] != 0x2E)
            throw new InvalidDataException(
                $"Unexpected response header: {FormatResponse(response)}");
        if (response[2] != command && response[2] != 0x00)
            throw new InvalidDataException(
                $"Response command mismatch: sent 0x{command:X2}, received 0x{response[2]:X2}");
    }

    public static DeviceStatus ParseStatus(ReadOnlySpan<byte> response)
    {
        byte rawMode = response.Length >= 6 ? response[5] : (byte)0;
        DisplayMode mode = response.Length < 6 ? DisplayMode.Unknown : rawMode switch
        {
            0xFF => DisplayMode.MachineInfo,
            0x00 => DisplayMode.ImageOrGif,
            _ => DisplayMode.Unknown,
        };
        byte rotation = response.Length >= 5 ? response[4] : (byte)0;
        return new DeviceStatus(mode, rawMode, rotation);
    }

    public static bool IsInMachineInfoMode(ReadOnlySpan<byte> statusResponse) =>
        ParseStatus(statusResponse).Mode == DisplayMode.MachineInfo;

    /// <summary>
    /// Hex-dumps the leading bytes of a response with the device identifier masked.
    /// Redaction is the default because the service logs unattended; the raw form
    /// exists only for an explicit, interactive opt-in.
    /// </summary>
    public static string FormatResponse(ReadOnlySpan<byte> response, bool showDeviceId = false)
    {
        int length = Math.Min(response.Length, FormatPrefixLength);
        var parts = new string[length];
        for (int i = 0; i < length; i++)
        {
            bool redact = !showDeviceId
                && i >= DeviceIdOffset
                && i < DeviceIdOffset + DeviceIdLength;
            parts[i] = redact ? ".." : response[i].ToString("X2");
        }

        return string.Join(' ', parts);
    }
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `dotnet test Mystique.slnx -c Release --filter FullyQualifiedName~MystiquePacketsTests`
Expected: PASS, no failures.

- [ ] **Step 6: Commit**

```bash
git add Mystique.Core.Device Mystique.Tests Mystique.slnx
git commit -m "feat: add pure packet layer with device-id redaction and parameterised rotation"
```

---

## Task 4: Win32 error taxonomy

Turns raw Win32 codes into a handling decision. Without this the supervisor either treats "device not enumerated yet at boot" as a fault (noisy, and the panel stays dark while backoff climbs) or treats a genuine stall as normal (silent freeze).

**Files:**
- Create: `Mystique.Core.Device/UsbFault.cs`
- Test: `Mystique.Tests/UsbFaultTests.cs`

**Interfaces:**
- Consumes: nothing
- Produces:
  - `enum FaultKind { NotPresent, Yield, Transient, Ambiguous, SurpriseRemoval, SelfAborted, Programming }`
  - `enum RecoveryAction { None, AbortAndFlush, AbortFlushReset, Reopen, WaitForArrival }`
  - `readonly record struct UsbFault(int Code, FaultKind Kind, RecoveryAction Recovery, bool UseFastBackoff, string Description)`
  - `static UsbFault UsbFault.Classify(int win32Code, bool weInitiatedAbort = false)`

- [ ] **Step 1: Write the failing tests**

`Mystique.Tests/UsbFaultTests.cs`:

```csharp
using Mystique.Core.Device;

namespace Mystique.Tests;

public class UsbFaultTests
{
    [Fact]
    public void FileNotFoundIsNormalAtBootNotAFault()
    {
        var fault = UsbFault.Classify(2);

        Assert.Equal(FaultKind.NotPresent, fault.Kind);
        Assert.Equal(RecoveryAction.WaitForArrival, fault.Recovery);
        Assert.True(fault.UseFastBackoff);
    }

    [Theory]
    [InlineData(5)]   // ERROR_ACCESS_DENIED
    [InlineData(32)]  // ERROR_SHARING_VIOLATION
    public void AnotherOwnerIsAYieldNotAnError(int code)
    {
        var fault = UsbFault.Classify(code);

        Assert.Equal(FaultKind.Yield, fault.Kind);
        Assert.Equal(RecoveryAction.None, fault.Recovery);
        Assert.True(fault.UseFastBackoff);
    }

    [Fact]
    public void PipeTimeoutAbortsAndFlushesBeforeRetrying()
    {
        var fault = UsbFault.Classify(121); // ERROR_SEM_TIMEOUT

        Assert.Equal(FaultKind.Transient, fault.Kind);
        Assert.Equal(RecoveryAction.AbortAndFlush, fault.Recovery);
        Assert.False(fault.UseFastBackoff);
    }

    [Fact]
    public void GeneralFailureGetsTheFullPipeReset()
    {
        var fault = UsbFault.Classify(31); // ERROR_GEN_FAILURE

        Assert.Equal(FaultKind.Ambiguous, fault.Kind);
        Assert.Equal(RecoveryAction.AbortFlushReset, fault.Recovery);
    }

    [Theory]
    [InlineData(433)]  // ERROR_NO_SUCH_DEVICE
    [InlineData(1167)] // ERROR_DEVICE_NOT_CONNECTED
    public void SurpriseRemovalSkipsPipeRecoveryAndWaits(int code)
    {
        var fault = UsbFault.Classify(code);

        Assert.Equal(FaultKind.SurpriseRemoval, fault.Kind);
        Assert.Equal(RecoveryAction.WaitForArrival, fault.Recovery);
        Assert.True(fault.UseFastBackoff);
    }

    [Fact]
    public void OperationAbortedIsSwallowedWhenWeInitiatedIt()
    {
        var fault = UsbFault.Classify(995, weInitiatedAbort: true);

        Assert.Equal(FaultKind.SelfAborted, fault.Kind);
        Assert.Equal(RecoveryAction.None, fault.Recovery);
    }

    [Fact]
    public void OperationAbortedMeansDeviceGoneWhenWeDidNotInitiateIt()
    {
        var fault = UsbFault.Classify(995, weInitiatedAbort: false);

        Assert.Equal(FaultKind.SurpriseRemoval, fault.Kind);
        Assert.Equal(RecoveryAction.WaitForArrival, fault.Recovery);
    }

    [Fact]
    public void InvalidHandleIsFlaggedAsAProgrammingError()
    {
        var fault = UsbFault.Classify(6); // ERROR_INVALID_HANDLE

        Assert.Equal(FaultKind.Programming, fault.Kind);
        Assert.Equal(RecoveryAction.Reopen, fault.Recovery);
    }

    [Fact]
    public void UnknownCodesFallIntoTheAmbiguousTierAndNeverCrash()
    {
        var fault = UsbFault.Classify(1234567);

        Assert.Equal(FaultKind.Ambiguous, fault.Kind);
        Assert.Equal(RecoveryAction.AbortFlushReset, fault.Recovery);
        Assert.Contains("1234567", fault.Description);
    }

    [Fact]
    public void EveryClassificationCarriesTheOriginalCode()
    {
        foreach (int code in new[] { 2, 5, 6, 31, 32, 121, 433, 995, 1167, 999999 })
            Assert.Equal(code, UsbFault.Classify(code).Code);
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `dotnet test Mystique.slnx -c Release --filter FullyQualifiedName~UsbFaultTests`
Expected: FAIL — `CS0103: The name 'UsbFault' does not exist in the current context`.

- [ ] **Step 3: Write the implementation**

`Mystique.Core.Device/UsbFault.cs`:

```csharp
namespace Mystique.Core.Device;

public enum FaultKind
{
    /// <summary>Device is not enumerated. Normal at boot; not an error.</summary>
    NotPresent,

    /// <summary>Someone else owns the handle. We step aside by design.</summary>
    Yield,

    /// <summary>Recoverable transfer-level failure.</summary>
    Transient,

    /// <summary>Could be either; treat conservatively.</summary>
    Ambiguous,

    /// <summary>The device physically went away mid-transfer.</summary>
    SurpriseRemoval,

    /// <summary>We cancelled the transfer ourselves.</summary>
    SelfAborted,

    /// <summary>A bug on our side, not the device's.</summary>
    Programming,
}

public enum RecoveryAction
{
    None,
    AbortAndFlush,
    AbortFlushReset,
    Reopen,
    WaitForArrival,
}

/// <summary>
/// Maps a Win32 error code to what the supervisor should do about it.
/// Unknown codes are handled conservatively rather than fatally: nothing in this
/// type ever terminates the service.
/// </summary>
public readonly record struct UsbFault(
    int Code,
    FaultKind Kind,
    RecoveryAction Recovery,
    bool UseFastBackoff,
    string Description)
{
    public const int FileNotFound = 2;
    public const int AccessDenied = 5;
    public const int InvalidHandle = 6;
    public const int GeneralFailure = 31;
    public const int SharingViolation = 32;
    public const int SemTimeout = 121;
    public const int NoSuchDevice = 433;
    public const int OperationAborted = 995;
    public const int DeviceNotConnected = 1167;

    public static UsbFault Classify(int win32Code, bool weInitiatedAbort = false) => win32Code switch
    {
        FileNotFound => new(win32Code, FaultKind.NotPresent, RecoveryAction.WaitForArrival, true,
            "device not enumerated yet"),

        AccessDenied or SharingViolation => new(win32Code, FaultKind.Yield, RecoveryAction.None, true,
            "another process owns the device"),

        SemTimeout => new(win32Code, FaultKind.Transient, RecoveryAction.AbortAndFlush, false,
            "pipe transfer timed out"),

        GeneralFailure => new(win32Code, FaultKind.Ambiguous, RecoveryAction.AbortFlushReset, false,
            "general device failure"),

        NoSuchDevice or DeviceNotConnected => new(
            win32Code, FaultKind.SurpriseRemoval, RecoveryAction.WaitForArrival, true,
            "device removed"),

        OperationAborted when weInitiatedAbort => new(
            win32Code, FaultKind.SelfAborted, RecoveryAction.None, true,
            "transfer cancelled by us"),

        OperationAborted => new(win32Code, FaultKind.SurpriseRemoval, RecoveryAction.WaitForArrival, true,
            "transfer aborted by the stack"),

        InvalidHandle => new(win32Code, FaultKind.Programming, RecoveryAction.Reopen, false,
            "invalid handle - use after free"),

        _ => new(win32Code, FaultKind.Ambiguous, RecoveryAction.AbortFlushReset, false,
            $"unclassified Win32 error {win32Code}"),
    };
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `dotnet test Mystique.slnx -c Release --filter FullyQualifiedName~UsbFaultTests`
Expected: PASS, no failures.

- [ ] **Step 5: Commit**

```bash
git add Mystique.Core.Device/UsbFault.cs Mystique.Tests/UsbFaultTests.cs
git commit -m "feat: classify Win32 USB errors into recovery actions"
```

---

## Task 5: Backoff schedules and failure escalation

Two schedules, not one. A textbook exponential curve applied to the boot path would leave the panel dark for 30 s at every boot — a visible regression against today's behaviour. Log escalation keys off *elapsed failure duration*, not attempt count, so a device unplugged for three days produces roughly 72 lines instead of 8,600.

**Files:**
- Create: `Mystique.Core.Device/BackoffSchedule.cs`, `Mystique.Core.Device/FailureEscalator.cs`
- Test: `Mystique.Tests/BackoffScheduleTests.cs`, `Mystique.Tests/FailureEscalatorTests.cs`, `Mystique.Tests/FakeTimeProvider.cs`

**Interfaces:**
- Consumes: nothing
- Produces:
  - `sealed class BackoffSchedule` with ctor `(TimeSpan[] steps, int maxAttempts, Func<double> jitter)`, `static BackoffSchedule Fast(Func<double>? jitter = null)`, `static BackoffSchedule Slow(Func<double>? jitter = null)`, `TimeSpan NextDelay()`, `void RecordSuccess()`, `void Reset()`, `int ConsecutiveFailures { get; }`, `bool Exhausted { get; }`
  - `const int BackoffSchedule.SuccessesBeforeReset = 30`
  - `enum LogLevel { Debug, Info, Warn, Error }`
  - `sealed class FailureEscalator(TimeProvider time)` with `LogLevel? OnFailure()`, `void OnSuccess()`, `TimeSpan FailingFor { get; }`

- [ ] **Step 1: Write the test double for time**

`Mystique.Tests/FakeTimeProvider.cs`:

```csharp
namespace Mystique.Tests;

/// <summary>
/// Minimal controllable clock. Hand-rolled rather than pulled from a package so
/// the test project stays free of a dependency for three lines of behaviour.
/// </summary>
public sealed class FakeTimeProvider : TimeProvider
{
    private DateTimeOffset now = new(2026, 1, 1, 0, 0, 0, TimeSpan.Zero);

    public override DateTimeOffset GetUtcNow() => now;

    public void Advance(TimeSpan by) => now += by;
}
```

- [ ] **Step 2: Write the failing tests**

`Mystique.Tests/BackoffScheduleTests.cs`:

```csharp
using Mystique.Core.Device;

namespace Mystique.Tests;

public class BackoffScheduleTests
{
    // Jitter of 0.5 is the midpoint, which maps to no adjustment at all.
    private static Func<double> NoJitter => () => 0.5;

    [Fact]
    public void FastScheduleStaysAtOneSecond()
    {
        var schedule = BackoffSchedule.Fast(NoJitter);

        for (int i = 0; i < 10; i++)
            Assert.Equal(TimeSpan.FromSeconds(1), schedule.NextDelay());
    }

    [Fact]
    public void FastScheduleGivesUpAfterThirtyAttempts()
    {
        var schedule = BackoffSchedule.Fast(NoJitter);

        for (int i = 0; i < 30; i++)
        {
            Assert.False(schedule.Exhausted);
            schedule.NextDelay();
        }

        Assert.True(schedule.Exhausted);
    }

    [Fact]
    public void SlowScheduleDoublesThenCapsAtThirtySeconds()
    {
        var schedule = BackoffSchedule.Slow(NoJitter);

        Assert.Equal(TimeSpan.FromSeconds(1), schedule.NextDelay());
        Assert.Equal(TimeSpan.FromSeconds(2), schedule.NextDelay());
        Assert.Equal(TimeSpan.FromSeconds(4), schedule.NextDelay());
        Assert.Equal(TimeSpan.FromSeconds(8), schedule.NextDelay());
        Assert.Equal(TimeSpan.FromSeconds(16), schedule.NextDelay());
        Assert.Equal(TimeSpan.FromSeconds(30), schedule.NextDelay());
        Assert.Equal(TimeSpan.FromSeconds(30), schedule.NextDelay());
        Assert.Equal(TimeSpan.FromSeconds(30), schedule.NextDelay());
    }

    [Fact]
    public void SlowScheduleNeverGivesUp()
    {
        var schedule = BackoffSchedule.Slow(NoJitter);

        for (int i = 0; i < 1000; i++)
            schedule.NextDelay();

        Assert.False(schedule.Exhausted);
    }

    [Theory]
    [InlineData(0.0, 0.8)] // full negative jitter
    [InlineData(0.5, 1.0)] // midpoint
    [InlineData(1.0, 1.2)] // full positive jitter
    public void JitterSpansPlusOrMinusTwentyPercent(double roll, double expectedMultiplier)
    {
        var schedule = BackoffSchedule.Fast(() => roll);

        TimeSpan delay = schedule.NextDelay();

        Assert.Equal(1000 * expectedMultiplier, delay.TotalMilliseconds, precision: 3);
    }

    [Fact]
    public void JitterIsAppliedToCappedDelaysToo()
    {
        var schedule = BackoffSchedule.Slow(() => 0.0);

        for (int i = 0; i < 5; i++)
            schedule.NextDelay();

        Assert.Equal(TimeSpan.FromSeconds(24), schedule.NextDelay()); // 30 s * 0.8
    }

    [Fact]
    public void ASingleSuccessDoesNotResetTheSchedule()
    {
        var schedule = BackoffSchedule.Slow(NoJitter);
        schedule.NextDelay();
        schedule.NextDelay();

        schedule.RecordSuccess();

        // Still advancing, not restarted: a flapping device must not turn
        // exponential backoff into a permanent one-second hammer.
        Assert.Equal(TimeSpan.FromSeconds(4), schedule.NextDelay());
    }

    [Fact]
    public void ThirtyConsecutiveSuccessesResetTheSchedule()
    {
        var schedule = BackoffSchedule.Slow(NoJitter);
        schedule.NextDelay();
        schedule.NextDelay();
        schedule.NextDelay();

        for (int i = 0; i < BackoffSchedule.SuccessesBeforeReset; i++)
            schedule.RecordSuccess();

        Assert.Equal(0, schedule.ConsecutiveFailures);
        Assert.Equal(TimeSpan.FromSeconds(1), schedule.NextDelay());
    }

    [Fact]
    public void AFailureRestartsTheSuccessRunTowardsReset()
    {
        var schedule = BackoffSchedule.Slow(NoJitter);
        schedule.NextDelay();

        for (int i = 0; i < 29; i++)
            schedule.RecordSuccess();
        schedule.NextDelay();                      // one failure wipes the run of 29
        for (int i = 0; i < 29; i++)
            schedule.RecordSuccess();

        Assert.Equal(2, schedule.ConsecutiveFailures);
    }

    [Fact]
    public void ConsecutiveFailuresCountsAttempts()
    {
        var schedule = BackoffSchedule.Slow(NoJitter);

        Assert.Equal(0, schedule.ConsecutiveFailures);
        schedule.NextDelay();
        Assert.Equal(1, schedule.ConsecutiveFailures);
        schedule.NextDelay();
        Assert.Equal(2, schedule.ConsecutiveFailures);
    }

    [Fact]
    public void ResetClearsEverythingImmediately()
    {
        var schedule = BackoffSchedule.Fast(NoJitter);
        for (int i = 0; i < 30; i++)
            schedule.NextDelay();

        schedule.Reset();

        Assert.False(schedule.Exhausted);
        Assert.Equal(0, schedule.ConsecutiveFailures);
    }

    [Fact]
    public void DefaultJitterStaysWithinTheDocumentedBand()
    {
        var schedule = BackoffSchedule.Fast();

        for (int i = 0; i < 200; i++)
        {
            double ms = schedule.NextDelay().TotalMilliseconds;
            Assert.InRange(ms, 800, 1200);
            schedule.Reset();
        }
    }
}
```

`Mystique.Tests/FailureEscalatorTests.cs`:

```csharp
using Mystique.Core.Device;

namespace Mystique.Tests;

public class FailureEscalatorTests
{
    [Fact]
    public void FirstFailureWarns()
    {
        var time = new FakeTimeProvider();
        var escalator = new FailureEscalator(time);

        Assert.Equal(LogLevel.Warn, escalator.OnFailure());
    }

    [Fact]
    public void FurtherFailuresInsideTheFirstMinuteAreSilent()
    {
        var time = new FakeTimeProvider();
        var escalator = new FailureEscalator(time);
        escalator.OnFailure();

        for (int i = 0; i < 50; i++)
        {
            time.Advance(TimeSpan.FromSeconds(1));
            Assert.Null(escalator.OnFailure());
        }
    }

    [Fact]
    public void OneMinuteOfFailureEscalatesToError()
    {
        var time = new FakeTimeProvider();
        var escalator = new FailureEscalator(time);
        escalator.OnFailure();

        time.Advance(TimeSpan.FromMinutes(1));

        Assert.Equal(LogLevel.Error, escalator.OnFailure());
    }

    [Fact]
    public void FiveMinutesOfFailureLogsAgain()
    {
        var time = new FakeTimeProvider();
        var escalator = new FailureEscalator(time);
        escalator.OnFailure();
        time.Advance(TimeSpan.FromMinutes(1));
        escalator.OnFailure();

        time.Advance(TimeSpan.FromMinutes(4));

        Assert.Equal(LogLevel.Error, escalator.OnFailure());
    }

    [Fact]
    public void AfterFiveMinutesItSettlesToHourly()
    {
        var time = new FakeTimeProvider();
        var escalator = new FailureEscalator(time);
        escalator.OnFailure();
        time.Advance(TimeSpan.FromMinutes(5));
        escalator.OnFailure();
        escalator.OnFailure();

        time.Advance(TimeSpan.FromMinutes(59));
        Assert.Null(escalator.OnFailure());

        time.Advance(TimeSpan.FromMinutes(1));
        Assert.Equal(LogLevel.Error, escalator.OnFailure());
    }

    [Fact]
    public void ThreeDaysOfFailureStaysUnderOneHundredLines()
    {
        var time = new FakeTimeProvider();
        var escalator = new FailureEscalator(time);
        int logged = 0;

        // One attempt every 30 s for 72 hours.
        for (int i = 0; i < 72 * 120; i++)
        {
            if (escalator.OnFailure() is not null)
                logged++;
            time.Advance(TimeSpan.FromSeconds(30));
        }

        Assert.InRange(logged, 70, 80);
    }

    [Fact]
    public void SuccessClearsTheEscalationState()
    {
        var time = new FakeTimeProvider();
        var escalator = new FailureEscalator(time);
        escalator.OnFailure();
        time.Advance(TimeSpan.FromMinutes(10));
        escalator.OnFailure();

        escalator.OnSuccess();

        Assert.Equal(TimeSpan.Zero, escalator.FailingFor);
        Assert.Equal(LogLevel.Warn, escalator.OnFailure());
    }

    [Fact]
    public void FailingForReportsElapsedTime()
    {
        var time = new FakeTimeProvider();
        var escalator = new FailureEscalator(time);
        escalator.OnFailure();

        time.Advance(TimeSpan.FromMinutes(3));

        Assert.Equal(TimeSpan.FromMinutes(3), escalator.FailingFor);
    }
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `dotnet test Mystique.slnx -c Release --filter "FullyQualifiedName~BackoffScheduleTests|FullyQualifiedName~FailureEscalatorTests"`
Expected: FAIL — `CS0103: The name 'BackoffSchedule' does not exist in the current context`.

- [ ] **Step 4: Write `BackoffSchedule`**

`Mystique.Core.Device/BackoffSchedule.cs`:

```csharp
namespace Mystique.Core.Device;

/// <summary>
/// Retry pacing. Two factory schedules exist because the two situations are not
/// alike:
///
/// <para><b>Fast</b> is for expected conditions - waiting for the sensor bridge at
/// boot, device arrival after a replug, resume from sleep, reopening after DeepCool
/// exits. These clear in seconds. An exponential curve here would leave the panel
/// dark for 30 s at every boot.</para>
///
/// <para><b>Slow</b> is for genuine faults. The 30 s cap is deliberate and low: a
/// retry costs one CreateFile plus a 2 s pipe connect, so backoff here protects
/// against log spam and pointless wakeups, nothing else. A multi-minute cap would
/// mean the LCD stays dead for minutes after the user closes DeepCool, which reads
/// as broken.</para>
///
/// <para>Jitter is not thundering-herd boilerplate. Two deterministic one-second
/// retry loops competing for an <i>exclusive</i> handle can lock-step and livelock,
/// each grabbing and losing it in alternation.</para>
/// </summary>
public sealed class BackoffSchedule
{
    /// <summary>
    /// Resetting on the first success would turn exponential backoff into a
    /// permanent one-second hammer against a flapping device. Roughly 33 s of
    /// clean operation is required instead.
    /// </summary>
    public const int SuccessesBeforeReset = 30;

    private const double JitterFraction = 0.2;

    private readonly TimeSpan[] steps;
    private readonly int maxAttempts;
    private readonly Func<double> jitter;
    private int attempt;
    private int consecutiveSuccesses;

    /// <param name="steps">Delay per attempt; the last entry is the cap.</param>
    /// <param name="maxAttempts">0 means never exhausted.</param>
    /// <param name="jitter">Returns a value in [0, 1). Injected for deterministic tests.</param>
    public BackoffSchedule(TimeSpan[] steps, int maxAttempts, Func<double> jitter)
    {
        ArgumentNullException.ThrowIfNull(steps);
        if (steps.Length == 0)
            throw new ArgumentException("At least one step is required.", nameof(steps));

        this.steps = steps;
        this.maxAttempts = maxAttempts;
        this.jitter = jitter ?? throw new ArgumentNullException(nameof(jitter));
    }

    public static BackoffSchedule Fast(Func<double>? jitter = null) =>
        new([TimeSpan.FromSeconds(1)], maxAttempts: 30, jitter ?? DefaultJitter);

    public static BackoffSchedule Slow(Func<double>? jitter = null) =>
        new(
            [
                TimeSpan.FromSeconds(1),
                TimeSpan.FromSeconds(2),
                TimeSpan.FromSeconds(4),
                TimeSpan.FromSeconds(8),
                TimeSpan.FromSeconds(16),
                TimeSpan.FromSeconds(30),
            ],
            maxAttempts: 0,
            jitter ?? DefaultJitter);

    public int ConsecutiveFailures => attempt;

    public bool Exhausted => maxAttempts > 0 && attempt >= maxAttempts;

    /// <summary>Records a failed attempt and returns how long to wait before the next one.</summary>
    public TimeSpan NextDelay()
    {
        TimeSpan baseDelay = steps[Math.Min(attempt, steps.Length - 1)];
        attempt++;
        consecutiveSuccesses = 0;

        // jitter roll in [0,1) maps to a multiplier in [0.8, 1.2).
        double multiplier = 1.0 + ((jitter() - 0.5) * 2.0 * JitterFraction);
        return TimeSpan.FromMilliseconds(baseDelay.TotalMilliseconds * multiplier);
    }

    public void RecordSuccess()
    {
        if (attempt == 0)
            return;

        consecutiveSuccesses++;
        if (consecutiveSuccesses >= SuccessesBeforeReset)
            Reset();
    }

    public void Reset()
    {
        attempt = 0;
        consecutiveSuccesses = 0;
    }

    private static double DefaultJitter() => Random.Shared.NextDouble();
}
```

- [ ] **Step 5: Write `FailureEscalator`**

`Mystique.Core.Device/FailureEscalator.cs`:

```csharp
namespace Mystique.Core.Device;

public enum LogLevel
{
    Debug,
    Info,
    Warn,
    Error,
}

/// <summary>
/// Decides whether a failed attempt is worth a log line, based on how long the
/// failure has been going on rather than how many attempts have been made.
/// A device unplugged for three days yields roughly 72 lines instead of 8,600.
/// </summary>
public sealed class FailureEscalator(TimeProvider time)
{
    private static readonly TimeSpan[] Thresholds =
    [
        TimeSpan.Zero,
        TimeSpan.FromMinutes(1),
        TimeSpan.FromMinutes(5),
    ];

    private static readonly TimeSpan SteadyStateInterval = TimeSpan.FromHours(1);

    private readonly TimeProvider time = time;
    private DateTimeOffset? failingSince;
    private DateTimeOffset lastLogged;
    private int thresholdsCrossed;

    public TimeSpan FailingFor =>
        failingSince is { } since ? time.GetUtcNow() - since : TimeSpan.Zero;

    /// <summary>Returns the level to log at, or null if this failure should stay silent.</summary>
    public LogLevel? OnFailure()
    {
        DateTimeOffset now = time.GetUtcNow();

        if (failingSince is null)
        {
            failingSince = now;
            lastLogged = now;
            thresholdsCrossed = 1;
            return LogLevel.Warn;
        }

        TimeSpan elapsed = now - failingSince.Value;

        if (thresholdsCrossed < Thresholds.Length && elapsed >= Thresholds[thresholdsCrossed])
        {
            thresholdsCrossed++;
            lastLogged = now;
            return LogLevel.Error;
        }

        if (thresholdsCrossed >= Thresholds.Length && now - lastLogged >= SteadyStateInterval)
        {
            lastLogged = now;
            return LogLevel.Error;
        }

        return null;
    }

    public void OnSuccess()
    {
        failingSince = null;
        thresholdsCrossed = 0;
    }
}
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `dotnet test Mystique.slnx -c Release --filter "FullyQualifiedName~BackoffScheduleTests|FullyQualifiedName~FailureEscalatorTests"`
Expected: PASS, no failures.

- [ ] **Step 7: Commit**

```bash
git add Mystique.Core.Device/BackoffSchedule.cs Mystique.Core.Device/FailureEscalator.cs Mystique.Tests
git commit -m "feat: add dual backoff schedules and duration-based log escalation"
```

---

## Task 6: Interop and honest device presence detection

The current `FindDevicePath` (`MystiqueUsbTool/Program.cs:94-113`) reads persisted registry state. On this machine 22 of 35 `SymbolicName` entries belong to **absent** devices, so it happily returns a path for hardware that is not plugged in. At boot that produces a confusing failure: the registry read succeeds, then `CreateFile` fails with error 2, which reads like "wrong path" but actually means "not enumerated yet".

`CM_Get_Device_Interface_List` with `CM_GET_DEVICE_INTERFACE_LIST_PRESENT` makes presence and path a single atomic, honest query.

Verified on this machine: `DeviceInterfaceGUIDs` under `Device Parameters` is **empty**, so a locator keyed on that registry value would find nothing. The interface GUID that actually appears in the device's `SymbolicName` is `{a5dcbf10-6530-11d2-901f-00c04fb951ed}` — `GUID_DEVINTERFACE_USB_DEVICE`, the generic USB device interface class. That is the GUID to enumerate.

**Files:**
- Create: `Mystique.Core.Device/UsbStructs.cs`, `Mystique.Core.Device/Native.cs`, `Mystique.Core.Device/DeviceLocator.cs`
- Test: `Mystique.Tests/DeviceLocatorTests.cs`
- Reference: P/Invokes to move from `MystiqueUsbTool/Program.cs:484-573`

**Interfaces:**
- Consumes: nothing
- Produces:
  - `static class Native` — all P/Invokes, `internal` to the assembly
  - `static readonly Guid DeviceLocator.UsbDeviceInterfaceClass`
  - `const string DeviceLocator.HardwareIdFragment = "vid_3633&pid_0009"`
  - `static IReadOnlyList<string> DeviceLocator.ParseInterfaceList(ReadOnlySpan<char> multiSz)`
  - `static bool DeviceLocator.IsMystiqueInterface(string interfacePath)`
  - `static string? DeviceLocator.FindPresentDevicePath()` — null means not present
  - `static string DeviceLocator.RedactSerial(string interfacePath)`
  - `struct UsbInterfaceDescriptor`, `struct WinUsbPipeInformation`, `enum UsbdPipeType`

- [ ] **Step 1: Write the failing tests**

`Mystique.Tests/DeviceLocatorTests.cs`:

```csharp
using Mystique.Core.Device;

namespace Mystique.Tests;

public class DeviceLocatorTests
{
    private const string RealPath =
        @"\\?\USB#VID_3633&PID_0009#0011223344556677889900AA#{a5dcbf10-6530-11d2-901f-00c04fb951ed}";

    private static char[] MultiSz(params string[] entries)
    {
        var chars = new List<char>();
        foreach (string entry in entries)
        {
            chars.AddRange(entry);
            chars.Add('\0');
        }

        chars.Add('\0'); // list terminator
        return chars.ToArray();
    }

    [Fact]
    public void ParsesADoubleNullTerminatedList()
    {
        var entries = DeviceLocator.ParseInterfaceList(MultiSz("alpha", "beta", "gamma"));

        Assert.Equal(new[] { "alpha", "beta", "gamma" }, entries);
    }

    [Fact]
    public void ParsesAnEmptyList()
    {
        Assert.Empty(DeviceLocator.ParseInterfaceList(new[] { '\0' }));
        Assert.Empty(DeviceLocator.ParseInterfaceList(ReadOnlySpan<char>.Empty));
    }

    [Fact]
    public void ParsesASingleEntry()
    {
        Assert.Equal(new[] { RealPath }, DeviceLocator.ParseInterfaceList(MultiSz(RealPath)));
    }

    [Fact]
    public void RecognisesTheMystiqueHardwareId()
    {
        Assert.True(DeviceLocator.IsMystiqueInterface(RealPath));
    }

    [Fact]
    public void MatchingIsCaseInsensitiveBecauseWindowsVariesTheCasing()
    {
        Assert.True(DeviceLocator.IsMystiqueInterface(RealPath.ToUpperInvariant()));
        Assert.True(DeviceLocator.IsMystiqueInterface(RealPath.ToLowerInvariant()));
    }

    [Theory]
    [InlineData(@"\\?\USB#VID_1038&PID_1122#abc#{a5dcbf10-6530-11d2-901f-00c04fb951ed}")]
    [InlineData(@"\\?\USB#VID_3633&PID_0008#abc#{a5dcbf10-6530-11d2-901f-00c04fb951ed}")]
    [InlineData("")]
    public void RejectsOtherDevices(string path)
    {
        Assert.False(DeviceLocator.IsMystiqueInterface(path));
    }

    [Fact]
    public void RedactionRemovesTheSerialButKeepsTheShape()
    {
        string redacted = DeviceLocator.RedactSerial(RealPath);

        Assert.DoesNotContain("0011223344556677889900AA", redacted);
        Assert.Contains("VID_3633&PID_0009", redacted);
        Assert.Contains("<serial>", redacted);
    }

    [Fact]
    public void RedactionLeavesUnexpectedShapesAlone()
    {
        Assert.Equal("not-a-device-path", DeviceLocator.RedactSerial("not-a-device-path"));
    }

    [Fact]
    [Trait("Category", "Hardware")]
    public void FindsThePresentDeviceOnThisMachine()
    {
        string? path = DeviceLocator.FindPresentDevicePath();

        Assert.NotNull(path);
        Assert.Contains(DeviceLocator.HardwareIdFragment, path, StringComparison.OrdinalIgnoreCase);
        Assert.StartsWith(@"\\?\", path, StringComparison.Ordinal);
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `dotnet test Mystique.slnx -c Release --filter FullyQualifiedName~DeviceLocatorTests`
Expected: FAIL — `CS0103: The name 'DeviceLocator' does not exist in the current context`.

- [ ] **Step 3: Write the interop structs**

`Mystique.Core.Device/UsbStructs.cs`:

```csharp
using System.Runtime.InteropServices;

namespace Mystique.Core.Device;

public enum UsbdPipeType
{
    Control,
    Isochronous,
    Bulk,
    Interrupt,
}

[StructLayout(LayoutKind.Sequential)]
public struct UsbInterfaceDescriptor
{
    public byte Length;
    public byte DescriptorType;
    public byte InterfaceNumber;
    public byte AlternateSetting;
    public byte NumEndpoints;
    public byte InterfaceClass;
    public byte InterfaceSubClass;
    public byte InterfaceProtocol;
    public byte Interface;
}

[StructLayout(LayoutKind.Sequential)]
public struct WinUsbPipeInformation
{
    public UsbdPipeType PipeType;
    public byte PipeId;
    public ushort MaximumPacketSize;
    public byte Interval;
}
```

- [ ] **Step 4: Write the P/Invokes**

`Mystique.Core.Device/Native.cs`:

```csharp
using System.Runtime.InteropServices;
using Microsoft.Win32.SafeHandles;

namespace Mystique.Core.Device;

internal static class Native
{
    internal const uint GenericRead = 0x80000000;
    internal const uint GenericWrite = 0x40000000;
    internal const uint FileShareRead = 0x00000001;
    internal const uint FileShareWrite = 0x00000002;
    internal const uint OpenExisting = 3;
    internal const uint FileAttributeNormal = 0x00000080;
    internal const uint FileFlagOverlapped = 0x40000000;

    // WINUSB_PIPE_POLICY identifiers.
    internal const uint PipeTransferTimeout = 0x03;
    internal const uint ResetPipeOnResume = 0x09;

    // CONFIGRET values.
    internal const int CrSuccess = 0x00;
    internal const int CrBufferSmall = 0x1A;

    // CM_GET_DEVICE_INTERFACE_LIST_PRESENT is 0; ALL_DEVICES is 1. Present-only is
    // the entire point of using this API over the registry.
    internal const uint GetInterfaceListPresent = 0x00000000;

    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    internal static extern SafeFileHandle CreateFile(
        string fileName,
        uint desiredAccess,
        uint shareMode,
        IntPtr securityAttributes,
        uint creationDisposition,
        uint flagsAndAttributes,
        IntPtr templateFile);

    [DllImport("cfgmgr32.dll", CharSet = CharSet.Unicode, EntryPoint = "CM_Get_Device_Interface_List_SizeW")]
    internal static extern int CM_Get_Device_Interface_List_Size(
        out uint length, ref Guid interfaceClassGuid, IntPtr deviceId, uint flags);

    [DllImport("cfgmgr32.dll", CharSet = CharSet.Unicode, EntryPoint = "CM_Get_Device_Interface_ListW")]
    internal static extern int CM_Get_Device_Interface_List(
        ref Guid interfaceClassGuid, IntPtr deviceId, char[] buffer, uint bufferLength, uint flags);

    [DllImport("winusb.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static extern bool WinUsb_Initialize(SafeFileHandle deviceHandle, out IntPtr interfaceHandle);

    [DllImport("winusb.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static extern bool WinUsb_Free(IntPtr interfaceHandle);

    [DllImport("winusb.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static extern bool WinUsb_QueryInterfaceSettings(
        IntPtr interfaceHandle, byte alternateInterfaceNumber,
        out UsbInterfaceDescriptor usbAltInterfaceDescriptor);

    [DllImport("winusb.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static extern bool WinUsb_QueryPipe(
        IntPtr interfaceHandle, byte alternateInterfaceNumber, byte pipeIndex,
        out WinUsbPipeInformation pipeInformation);

    /// <summary>ULONG-valued policies, e.g. PIPE_TRANSFER_TIMEOUT.</summary>
    [DllImport("winusb.dll", SetLastError = true, EntryPoint = "WinUsb_SetPipePolicy")]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static extern bool WinUsb_SetPipePolicyUInt32(
        IntPtr interfaceHandle, byte pipeId, uint policyType, uint valueLength, ref uint value);

    /// <summary>BOOL-valued policies, e.g. RESET_PIPE_ON_RESUME. Value length is 1, not 4.</summary>
    [DllImport("winusb.dll", SetLastError = true, EntryPoint = "WinUsb_SetPipePolicy")]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static extern bool WinUsb_SetPipePolicyByte(
        IntPtr interfaceHandle, byte pipeId, uint policyType, uint valueLength, ref byte value);

    [DllImport("winusb.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static extern bool WinUsb_WritePipe(
        IntPtr interfaceHandle, byte pipeId, byte[] buffer, uint bufferLength,
        out uint lengthTransferred, IntPtr overlapped);

    [DllImport("winusb.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static extern bool WinUsb_ReadPipe(
        IntPtr interfaceHandle, byte pipeId, byte[] buffer, uint bufferLength,
        out uint lengthTransferred, IntPtr overlapped);

    [DllImport("winusb.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static extern bool WinUsb_AbortPipe(IntPtr interfaceHandle, byte pipeId);

    [DllImport("winusb.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static extern bool WinUsb_FlushPipe(IntPtr interfaceHandle, byte pipeId);

    [DllImport("winusb.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static extern bool WinUsb_ResetPipe(IntPtr interfaceHandle, byte pipeId);
}
```

- [ ] **Step 5: Write the locator**

`Mystique.Core.Device/DeviceLocator.cs`:

```csharp
using System.ComponentModel;

namespace Mystique.Core.Device;

/// <summary>
/// Finds the Mystique's WinUSB interface path, and answers "is it plugged in?"
/// with the same call.
///
/// <para>This deliberately does not read
/// <c>HKLM\SYSTEM\CurrentControlSet\Enum\...\Device Parameters\SymbolicName</c>.
/// That key persists across unplugs, so it returns paths for absent hardware;
/// on the development machine 22 of 35 such entries were stale.
/// <c>CM_GET_DEVICE_INTERFACE_LIST_PRESENT</c> returns present devices only.</para>
/// </summary>
public static class DeviceLocator
{
    /// <summary>
    /// GUID_DEVINTERFACE_USB_DEVICE. Confirmed to be the GUID in this device's
    /// SymbolicName; its DeviceInterfaceGUIDs value is empty, so it must not be
    /// used as the lookup key.
    /// </summary>
    public static readonly Guid UsbDeviceInterfaceClass =
        new("a5dcbf10-6530-11d2-901f-00c04fb951ed");

    public const string HardwareIdFragment = "vid_3633&pid_0009";

    private const int MaxSizeRetries = 4;

    /// <summary>Returns the interface path, or null if the device is not present.</summary>
    public static string? FindPresentDevicePath()
    {
        foreach (string path in EnumeratePresentInterfaces())
        {
            if (IsMystiqueInterface(path))
                return path;
        }

        return null;
    }

    public static bool IsMystiqueInterface(string interfacePath) =>
        interfacePath.Contains(HardwareIdFragment, StringComparison.OrdinalIgnoreCase);

    /// <summary>Splits a Windows REG_MULTI_SZ-style double-null-terminated buffer.</summary>
    public static IReadOnlyList<string> ParseInterfaceList(ReadOnlySpan<char> multiSz)
    {
        var entries = new List<string>();
        int start = 0;

        for (int i = 0; i < multiSz.Length; i++)
        {
            if (multiSz[i] != '\0')
                continue;

            if (i == start)
                break; // empty entry marks the end of the list

            entries.Add(multiSz[start..i].ToString());
            start = i + 1;
        }

        return entries;
    }

    /// <summary>Masks the per-device serial in an interface path for logging.</summary>
    public static string RedactSerial(string interfacePath)
    {
        string[] parts = interfacePath.Split('#');
        return parts.Length >= 4
            ? string.Join('#', parts[0], parts[1], "<serial>", parts[3])
            : interfacePath;
    }

    private static IReadOnlyList<string> EnumeratePresentInterfaces()
    {
        Guid guid = UsbDeviceInterfaceClass;

        // The device set can change between sizing and fetching, so CM can return
        // CR_BUFFER_SMALL on the second call. Retry a bounded number of times.
        for (int attempt = 0; attempt < MaxSizeRetries; attempt++)
        {
            int sizeResult = Native.CM_Get_Device_Interface_List_Size(
                out uint length, ref guid, IntPtr.Zero, Native.GetInterfaceListPresent);
            if (sizeResult != Native.CrSuccess)
                throw new InvalidOperationException(
                    $"CM_Get_Device_Interface_List_Size failed with CONFIGRET 0x{sizeResult:X2}.");

            if (length <= 1)
                return [];

            char[] buffer = new char[length];
            int listResult = Native.CM_Get_Device_Interface_List(
                ref guid, IntPtr.Zero, buffer, length, Native.GetInterfaceListPresent);

            if (listResult == Native.CrSuccess)
                return ParseInterfaceList(buffer);

            if (listResult != Native.CrBufferSmall)
                throw new InvalidOperationException(
                    $"CM_Get_Device_Interface_List failed with CONFIGRET 0x{listResult:X2}.");
        }

        throw new InvalidOperationException(
            $"Device interface list kept growing across {MaxSizeRetries} attempts.");
    }
}
```

The unused `System.ComponentModel` using will trip `TreatWarningsAsErrors`; remove it if the compiler flags it.

- [ ] **Step 6: Run the tests to verify they pass**

Run: `dotnet test Mystique.slnx -c Release --filter FullyQualifiedName~DeviceLocatorTests`
Expected: PASS, no failures, including the hardware test (the device is present and reports Status `OK`).

If `FindsThePresentDeviceOnThisMachine` fails, the device is genuinely unplugged or the GUID is wrong. Confirm with:

```powershell
Get-PnpDevice | Where-Object InstanceId -like 'USB\VID_3633&PID_0009*' | Select-Object Status,InstanceId
```

- [ ] **Step 7: Commit**

```bash
git add Mystique.Core.Device/UsbStructs.cs Mystique.Core.Device/Native.cs Mystique.Core.Device/DeviceLocator.cs Mystique.Tests/DeviceLocatorTests.cs
git commit -m "feat: detect device presence via CM_Get_Device_Interface_List instead of stale registry state"
```

---

## Task 7: MystiqueDevice — handle lifetime and exchanges

Three traps make a naive version of this **worse** than today's crash-on-error behaviour. All three are addressed by design here, not by care:

1. **`PIPE_TRANSFER_TIMEOUT` defaults to 0 = infinite for bulk endpoints, and pipe policy is state on the handle.** A reconnect that reopens the device but forgets to re-apply the timeout blocks the worker thread *forever* — no exception, no log, status green, LCD frozen. Mitigation: policy application lives inside `Open`, so it cannot be separated from `CreateFile` + `WinUsb_Initialize`. There is no public `SetTimeout`.
2. **After any reopen the device is no longer in Machine Info mode.** Writes are ACKed and display nothing. Mitigation: `EnterMachineInfoMode` re-verifies the `0xFF` status byte and throws if absent; the supervisor may not declare `Streaming` without it.
3. **Today's per-exchange `SetTimeout` is an accidental liveness probe** that throws on a dead device *before* the write, which is why current errors are clean. Moving policy to open-time removes it. Mitigation: every write and read maps its Win32 code through `UsbFault.Classify` and throws `MystiqueUsbException`, so diagnosability does not silently regress.

**Files:**
- Create: `Mystique.Core.Device/IMystiqueDevice.cs`, `Mystique.Core.Device/MystiqueUsbException.cs`, `Mystique.Core.Device/MystiqueDevice.cs`
- Test: `Mystique.Tests/MystiqueDeviceHardwareTests.cs`
- Reference: current exchange at `MystiqueUsbTool/Program.cs:261-298`, open at `:28-42`, init at `:135-166`

**Interfaces:**
- Consumes: `MystiquePackets`, `UsbFault`, `DeviceLocator`, `Native`, `TelemetryFrame`
- Produces:
  - `interface IMystiqueDevice : IDisposable` with `string RedactedPath { get; }`, `IReadOnlyList<InitStep> EnterMachineInfoMode(byte rotation)`, `DeviceStatus ReadStatus()`, `byte[] SendTelemetry(in TelemetryFrame frame)`, `void Recover(RecoveryAction action)`, `void AbortPendingTransfers()`
  - `readonly record struct InitStep(byte Command, byte[] Response)`
  - `sealed class MystiqueUsbException : Exception` with `UsbFault Fault { get; }`
  - `sealed class MystiqueDevice : IMystiqueDevice` with `static MystiqueDevice Open()`
  - `readonly record struct PipeInfo(byte PipeId, UsbdPipeType Type, ushort MaximumPacketSize)`

Note: Task 9 adds one more member to `IMystiqueDevice` — `byte[] SendInfoRequest()`. Any fake implementing this interface must include it.

- [ ] **Step 1: Write the failing hardware test**

`Mystique.Tests/MystiqueDeviceHardwareTests.cs`:

```csharp
using Mystique.Core.Contracts;
using Mystique.Core.Device;

namespace Mystique.Tests;

/// <summary>
/// These talk to the real panel. They require the device plugged in and no other
/// process holding it (close DeepCool.exe and stop MystiqueService first).
/// Filter them out with: --filter "Category!=Hardware"
/// </summary>
[Trait("Category", "Hardware")]
public class MystiqueDeviceHardwareTests
{
    [Fact]
    public void OpenExposesTheFourExpectedBulkEndpoints()
    {
        using var device = MystiqueDevice.Open();

        var pipes = device.DescribePipes();

        Assert.Equal(4, pipes.Count);
        Assert.All(pipes, p => Assert.Equal(UsbdPipeType.Bulk, p.Type));
        Assert.All(pipes, p => Assert.Equal(64, p.MaximumPacketSize));
        Assert.Equal(
            new byte[] { 0x01, 0x81, 0x02, 0x82 },
            pipes.Select(p => p.PipeId).Order().ToArray());
    }

    [Fact]
    public void OpenNeverLeaksTheSerialInTheReportedPath()
    {
        using var device = MystiqueDevice.Open();

        Assert.Contains("<serial>", device.RedactedPath);
        Assert.DoesNotContain("0011223344556677889900AA", device.RedactedPath);
    }

    [Fact]
    public void MachineInfoModeCompletesAndIsConfirmedByTheDevice()
    {
        using var device = MystiqueDevice.Open();

        var steps = device.EnterMachineInfoMode(rotation: 0);

        Assert.Equal(13, steps.Count);
        Assert.All(steps, s => Assert.True(s.Response.Length >= 3));
        Assert.Equal(DisplayMode.MachineInfo, device.ReadStatus().Mode);
    }

    [Fact]
    public void TelemetryFramesAreAcknowledged()
    {
        using var device = MystiqueDevice.Open();
        device.EnterMachineInfoMode(rotation: 0);

        for (int i = 0; i < 5; i++)
        {
            byte[] response = device.SendTelemetry(
                new TelemetryFrame((byte)(40 + i), (byte)(10 + i), 42, 45, 4500));

            MystiquePackets.ValidateResponse(response, MystiquePackets.CommandTelemetry);
        }
    }

    [Fact]
    public void RotationIsReportedBackByTheDevice()
    {
        using var device = MystiqueDevice.Open();

        device.EnterMachineInfoMode(rotation: 2);

        Assert.Equal(2, device.ReadStatus().Rotation);

        device.EnterMachineInfoMode(rotation: 0); // restore
        Assert.Equal(0, device.ReadStatus().Rotation);
    }

    [Fact]
    public void UsingADisposedDeviceThrowsRatherThanTouchingAFreedHandle()
    {
        var device = MystiqueDevice.Open();
        device.Dispose();

        Assert.Throws<ObjectDisposedException>(() => device.ReadStatus());
    }

    [Fact]
    public void DisposeIsIdempotent()
    {
        var device = MystiqueDevice.Open();
        device.Dispose();
        device.Dispose();
    }

    [Fact]
    public void ASecondOpenSucceedsBecauseSharingIsPermissive()
    {
        // FILE_SHARE_READ | FILE_SHARE_WRITE is an invariant: getting there first
        // with a permissive share mode is what makes DeepCool takeover work in our
        // favour rather than producing a reset war.
        using var first = MystiqueDevice.Open();
        using var second = MystiqueDevice.Open();

        Assert.NotNull(second.RedactedPath);
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `dotnet test Mystique.slnx -c Release --filter FullyQualifiedName~MystiqueDeviceHardwareTests`
Expected: FAIL — `CS0103: The name 'MystiqueDevice' does not exist in the current context`.

- [ ] **Step 3: Write the exception type**

`Mystique.Core.Device/MystiqueUsbException.cs`:

```csharp
namespace Mystique.Core.Device;

/// <summary>
/// A USB operation failed, carrying the classification the supervisor needs.
/// Every WinUSB failure path in this assembly raises this rather than a bare
/// Win32Exception, so no caller has to re-derive the handling decision.
/// </summary>
public sealed class MystiqueUsbException : Exception
{
    public MystiqueUsbException(UsbFault fault, string operation)
        : base($"{operation} failed: {fault.Description} (Win32 {fault.Code})")
    {
        Fault = fault;
        Operation = operation;
    }

    public UsbFault Fault { get; }

    public string Operation { get; }
}
```

- [ ] **Step 4: Write the device**

`Mystique.Core.Device/IMystiqueDevice.cs`:

```csharp
using Mystique.Core.Contracts;

namespace Mystique.Core.Device;

/// <summary>One command of the init handshake and the device's reply to it.</summary>
public readonly record struct InitStep(byte Command, byte[] Response);

public readonly record struct PipeInfo(byte PipeId, UsbdPipeType Type, ushort MaximumPacketSize);

/// <summary>
/// The device surface the supervisor depends on. An interface so the state
/// machine can be tested against a fake without hardware.
/// </summary>
public interface IMystiqueDevice : IDisposable
{
    /// <summary>Interface path with the per-device serial masked. Safe to log.</summary>
    string RedactedPath { get; }

    IReadOnlyList<PipeInfo> DescribePipes();

    /// <summary>
    /// Runs the 13-command handshake and verifies the device reports Machine Info
    /// mode. Must be re-run after every reopen: a reopened device is not in
    /// Machine Info mode, and telemetry writes would be ACKed while displaying nothing.
    /// </summary>
    IReadOnlyList<InitStep> EnterMachineInfoMode(byte rotation);

    DeviceStatus ReadStatus();

    byte[] SendTelemetry(in TelemetryFrame frame);

    void Recover(RecoveryAction action);

    /// <summary>Cancels in-flight transfers so a blocked worker can unwind promptly.</summary>
    void AbortPendingTransfers();
}
```

`Mystique.Core.Device/MystiqueDevice.cs`:

```csharp
using System.Runtime.InteropServices;
using Microsoft.Win32.SafeHandles;
using Mystique.Core.Contracts;

namespace Mystique.Core.Device;

/// <summary>
/// Owns one WinUSB handle for its lifetime. Not thread-safe by design: exactly one
/// worker thread performs exchanges. <see cref="AbortPendingTransfers"/> is the one
/// member callable from another thread.
/// </summary>
public sealed class MystiqueDevice : IMystiqueDevice
{
    /// <summary>
    /// Applied to all four endpoints at open time. Bulk pipes default to 0, which
    /// means <i>infinite</i>: forgetting this on a reopen hangs the worker forever
    /// with no exception and no log.
    /// </summary>
    private const uint TransferTimeoutMilliseconds = 1500;

    private static readonly byte[] AllPipes =
    [
        MystiquePackets.InitPipeOut,
        MystiquePackets.InitPipeIn,
        MystiquePackets.DataPipeOut,
        MystiquePackets.DataPipeIn,
    ];

    private readonly SafeFileHandle file;
    private readonly PipeInfo[] pipes;
    private IntPtr usb;
    private volatile bool abortRequested;

    private MystiqueDevice(SafeFileHandle file, IntPtr usb, string redactedPath, PipeInfo[] pipes)
    {
        this.file = file;
        this.usb = usb;
        this.pipes = pipes;
        RedactedPath = redactedPath;
    }

    public string RedactedPath { get; }

    /// <summary>
    /// Locates, opens, initializes, and applies pipe policy as one indivisible step.
    /// There is deliberately no way to obtain a device without its policy applied.
    /// </summary>
    public static MystiqueDevice Open()
    {
        string? path = DeviceLocator.FindPresentDevicePath()
            ?? throw new MystiqueUsbException(
                UsbFault.Classify(UsbFault.FileNotFound), "locate device");

        SafeFileHandle file = Native.CreateFile(
            path,
            Native.GenericRead | Native.GenericWrite,
            // Permissive sharing is an invariant, not an oversight. See design spec.
            Native.FileShareRead | Native.FileShareWrite,
            IntPtr.Zero,
            Native.OpenExisting,
            Native.FileAttributeNormal | Native.FileFlagOverlapped,
            IntPtr.Zero);

        if (file.IsInvalid)
        {
            int code = Marshal.GetLastWin32Error();
            file.Dispose();
            throw new MystiqueUsbException(UsbFault.Classify(code), "CreateFile");
        }

        if (!Native.WinUsb_Initialize(file, out IntPtr usb))
        {
            int code = Marshal.GetLastWin32Error();
            file.Dispose();
            throw new MystiqueUsbException(UsbFault.Classify(code), "WinUsb_Initialize");
        }

        try
        {
            PipeInfo[] discovered = QueryPipes(usb);
            ApplyPipePolicy(usb);
            return new MystiqueDevice(file, usb, DeviceLocator.RedactSerial(path), discovered);
        }
        catch
        {
            Native.WinUsb_Free(usb);
            file.Dispose();
            throw;
        }
    }

    public IReadOnlyList<PipeInfo> DescribePipes() => pipes;

    public IReadOnlyList<InitStep> EnterMachineInfoMode(byte rotation)
    {
        var steps = new List<InitStep>();

        foreach (InitCommand command in MystiquePackets.BuildInitSequence(rotation))
        {
            byte[] response = Exchange(
                MystiquePackets.InitPipeOut, MystiquePackets.InitPipeIn,
                command.Command, command.Payload);
            steps.Add(new InitStep(command.Command, response));
            Thread.Sleep(command.SettleMilliseconds);
        }

        DeviceStatus status = ReadStatus();
        if (status.Mode != DisplayMode.MachineInfo)
            throw new InvalidOperationException(
                $"Device did not enter Machine Info mode (mode byte 0x{status.RawMode:X2}).");

        return steps;
    }

    public DeviceStatus ReadStatus()
    {
        byte[] response = Exchange(
            MystiquePackets.DataPipeOut, MystiquePackets.DataPipeIn,
            MystiquePackets.CommandStatus, []);
        return MystiquePackets.ParseStatus(response);
    }

    public byte[] SendTelemetry(in TelemetryFrame frame) =>
        Exchange(
            MystiquePackets.DataPipeOut, MystiquePackets.DataPipeIn,
            MystiquePackets.CommandTelemetry, MystiquePackets.BuildDisplayPayload(frame));

    public void Recover(RecoveryAction action)
    {
        if (action is RecoveryAction.None or RecoveryAction.Reopen or RecoveryAction.WaitForArrival)
            return;

        IntPtr handle = usb;
        if (handle == IntPtr.Zero)
            return;

        foreach (byte pipe in AllPipes)
        {
            Native.WinUsb_AbortPipe(handle, pipe);
            Native.WinUsb_FlushPipe(handle, pipe);
            if (action == RecoveryAction.AbortFlushReset)
                Native.WinUsb_ResetPipe(handle, pipe);
        }
    }

    public void AbortPendingTransfers()
    {
        IntPtr handle = usb;
        if (handle == IntPtr.Zero)
            return;

        abortRequested = true;
        foreach (byte pipe in AllPipes)
            Native.WinUsb_AbortPipe(handle, pipe);
    }

    public void Dispose()
    {
        IntPtr handle = Interlocked.Exchange(ref usb, IntPtr.Zero);
        if (handle != IntPtr.Zero)
            Native.WinUsb_Free(handle);

        file.Dispose();
    }

    private byte[] Exchange(byte outPipe, byte inPipe, byte command, ReadOnlySpan<byte> payload)
    {
        IntPtr handle = usb;
        ObjectDisposedException.ThrowIf(handle == IntPtr.Zero, this);

        byte[] packet = MystiquePackets.BuildPacket(command, payload);

        if (!Native.WinUsb_WritePipe(handle, outPipe, packet, (uint)packet.Length,
                out uint written, IntPtr.Zero))
            throw Fail($"write 0x{outPipe:X2} command 0x{command:X2}");

        if (written != packet.Length)
            throw new IOException(
                $"Short write on 0x{outPipe:X2}: {written}/{packet.Length} bytes");

        byte[] response = new byte[64];
        if (!Native.WinUsb_ReadPipe(handle, inPipe, response, (uint)response.Length,
                out uint read, IntPtr.Zero))
            throw Fail($"read 0x{inPipe:X2} command 0x{command:X2}");

        Array.Resize(ref response, checked((int)read));
        MystiquePackets.ValidateResponse(response, command);
        return response;
    }

    /// <summary>
    /// Builds the classified exception. Because pipe policy no longer runs per
    /// exchange, this is the only place a dead device is detected, so it must
    /// classify rather than rethrow blindly.
    /// </summary>
    private MystiqueUsbException Fail(string operation)
    {
        int code = Marshal.GetLastWin32Error();
        return new MystiqueUsbException(UsbFault.Classify(code, abortRequested), operation);
    }

    private static PipeInfo[] QueryPipes(IntPtr usb)
    {
        if (!Native.WinUsb_QueryInterfaceSettings(usb, 0, out UsbInterfaceDescriptor descriptor))
            throw new MystiqueUsbException(
                UsbFault.Classify(Marshal.GetLastWin32Error()), "WinUsb_QueryInterfaceSettings");

        var discovered = new PipeInfo[descriptor.NumEndpoints];
        for (byte index = 0; index < descriptor.NumEndpoints; index++)
        {
            if (!Native.WinUsb_QueryPipe(usb, 0, index, out WinUsbPipeInformation pipe))
                throw new MystiqueUsbException(
                    UsbFault.Classify(Marshal.GetLastWin32Error()), $"WinUsb_QueryPipe({index})");

            discovered[index] = new PipeInfo(pipe.PipeId, pipe.PipeType, pipe.MaximumPacketSize);
        }

        return discovered;
    }

    private static void ApplyPipePolicy(IntPtr usb)
    {
        foreach (byte pipe in AllPipes)
        {
            uint timeout = TransferTimeoutMilliseconds;
            if (!Native.WinUsb_SetPipePolicyUInt32(
                    usb, pipe, Native.PipeTransferTimeout, sizeof(uint), ref timeout))
                throw new MystiqueUsbException(
                    UsbFault.Classify(Marshal.GetLastWin32Error()),
                    $"set PIPE_TRANSFER_TIMEOUT on 0x{pipe:X2}");

            // Defaults to FALSE. Exists precisely for devices that do not preserve
            // data-toggle state across suspend, which is the sleep/resume case.
            byte resetOnResume = 1;
            if (!Native.WinUsb_SetPipePolicyByte(
                    usb, pipe, Native.ResetPipeOnResume, sizeof(byte), ref resetOnResume))
                throw new MystiqueUsbException(
                    UsbFault.Classify(Marshal.GetLastWin32Error()),
                    $"set RESET_PIPE_ON_RESUME on 0x{pipe:X2}");
        }
    }
}
```

- [ ] **Step 5: Run the hardware tests to verify they pass**

Ensure nothing else holds the device first:

```powershell
Get-Process DeepCool,MystiqueUsbTool -ErrorAction SilentlyContinue
```

Run: `dotnet test Mystique.slnx -c Release --filter FullyQualifiedName~MystiqueDeviceHardwareTests`
Expected: PASS, no failures. The LCD will be left in Machine Info mode showing the last test frame; that is correct and expected.

- [ ] **Step 6: Confirm the non-hardware suite still runs clean without the device**

Run: `dotnet test Mystique.slnx -c Release --filter "Category!=Hardware"`
Expected: PASS, and no test in this run touches USB.

- [ ] **Step 7: Commit**

```bash
git add Mystique.Core.Device Mystique.Tests
git commit -m "feat: add MystiqueDevice with open-time pipe policy and classified faults"
```

---

## Task 8: Sensor bridge client

Moves the DeepCool sensor-bridge client into Core behind an interface, and fixes a real security defect while doing so.

**The defect:** `MystiqueUsbTool/Program.cs:372-373` and `:399-400` use the simple `NamedPipeClientStream` constructor. That leaves `TokenImpersonationLevel` at `None`, which leaves `SECURITY_SQOS_PRESENT` unset, which makes NT apply its default of `SecurityImpersonation`. A process that squats the pipe name before the real server starts could then impersonate the connecting token. Passing `TokenImpersonationLevel.Anonymous` closes it. Do not carry the current form forward.

**Files:**
- Create: `Mystique.Core.Device/SensorSample.cs`, `Mystique.Core.Device/ISensorSource.cs`, `Mystique.Core.Device/SensorBridgeClient.cs`
- Test: `Mystique.Tests/SensorBridgeParsingTests.cs`, `Mystique.Tests/SensorBridgeHardwareTests.cs`
- Reference: current implementation at `MystiqueUsbTool/Program.cs:350-482`

**Interfaces:**
- Consumes: nothing from earlier tasks
- Produces:
  - `sealed record SensorSample(double CpuTemperature, double CpuUsage, double MemoryLoad, double GpuTemperature, double CpuClock)`
  - `interface ISensorSource : IDisposable` with `Task<SensorSample> ReadAsync(CancellationToken ct)`
  - `sealed class SensorBridgeClient : ISensorSource` with `static Task<SensorBridgeClient> ConnectAsync(CancellationToken ct)`
  - `static int SensorBridgeClient.ReadFrameLength(ReadOnlySpan<byte> header)` — throws on bad magic
  - `static SensorSample SensorBridgeClient.ParseSensorPayload(ReadOnlySpan<byte> jsonBytes)`
  - `const string SensorBridgeClient.ControlPipeName = "deepcool_sensor_data"`

- [ ] **Step 1: Write the failing parsing tests**

`Mystique.Tests/SensorBridgeParsingTests.cs`:

```csharp
using System.Text;
using System.Text.Json;
using Mystique.Core.Device;

namespace Mystique.Tests;

public class SensorBridgeParsingTests
{
    private static byte[] Envelope(string innerJson) =>
        Encoding.UTF8.GetBytes(JsonSerializer.Serialize(
            new { type = "GET_SENSOR_DATA", payload = innerJson }));

    private const string GoodInner = """
        {
          "CPU Temperature": { "Value": 55.5 },
          "CPU Usage":       { "Value": 12.25 },
          "Memory Load":     { "Value": 41.0 },
          "Gpu temperature": { "Value": 48.0 },
          "CPU Clock":       { "Value": 4612.5 }
        }
        """;

    [Fact]
    public void ReadsEverySensorValue()
    {
        SensorSample sample = SensorBridgeClient.ParseSensorPayload(Envelope(GoodInner));

        Assert.Equal(55.5, sample.CpuTemperature);
        Assert.Equal(12.25, sample.CpuUsage);
        Assert.Equal(41.0, sample.MemoryLoad);
        Assert.Equal(48.0, sample.GpuTemperature);
        Assert.Equal(4612.5, sample.CpuClock);
    }

    [Fact]
    public void RejectsAnUnexpectedMessageType()
    {
        byte[] bytes = Encoding.UTF8.GetBytes(JsonSerializer.Serialize(
            new { type = "CLIENT_HELLO", payload = GoodInner }));

        Assert.Throws<InvalidDataException>(() => SensorBridgeClient.ParseSensorPayload(bytes));
    }

    [Fact]
    public void RejectsAMissingSensor()
    {
        string missingGpu = """
            {
              "CPU Temperature": { "Value": 55.5 },
              "CPU Usage":       { "Value": 12.25 },
              "Memory Load":     { "Value": 41.0 },
              "CPU Clock":       { "Value": 4612.5 }
            }
            """;

        var error = Assert.Throws<InvalidDataException>(
            () => SensorBridgeClient.ParseSensorPayload(Envelope(missingGpu)));
        Assert.Contains("Gpu temperature", error.Message);
    }

    [Fact]
    public void RejectsANonNumericSensorValue()
    {
        string textValue = """
            {
              "CPU Temperature": { "Value": "hot" },
              "CPU Usage":       { "Value": 12.25 },
              "Memory Load":     { "Value": 41.0 },
              "Gpu temperature": { "Value": 48.0 },
              "CPU Clock":       { "Value": 4612.5 }
            }
            """;

        Assert.Throws<InvalidDataException>(
            () => SensorBridgeClient.ParseSensorPayload(Envelope(textValue)));
    }

    [Fact]
    public void ReadsTheFrameLengthFromAValidHeader()
    {
        // DE AD BE EF then a little-endian ushort length.
        byte[] header = [0xDE, 0xAD, 0xBE, 0xEF, 0x34, 0x12];

        Assert.Equal(0x1234, SensorBridgeClient.ReadFrameLength(header));
    }

    [Fact]
    public void RejectsAFrameWithWrongMagic()
    {
        byte[] header = [0xDE, 0xAD, 0xBE, 0xEE, 0x34, 0x12];

        var error = Assert.Throws<InvalidDataException>(
            () => SensorBridgeClient.ReadFrameLength(header));
        Assert.Contains("magic", error.Message, StringComparison.OrdinalIgnoreCase);
    }

    [Fact]
    public void RejectsATruncatedHeader()
    {
        Assert.Throws<InvalidDataException>(
            () => SensorBridgeClient.ReadFrameLength([0xDE, 0xAD, 0xBE]));
    }
}
```

`Mystique.Tests/SensorBridgeHardwareTests.cs`:

```csharp
using Mystique.Core.Device;

namespace Mystique.Tests;

/// <summary>
/// Requires the "Deep Cool Helper Service" running and serving
/// \\.\pipe\deepcool_sensor_data. Does not touch USB.
/// </summary>
[Trait("Category", "Hardware")]
public class SensorBridgeHardwareTests
{
    [Fact]
    public async Task ConnectsAndReceivesPlausibleReadings()
    {
        using var cts = new CancellationTokenSource(TimeSpan.FromSeconds(15));
        using SensorBridgeClient sensors = await SensorBridgeClient.ConnectAsync(cts.Token);

        SensorSample sample = await sensors.ReadAsync(cts.Token);

        Assert.InRange(sample.CpuTemperature, 1, 120);
        Assert.InRange(sample.CpuUsage, 0, 100);
        Assert.InRange(sample.MemoryLoad, 0, 100);
        Assert.InRange(sample.CpuClock, 100, 10000);
    }

    [Fact]
    public async Task PushCadenceIsRoughlyOncePerSecond()
    {
        using var cts = new CancellationTokenSource(TimeSpan.FromSeconds(30));
        using SensorBridgeClient sensors = await SensorBridgeClient.ConnectAsync(cts.Token);

        await sensors.ReadAsync(cts.Token);
        long start = Environment.TickCount64;
        for (int i = 0; i < 5; i++)
            await sensors.ReadAsync(cts.Token);
        double perSample = (Environment.TickCount64 - start) / 5.0;

        // Measured baseline is ~1.08 s. This is the refresh bottleneck, not USB.
        Assert.InRange(perSample, 500, 3000);
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `dotnet test Mystique.slnx -c Release --filter FullyQualifiedName~SensorBridge`
Expected: FAIL — `CS0103: The name 'SensorBridgeClient' does not exist in the current context`.

- [ ] **Step 3: Write the sample and interface**

`Mystique.Core.Device/SensorSample.cs`:

```csharp
namespace Mystique.Core.Device;

public sealed record SensorSample(
    double CpuTemperature,
    double CpuUsage,
    double MemoryLoad,
    double GpuTemperature,
    double CpuClock);
```

`Mystique.Core.Device/ISensorSource.cs`:

```csharp
namespace Mystique.Core.Device;

/// <summary>
/// A source of sensor readings. An interface so the supervisor state machine can
/// be driven from a fake in tests, and so a non-DeepCool source can be substituted
/// later without touching the supervisor.
/// </summary>
public interface ISensorSource : IDisposable
{
    /// <summary>
    /// Completes when the next sample is pushed. This is a subscription, not a
    /// poll: the bridge pushes at its own cadence (~1.08 s measured), which is what
    /// sets the LCD refresh rate.
    /// </summary>
    Task<SensorSample> ReadAsync(CancellationToken cancellationToken);
}
```

- [ ] **Step 4: Write the client**

`Mystique.Core.Device/SensorBridgeClient.cs`:

```csharp
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
        if (!root.TryGetProperty(name, out JsonElement sensor) ||
            !sensor.TryGetProperty("Value", out JsonElement value) ||
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
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `dotnet test Mystique.slnx -c Release --filter FullyQualifiedName~SensorBridge`
Expected: PASS, no failures.

If the hardware tests fail, confirm the bridge is up:

```powershell
Get-Service 'Deep Cool Helper Service'
[System.IO.Directory]::GetFiles('\\.\pipe\') | Where-Object { $_ -match 'deepcool_sensor_data' }
```

- [ ] **Step 6: Commit**

```bash
git add Mystique.Core.Device Mystique.Tests
git commit -m "feat: move sensor bridge client to Core and pin anonymous impersonation"
```

---

## Task 9: Rewire MystiqueUsbTool onto Core — end of Phase 1

`Program.cs` drops from 573 lines to a thin CLI. All six verbs keep their exact names and arguments so existing muscle memory and the README stay valid. The one behaviour change is deliberate: **the device identifier is redacted by default**, with `--show-device-id` to opt in.

`mystique-live.log:8` currently contains `command 0x12: 55 2E 12 00 41 41 42 42 43 43 44 44` — ASCII `AABBCCDD`, a per-device identifier printed by `FormatPrefix` on every `init` and every `live`. Because it is stored as hex byte pairs, a text search for the identifier does not find it, which is why the log files must be excluded wholesale rather than scrubbed.

**Files:**
- Modify: `MystiqueUsbTool/Program.cs` (full rewrite), `MystiqueUsbTool/MystiqueUsbTool.csproj`
- Modify: `Mystique.Core.Device/MystiquePackets.cs` (add `FormatResponseAscii`)
- Test: `Mystique.Tests/MystiquePacketsTests.cs` (add two cases)

**Interfaces:**
- Consumes: everything from Tasks 2-8
- Produces: `static string MystiquePackets.FormatResponseAscii(ReadOnlySpan<byte> response, bool showDeviceId = false)`

- [ ] **Step 1: Write the failing tests for ASCII redaction**

Append to `Mystique.Tests/MystiquePacketsTests.cs`:

```csharp
    [Fact]
    public void AsciiFormattingAlsoRedactsTheDeviceIdentifier()
    {
        byte[] response = [0x55, 0x2E, 0x12, 0x00, 0x41, 0x41, 0x42, 0x42, 0x43, 0x43, 0x44, 0x44];

        string text = MystiquePackets.FormatResponseAscii(response);

        Assert.DoesNotContain("AABBCCDD", text);
        Assert.Equal("U..........", text[..11]);
    }

    [Fact]
    public void AsciiFormattingCanShowTheDeviceIdentifier()
    {
        byte[] response = [0x55, 0x2E, 0x12, 0x00, 0x41, 0x41, 0x42, 0x42, 0x43, 0x43, 0x44, 0x44];

        Assert.Contains("AABBCCDD", MystiquePackets.FormatResponseAscii(response, showDeviceId: true));
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `dotnet test Mystique.slnx -c Release --filter FullyQualifiedName~MystiquePacketsTests`
Expected: FAIL — `CS0117: 'MystiquePackets' does not contain a definition for 'FormatResponseAscii'`.

- [ ] **Step 3: Add the ASCII formatter**

Append to `MystiquePackets` in `Mystique.Core.Device/MystiquePackets.cs`:

```csharp
    /// <summary>
    /// Renders a response as printable ASCII with the device identifier masked.
    /// The identifier is ASCII text in the response body, so the hex formatter
    /// alone is not sufficient redaction.
    /// </summary>
    public static string FormatResponseAscii(ReadOnlySpan<byte> response, bool showDeviceId = false)
    {
        var text = new char[response.Length];
        for (int i = 0; i < response.Length; i++)
        {
            bool redact = !showDeviceId
                && i >= DeviceIdOffset
                && i < DeviceIdOffset + DeviceIdLength;

            char c = (char)response[i];
            text[i] = redact || c < 0x20 || c > 0x7E ? '.' : c;
        }

        return new string(text);
    }
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `dotnet test Mystique.slnx -c Release --filter FullyQualifiedName~MystiquePacketsTests`
Expected: PASS, no failures.

- [ ] **Step 5: Rewrite the tool**

`MystiqueUsbTool/MystiqueUsbTool.csproj`:

```xml
<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <OutputType>Exe</OutputType>
    <AssemblyName>MystiqueUsbTool</AssemblyName>
  </PropertyGroup>
  <ItemGroup>
    <ProjectReference Include="..\Mystique.Core.Contracts\Mystique.Core.Contracts.csproj" />
    <ProjectReference Include="..\Mystique.Core.Device\Mystique.Core.Device.csproj" />
  </ItemGroup>
</Project>
```

`MystiqueUsbTool/Program.cs` — replace the entire file:

```csharp
using Mystique.Core.Contracts;
using Mystique.Core.Device;

namespace MystiqueUsbTool;

/// <summary>
/// Diagnostic CLI over Mystique.Core. Deliberately thin: all protocol and device
/// logic lives in Core so the service and this tool cannot drift apart.
/// </summary>
internal static class Program
{
    private const string Usage =
        "Usage: MystiqueUsbTool [pipes|status|init [rotation]|info|stream [seconds]|live [updates]] " +
        "[--show-device-id]";

    private static async Task<int> Main(string[] args)
    {
        bool showDeviceId = args.Contains("--show-device-id", StringComparer.OrdinalIgnoreCase);
        string[] positional = args
            .Where(a => !a.StartsWith("--", StringComparison.Ordinal))
            .ToArray();

        string operation = positional.Length == 0 ? "pipes" : positional[0].ToLowerInvariant();
        if (operation is not ("pipes" or "status" or "init" or "info" or "stream" or "live"))
        {
            Console.Error.WriteLine(Usage);
            return 2;
        }

        try
        {
            using MystiqueDevice device = MystiqueDevice.Open();
            Console.WriteLine($"Device: {device.RedactedPath}");
            PrintPipes(device);

            switch (operation)
            {
                case "pipes":
                    break;

                case "status":
                    PrintStatus(device.ReadStatus());
                    break;

                case "info":
                    PrintInfo(device, showDeviceId);
                    break;

                case "init":
                    RunInit(device, ReadRotation(positional), showDeviceId);
                    break;

                case "stream":
                    StreamSynthetic(device, ReadCount(positional, 30, 1, 600));
                    break;

                case "live":
                    RunInit(device, ReadRotation(positional), showDeviceId);
                    await StreamLiveAsync(device, ReadCount(positional, 0, 0, 86_400));
                    break;
            }

            return 0;
        }
        catch (MystiqueUsbException ex)
        {
            Console.Error.WriteLine($"ERROR ({ex.Fault.Kind}, Win32 {ex.Fault.Code}): {ex.Message}");
            if (ex.Fault.Kind == FaultKind.Yield)
                Console.Error.WriteLine("Close DeepCool.exe and stop MystiqueService, then retry.");
            return 1;
        }
        catch (Exception ex)
        {
            Console.Error.WriteLine($"ERROR: {ex.Message}");
            return 1;
        }
    }

    private static byte ReadRotation(string[] positional) =>
        positional.Length >= 2 && byte.TryParse(positional[1], out byte parsed) && parsed <= 3
            ? parsed
            : (byte)0;

    private static int ReadCount(string[] positional, int fallback, int minimum, int maximum) =>
        positional.Length >= 2 && int.TryParse(positional[1], out int parsed)
            ? Math.Clamp(parsed, minimum, maximum)
            : fallback;

    private static void PrintPipes(IMystiqueDevice device)
    {
        var pipes = device.DescribePipes();
        Console.WriteLine($"Interface 0: {pipes.Count} endpoints");
        foreach (PipeInfo pipe in pipes)
            Console.WriteLine($"  0x{pipe.PipeId:X2} {pipe.Type,-9} max-packet={pipe.MaximumPacketSize}");
    }

    private static void PrintStatus(DeviceStatus status) =>
        Console.WriteLine(
            $"Status: mode={status.Mode} (0x{status.RawMode:X2}), rotation={status.RotationDegrees} degrees");

    private static void PrintInfo(IMystiqueDevice device, bool showDeviceId)
    {
        // Redacted unless explicitly requested: bytes 4-11 are a per-device identifier.
        byte[] response = device.SendInfoRequest();
        Console.WriteLine($"Info hex:  {MystiquePackets.FormatResponse(response, showDeviceId)}");
        Console.WriteLine($"Info text: {MystiquePackets.FormatResponseAscii(response, showDeviceId)}");
        if (!showDeviceId)
            Console.WriteLine("(device identifier redacted; pass --show-device-id to reveal)");
    }

    private static void RunInit(IMystiqueDevice device, byte rotation, bool showDeviceId)
    {
        Console.WriteLine($"Sending reversible Machine Info initialization (rotation {rotation * 90} degrees)...");
        foreach (InitStep step in device.EnterMachineInfoMode(rotation))
        {
            Console.WriteLine(
                $"  command 0x{step.Command:X2}: {MystiquePackets.FormatResponse(step.Response, showDeviceId)}");
        }

        PrintStatus(device.ReadStatus());
    }

    private static void StreamSynthetic(IMystiqueDevice device, int seconds)
    {
        Console.WriteLine($"Sending {seconds} one-per-second synthetic telemetry updates...");
        for (int second = 0; second < seconds; second++)
        {
            var frame = new TelemetryFrame(
                CpuTemperatureC: (byte)(35 + second % 25),
                CpuUsagePercent: (byte)(10 + second % 80),
                MemoryLoadPercent: (byte)(40 + second % 20),
                GpuTemperatureC: (byte)(45 + second % 20),
                CpuClockMhz: (ushort)(4200 + second % 8 * 100));

            device.SendTelemetry(frame);
            Console.WriteLine(
                $"  {second + 1,3}/{seconds}: CPU {frame.CpuTemperatureC}C {frame.CpuUsagePercent}%  " +
                $"RAM {frame.MemoryLoadPercent}%  GPU {frame.GpuTemperatureC}C  {frame.CpuClockMhz}MHz");
            Thread.Sleep(1000);
        }
    }

    private static async Task StreamLiveAsync(IMystiqueDevice device, int updateLimit)
    {
        using var stop = new CancellationTokenSource();
        ConsoleCancelEventHandler cancelHandler = (_, e) =>
        {
            e.Cancel = true;
            stop.Cancel();
        };
        Console.CancelKeyPress += cancelHandler;

        try
        {
            using SensorBridgeClient sensors = await SensorBridgeClient.ConnectAsync(stop.Token);
            Console.WriteLine(updateLimit == 0
                ? "Streaming live telemetry until Ctrl+C..."
                : $"Streaming {updateLimit} live telemetry updates...");

            int sent = 0;
            while (!stop.IsCancellationRequested && (updateLimit == 0 || sent < updateLimit))
            {
                SensorSample sample = await sensors.ReadAsync(stop.Token);
                var frame = TelemetryFrame.FromRaw(
                    sample.CpuTemperature, sample.CpuUsage, sample.MemoryLoad,
                    sample.GpuTemperature, sample.CpuClock);

                device.SendTelemetry(frame);
                sent++;
                Console.WriteLine(
                    $"  {sent,5}: CPU {frame.CpuTemperatureC}C {frame.CpuUsagePercent}% " +
                    $"{frame.CpuClockMhz}MHz  RAM {frame.MemoryLoadPercent}%  GPU {frame.GpuTemperatureC}C");
            }
        }
        catch (OperationCanceledException) when (stop.IsCancellationRequested)
        {
            Console.WriteLine("Stopped.");
        }
        finally
        {
            Console.CancelKeyPress -= cancelHandler;
        }
    }
}
```

- [ ] **Step 6: Add the info request to the device interface**

`PrintInfo` calls `SendInfoRequest`, which does not exist yet. Add to `IMystiqueDevice` in `Mystique.Core.Device/IMystiqueDevice.cs`:

```csharp
    /// <summary>Sends command 0x12. The reply body carries a per-device identifier.</summary>
    byte[] SendInfoRequest();
```

And to `MystiqueDevice` in `Mystique.Core.Device/MystiqueDevice.cs`, next to `ReadStatus`:

```csharp
    public byte[] SendInfoRequest() =>
        Exchange(
            MystiquePackets.InitPipeOut, MystiquePackets.InitPipeIn,
            MystiquePackets.CommandInfo, []);
```

- [ ] **Step 7: Build and run the full suite**

Run: `dotnet build Mystique.slnx -c Release && dotnet test Mystique.slnx -c Release`
Expected: build clean, all tests pass including hardware.

- [ ] **Step 8: Verify redaction on the real device**

```powershell
dotnet run --project MystiqueUsbTool -c Release -- info
```

Expected: `Info hex:` shows `55 2E 12 00 .. .. .. .. .. .. .. ..` and `Info text:` shows no `AABBCCDD`. Then confirm the opt-in still works:

```powershell
dotnet run --project MystiqueUsbTool -c Release -- info --show-device-id
```

Expected: the identifier is visible.

- [ ] **Step 9: Verify `live` still behaves like the 12-hour baseline**

```powershell
dotnet run --project MystiqueUsbTool -c Release -- live 60
```

Expected: 60 updates at roughly one per second, no errors, panel updating. Sequence numbers ascend with no gaps. Compare against the baseline of 39,227 updates with zero failures — a 60-update run is a smoke test, not a substitute for the long run in Task 14.

- [ ] **Step 10: Verify rotation end-to-end**

```powershell
dotnet run --project MystiqueUsbTool -c Release -- init 2
dotnet run --project MystiqueUsbTool -c Release -- status
dotnet run --project MystiqueUsbTool -c Release -- init 0
```

Expected: the panel image rotates 180 degrees, `status` reports `rotation=180 degrees`, then it returns to 0.

- [ ] **Step 11: Commit — Phase 1 complete**

```bash
git add MystiqueUsbTool Mystique.Core.Device Mystique.Tests
git commit -m "refactor: rewire MystiqueUsbTool onto Core and redact device id by default"
```

At this point Phase 1 is done: same six verbs, same behaviour, all protocol logic tested, device identifier no longer leaking. Nothing user-visible has changed except redaction and the new optional `init <rotation>` argument.

---

# Phase 2 — the service

## Task 10: Logging

The current log is 5.7 MB of 40,000+ identically-shaped lines with **zero timestamps**, and it exists only because of shell redirection — `Program.cs` contains no file I/O at all. Under a service with no console, the entire operational log would silently evaporate.

Disk is not the problem (2.44 GiB/year against 171 GB free). The fix is **deleting the per-update line**, not rotation: state transitions plus one summary per 5 minutes is roughly 11 MiB/year. Rotation is a backstop.

`C:\ProgramData` grants `BUILTIN\Users` create-file and CREATOR OWNER `GENERIC_ALL`, so a standard user can pre-create files under a new subdirectory and own them. The directory must therefore be created with inheritance disabled and an explicit DACL.

**Files:**
- Create: `MystiqueService/MystiqueService.csproj`, `MystiqueService/ILog.cs`, `MystiqueService/RollingFileLog.cs`
- Test: `Mystique.Tests/RollingFileLogTests.cs`

**Interfaces:**
- Consumes: `LogLevel` from Task 5
- Produces:
  - `interface ILog` with `void Write(LogLevel level, string message)`, `IReadOnlyList<string> Recent(int count)`
  - extension-style helpers `Info`, `Warn`, `Error`, `Debug` on `ILog`
  - `sealed class RollingFileLog : ILog, IDisposable` — ctor `(string directory, TimeProvider time, long maxBytes = 8 * 1024 * 1024, int generations = 3)`
  - `static string RollingFileLog.DefaultDirectory` — `%ProgramData%\Mystique\logs`
  - `static void RollingFileLog.EnsureSecureDirectory(string path)`
  - `sealed class ConsoleLog : ILog`, `sealed class CompositeLog(params ILog[] sinks) : ILog`

- [ ] **Step 1: Write the failing tests**

`Mystique.Tests/RollingFileLogTests.cs`:

```csharp
using System.Security.AccessControl;
using System.Security.Principal;
using Mystique.Core.Device;
using MystiqueService;

namespace Mystique.Tests;

public sealed class RollingFileLogTests : IDisposable
{
    private readonly string directory = Path.Combine(
        Path.GetTempPath(), "mystique-log-tests", Guid.NewGuid().ToString("N"));

    public void Dispose()
    {
        if (Directory.Exists(directory))
            Directory.Delete(directory, recursive: true);
    }

    [Fact]
    public void EveryLineCarriesATimestampAndLevel()
    {
        var time = new FakeTimeProvider();
        using (var log = new RollingFileLog(directory, time))
            log.Write(LogLevel.Warn, "device not present");

        string line = File.ReadAllLines(Path.Combine(directory, "mystique.log")).Single();

        Assert.StartsWith("2026-01-01T00:00:00", line, StringComparison.Ordinal);
        Assert.Contains("WARN", line);
        Assert.EndsWith("device not present", line, StringComparison.Ordinal);
    }

    [Fact]
    public void TimestampsAdvanceWithTheClock()
    {
        var time = new FakeTimeProvider();
        using (var log = new RollingFileLog(directory, time))
        {
            log.Write(LogLevel.Info, "first");
            time.Advance(TimeSpan.FromMinutes(90));
            log.Write(LogLevel.Info, "second");
        }

        string[] lines = File.ReadAllLines(Path.Combine(directory, "mystique.log"));

        Assert.StartsWith("2026-01-01T00:00:00", lines[0], StringComparison.Ordinal);
        Assert.StartsWith("2026-01-01T01:30:00", lines[1], StringComparison.Ordinal);
    }

    [Fact]
    public void RotatesOnceTheSizeCapIsExceeded()
    {
        var time = new FakeTimeProvider();
        using (var log = new RollingFileLog(directory, time, maxBytes: 512, generations: 3))
        {
            for (int i = 0; i < 100; i++)
                log.Write(LogLevel.Info, $"line {i} padded out to force rotation quickly");
        }

        Assert.True(File.Exists(Path.Combine(directory, "mystique.log")));
        Assert.True(File.Exists(Path.Combine(directory, "mystique.1.log")));
    }

    [Fact]
    public void NeverKeepsMoreThanTheConfiguredGenerations()
    {
        var time = new FakeTimeProvider();
        using (var log = new RollingFileLog(directory, time, maxBytes: 256, generations: 2))
        {
            for (int i = 0; i < 400; i++)
                log.Write(LogLevel.Info, $"line {i} padded out to force many rotations");
        }

        Assert.Equal(2, Directory.GetFiles(directory, "mystique*.log").Length);
    }

    [Fact]
    public void RecentReturnsTheNewestLinesForTheTrayWithoutReadingTheFile()
    {
        var time = new FakeTimeProvider();
        using var log = new RollingFileLog(directory, time);

        for (int i = 0; i < 500; i++)
            log.Write(LogLevel.Info, $"line {i}");

        var recent = log.Recent(10);

        Assert.Equal(10, recent.Count);
        Assert.EndsWith("line 499", recent[^1], StringComparison.Ordinal);
        Assert.EndsWith("line 490", recent[0], StringComparison.Ordinal);
    }

    [Fact]
    public void RecentIsBoundedEvenWhenAskedForMore()
    {
        var time = new FakeTimeProvider();
        using var log = new RollingFileLog(directory, time);
        log.Write(LogLevel.Info, "only one");

        Assert.Single(log.Recent(1000));
    }

    [Fact]
    public void ConcurrentWritersDoNotCorruptOrLoseLines()
    {
        var time = new FakeTimeProvider();
        using (var log = new RollingFileLog(directory, time))
        {
            Parallel.For(0, 200, i => log.Write(LogLevel.Info, $"line {i}"));
        }

        string[] lines = File.ReadAllLines(Path.Combine(directory, "mystique.log"));

        Assert.Equal(200, lines.Length);
        Assert.All(lines, l => Assert.Contains("INFO", l));
    }

    [Fact]
    public void SecureDirectoryDisablesInheritanceAndDeniesUserWrites()
    {
        RollingFileLog.EnsureSecureDirectory(directory);

        var security = new DirectoryInfo(directory).GetAccessControl();
        var rules = security.GetAccessRules(true, true, typeof(SecurityIdentifier));

        Assert.True(security.AreAccessRulesProtected);

        var users = new SecurityIdentifier(WellKnownSidType.BuiltinUsersSid, null);
        foreach (FileSystemAccessRule rule in rules)
        {
            if (!rule.IdentityReference.Equals(users))
                continue;

            Assert.Equal(AccessControlType.Allow, rule.AccessControlType);
            Assert.Equal(0, (int)(rule.FileSystemRights & FileSystemRights.Write));
        }
    }

    [Fact]
    public void EnsureSecureDirectoryIsIdempotent()
    {
        RollingFileLog.EnsureSecureDirectory(directory);
        RollingFileLog.EnsureSecureDirectory(directory);

        Assert.True(Directory.Exists(directory));
    }

    [Fact]
    public void CompositeWritesToEverySink()
    {
        var time = new FakeTimeProvider();
        using var first = new RollingFileLog(Path.Combine(directory, "a"), time);
        using var second = new RollingFileLog(Path.Combine(directory, "b"), time);
        var composite = new CompositeLog(first, second);

        composite.Write(LogLevel.Error, "both");

        Assert.Single(first.Recent(10));
        Assert.Single(second.Recent(10));
    }
}
```

- [ ] **Step 2: Create the service project**

```bash
dotnet new console -n MystiqueService -o MystiqueService
dotnet sln add MystiqueService/MystiqueService.csproj
dotnet add MystiqueService/MystiqueService.csproj reference Mystique.Core.Contracts/Mystique.Core.Contracts.csproj Mystique.Core.Device/Mystique.Core.Device.csproj
dotnet add MystiqueService/MystiqueService.csproj package System.ServiceProcess.ServiceController
dotnet add Mystique.Tests/Mystique.Tests.csproj reference MystiqueService/MystiqueService.csproj
```

Delete the generated `<TargetFramework>` line and the template `Program.cs` body (Task 13 replaces it; leave a stub `internal static class Program { private static int Main() => 0; }` so the project builds).

Confirm the package resolved to a 10.x version and that it brought in only `System.Diagnostics.EventLog`:

```bash
dotnet list MystiqueService/MystiqueService.csproj package --include-transitive
```

Final `MystiqueService.csproj`:

```xml
<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <OutputType>Exe</OutputType>
    <RootNamespace>MystiqueService</RootNamespace>
    <AssemblyName>MystiqueService</AssemblyName>
  </PropertyGroup>
  <ItemGroup>
    <PackageReference Include="System.ServiceProcess.ServiceController" Version="10.0.*" />
  </ItemGroup>
  <ItemGroup>
    <ProjectReference Include="..\Mystique.Core.Contracts\Mystique.Core.Contracts.csproj" />
    <ProjectReference Include="..\Mystique.Core.Device\Mystique.Core.Device.csproj" />
  </ItemGroup>
</Project>
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `dotnet test Mystique.slnx -c Release --filter FullyQualifiedName~RollingFileLogTests`
Expected: FAIL — `CS0246: The type or namespace name 'RollingFileLog' could not be found`.

- [ ] **Step 4: Write `ILog`**

`MystiqueService/ILog.cs`:

```csharp
using Mystique.Core.Device;

namespace MystiqueService;

public interface ILog
{
    void Write(LogLevel level, string message);

    /// <summary>Newest lines, oldest first. Served from memory so a reader never touches the file.</summary>
    IReadOnlyList<string> Recent(int count);
}

public static class LogExtensions
{
    public static void Debug(this ILog log, string message) => log.Write(LogLevel.Debug, message);
    public static void Info(this ILog log, string message) => log.Write(LogLevel.Info, message);
    public static void Warn(this ILog log, string message) => log.Write(LogLevel.Warn, message);
    public static void Error(this ILog log, string message) => log.Write(LogLevel.Error, message);
}

public sealed class ConsoleLog : ILog
{
    private readonly Lock gate = new();
    private readonly Queue<string> recent = new();

    public void Write(LogLevel level, string message)
    {
        string line = $"{DateTimeOffset.Now:O} {level.ToString().ToUpperInvariant(),-5} {message}";
        lock (gate)
        {
            recent.Enqueue(line);
            while (recent.Count > 200)
                recent.Dequeue();
        }

        if (level >= LogLevel.Warn)
            Console.Error.WriteLine(line);
        else
            Console.WriteLine(line);
    }

    public IReadOnlyList<string> Recent(int count)
    {
        lock (gate)
            return recent.TakeLast(count).ToArray();
    }
}

public sealed class CompositeLog(params ILog[] sinks) : ILog
{
    public void Write(LogLevel level, string message)
    {
        foreach (ILog sink in sinks)
            sink.Write(level, message);
    }

    public IReadOnlyList<string> Recent(int count) =>
        sinks.Length == 0 ? [] : sinks[0].Recent(count);
}
```

- [ ] **Step 5: Write `RollingFileLog`**

`MystiqueService/RollingFileLog.cs`:

```csharp
using System.Security.AccessControl;
using System.Security.Principal;
using Mystique.Core.Device;

namespace MystiqueService;

/// <summary>
/// Timestamped rolling log plus an in-memory ring the tray can read without
/// touching the file. Rotation is a backstop: the real volume control is not
/// logging one line per telemetry update.
/// </summary>
public sealed class RollingFileLog : ILog, IDisposable
{
    private const int RingCapacity = 200;
    private const string BaseName = "mystique";

    private readonly Lock gate = new();
    private readonly Queue<string> ring = new();
    private readonly string directory;
    private readonly string currentPath;
    private readonly TimeProvider time;
    private readonly long maxBytes;
    private readonly int generations;
    private StreamWriter writer;
    private long written;

    public RollingFileLog(
        string directory,
        TimeProvider time,
        long maxBytes = 8 * 1024 * 1024,
        int generations = 3)
    {
        this.directory = directory;
        this.time = time;
        this.maxBytes = maxBytes;
        this.generations = Math.Max(1, generations);

        EnsureSecureDirectory(directory);
        currentPath = Path.Combine(directory, $"{BaseName}.log");
        writer = Open(currentPath, out written);
    }

    public static string DefaultDirectory => Path.Combine(
        Environment.GetFolderPath(Environment.SpecialFolder.CommonApplicationData),
        "Mystique",
        "logs");

    /// <summary>
    /// Creates the directory with inheritance disabled and an explicit DACL.
    /// Inherited C:\ProgramData permissions would otherwise let a standard user
    /// pre-create files here and own them.
    /// </summary>
    public static void EnsureSecureDirectory(string path)
    {
        Directory.CreateDirectory(path);

        var info = new DirectoryInfo(path);
        DirectorySecurity security = info.GetAccessControl();
        security.SetAccessRuleProtection(isProtected: true, preserveInheritance: false);

        foreach (FileSystemAccessRule existing in
                 security.GetAccessRules(true, false, typeof(SecurityIdentifier)))
            security.RemoveAccessRule(existing);

        Grant(security, WellKnownSidType.LocalSystemSid, FileSystemRights.FullControl);
        Grant(security, WellKnownSidType.BuiltinAdministratorsSid, FileSystemRights.FullControl);
        Grant(security, WellKnownSidType.BuiltinUsersSid, FileSystemRights.ReadAndExecute);

        info.SetAccessControl(security);
    }

    public void Write(LogLevel level, string message)
    {
        string line =
            $"{time.GetUtcNow():yyyy-MM-ddTHH:mm:ss.fffzzz} {level.ToString().ToUpperInvariant(),-5} {message}";

        lock (gate)
        {
            ring.Enqueue(line);
            while (ring.Count > RingCapacity)
                ring.Dequeue();

            writer.WriteLine(line);
            written += line.Length + Environment.NewLine.Length;

            if (written >= maxBytes)
                Rotate();
        }
    }

    public IReadOnlyList<string> Recent(int count)
    {
        lock (gate)
            return ring.TakeLast(Math.Max(0, count)).ToArray();
    }

    public void Dispose()
    {
        lock (gate)
            writer.Dispose();
    }

    private void Rotate()
    {
        writer.Dispose();

        string oldest = Path.Combine(directory, $"{BaseName}.{generations - 1}.log");
        if (File.Exists(oldest))
            File.Delete(oldest);

        for (int generation = generations - 2; generation >= 1; generation--)
        {
            string from = Path.Combine(directory, $"{BaseName}.{generation}.log");
            if (File.Exists(from))
                File.Move(from, Path.Combine(directory, $"{BaseName}.{generation + 1}.log"), overwrite: true);
        }

        if (generations > 1)
            File.Move(currentPath, Path.Combine(directory, $"{BaseName}.1.log"), overwrite: true);
        else
            File.Delete(currentPath);

        writer = Open(currentPath, out written);
    }

    private static StreamWriter Open(string path, out long length)
    {
        var stream = new FileStream(path, FileMode.Append, FileAccess.Write, FileShare.Read);
        length = stream.Length;
        return new StreamWriter(stream) { AutoFlush = true };
    }

    private static void Grant(DirectorySecurity security, WellKnownSidType sid, FileSystemRights rights) =>
        security.AddAccessRule(new FileSystemAccessRule(
            new SecurityIdentifier(sid, null),
            rights,
            InheritanceFlags.ContainerInherit | InheritanceFlags.ObjectInherit,
            PropagationFlags.None,
            AccessControlType.Allow));
}
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `dotnet test Mystique.slnx -c Release --filter FullyQualifiedName~RollingFileLogTests`
Expected: PASS, no failures.

- [ ] **Step 7: Commit**

```bash
git add MystiqueService Mystique.Tests Mystique.slnx
git commit -m "feat: add rolling log with timestamps, ring buffer, and a protected ProgramData DACL"
```

---

## Task 11: DeepCool coexistence watcher

**Policy: yield, never fight.** When `DeepCool.exe` appears we release the handle pre-emptively rather than losing it. That is the direct antidote to "DeepCool repeatedly tears down a healthy WinUSB handle" — if we are already gone, there is no handle to tear down and no reset war.

Yielding is not a failure. Grey state, no failure counters, never ERROR level, one INFO line on entry, a heartbeat every 15 minutes, one line on exit.

**Files:**
- Create: `MystiqueService/DeepCoolWatcher.cs`
- Test: `Mystique.Tests/DeepCoolWatcherTests.cs`

**Interfaces:**
- Consumes: nothing
- Produces:
  - `enum DeepCoolTransition { Unchanged, Appeared, Exited }`
  - `sealed class DeepCoolWatcher` — ctor `(Func<bool> isRunning, TimeProvider time)`, `DeepCoolTransition Poll()`, `bool IsPresent { get; }`, `bool ShouldLogHeartbeat()`
  - `static bool DeepCoolWatcher.ProcessIsRunning()`
  - `static readonly TimeSpan DeepCoolWatcher.PollInterval` (2 s), `HeartbeatInterval` (15 min), `SettleDelay` (3 s)

- [ ] **Step 1: Write the failing tests**

`Mystique.Tests/DeepCoolWatcherTests.cs`:

```csharp
using MystiqueService;

namespace Mystique.Tests;

public class DeepCoolWatcherTests
{
    [Fact]
    public void ReportsNoTransitionWhileDeepCoolStaysAway()
    {
        bool running = false;
        var watcher = new DeepCoolWatcher(() => running, new FakeTimeProvider());

        Assert.Equal(DeepCoolTransition.Unchanged, watcher.Poll());
        Assert.Equal(DeepCoolTransition.Unchanged, watcher.Poll());
        Assert.False(watcher.IsPresent);
    }

    [Fact]
    public void DetectsAppearanceExactlyOnce()
    {
        bool running = false;
        var watcher = new DeepCoolWatcher(() => running, new FakeTimeProvider());
        watcher.Poll();

        running = true;

        Assert.Equal(DeepCoolTransition.Appeared, watcher.Poll());
        Assert.Equal(DeepCoolTransition.Unchanged, watcher.Poll());
        Assert.True(watcher.IsPresent);
    }

    [Fact]
    public void DetectsExitExactlyOnce()
    {
        bool running = true;
        var watcher = new DeepCoolWatcher(() => running, new FakeTimeProvider());
        watcher.Poll();

        running = false;

        Assert.Equal(DeepCoolTransition.Exited, watcher.Poll());
        Assert.Equal(DeepCoolTransition.Unchanged, watcher.Poll());
        Assert.False(watcher.IsPresent);
    }

    [Fact]
    public void DeepCoolAlreadyRunningAtStartupCountsAsAnAppearance()
    {
        var watcher = new DeepCoolWatcher(() => true, new FakeTimeProvider());

        Assert.Equal(DeepCoolTransition.Appeared, watcher.Poll());
    }

    [Fact]
    public void HeartbeatIsSilentForTheFirstFifteenMinutes()
    {
        var time = new FakeTimeProvider();
        var watcher = new DeepCoolWatcher(() => true, time);
        watcher.Poll();

        time.Advance(TimeSpan.FromMinutes(14));

        Assert.False(watcher.ShouldLogHeartbeat());
    }

    [Fact]
    public void HeartbeatFiresEveryFifteenMinutesWhileYielded()
    {
        var time = new FakeTimeProvider();
        var watcher = new DeepCoolWatcher(() => true, time);
        watcher.Poll();

        time.Advance(TimeSpan.FromMinutes(15));
        Assert.True(watcher.ShouldLogHeartbeat());
        Assert.False(watcher.ShouldLogHeartbeat());

        time.Advance(TimeSpan.FromMinutes(15));
        Assert.True(watcher.ShouldLogHeartbeat());
    }

    [Fact]
    public void EightHoursYieldedProducesAboutThirtyTwoHeartbeats()
    {
        var time = new FakeTimeProvider();
        var watcher = new DeepCoolWatcher(() => true, time);
        watcher.Poll();
        int beats = 0;

        // Poll every 2 s for 8 hours.
        for (int i = 0; i < 8 * 1800; i++)
        {
            watcher.Poll();
            if (watcher.ShouldLogHeartbeat())
                beats++;
            time.Advance(TimeSpan.FromSeconds(2));
        }

        Assert.Equal(32, beats);
    }

    [Fact]
    public void HeartbeatDoesNotFireWhenDeepCoolIsAbsent()
    {
        var time = new FakeTimeProvider();
        var watcher = new DeepCoolWatcher(() => false, time);
        watcher.Poll();

        time.Advance(TimeSpan.FromHours(4));

        Assert.False(watcher.ShouldLogHeartbeat());
    }

    [Fact]
    public void HeartbeatClockRestartsOnEachAppearance()
    {
        var time = new FakeTimeProvider();
        bool running = true;
        var watcher = new DeepCoolWatcher(() => running, time);
        watcher.Poll();

        time.Advance(TimeSpan.FromMinutes(14));
        running = false;
        watcher.Poll();
        running = true;
        watcher.Poll();

        time.Advance(TimeSpan.FromMinutes(14));
        Assert.False(watcher.ShouldLogHeartbeat());

        time.Advance(TimeSpan.FromMinutes(1));
        Assert.True(watcher.ShouldLogHeartbeat());
    }

    [Fact]
    [Trait("Category", "Hardware")]
    public void RealProcessProbeAgreesWithTheProcessTable()
    {
        bool expected = System.Diagnostics.Process.GetProcessesByName("DeepCool").Length > 0;

        Assert.Equal(expected, DeepCoolWatcher.ProcessIsRunning());
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `dotnet test Mystique.slnx -c Release --filter FullyQualifiedName~DeepCoolWatcherTests`
Expected: FAIL — `CS0246: The type or namespace name 'DeepCoolWatcher' could not be found`.

- [ ] **Step 3: Write the watcher**

`MystiqueService/DeepCoolWatcher.cs`:

```csharp
using System.Diagnostics;

namespace MystiqueService;

public enum DeepCoolTransition
{
    Unchanged,
    Appeared,
    Exited,
}

/// <summary>
/// Watches for DeepCool.exe so the supervisor can step aside before it takes the
/// device, rather than discovering the loss as an error.
///
/// <para>Only DeepCool.exe matters here. The two DeepCool <i>services</i> stay
/// running and supply the sensor data we consume; they are not competitors.</para>
/// </summary>
public sealed class DeepCoolWatcher(Func<bool> isRunning, TimeProvider time)
{
    public static readonly TimeSpan PollInterval = TimeSpan.FromSeconds(2);
    public static readonly TimeSpan HeartbeatInterval = TimeSpan.FromMinutes(15);

    /// <summary>How long to let the device settle after DeepCool exits before reopening.</summary>
    public static readonly TimeSpan SettleDelay = TimeSpan.FromSeconds(3);

    private bool present;
    private DateTimeOffset lastHeartbeat;

    public bool IsPresent => present;

    public DeepCoolTransition Poll()
    {
        bool now = isRunning();
        if (now == present)
            return DeepCoolTransition.Unchanged;

        present = now;
        if (present)
        {
            lastHeartbeat = time.GetUtcNow();
            return DeepCoolTransition.Appeared;
        }

        return DeepCoolTransition.Exited;
    }

    /// <summary>
    /// True at most once per heartbeat interval while yielded. Yielding is a normal
    /// state, so it gets a slow heartbeat rather than repeated warnings.
    /// </summary>
    public bool ShouldLogHeartbeat()
    {
        if (!present)
            return false;

        DateTimeOffset now = time.GetUtcNow();
        if (now - lastHeartbeat < HeartbeatInterval)
            return false;

        lastHeartbeat = now;
        return true;
    }

    public static bool ProcessIsRunning()
    {
        Process[] found = Process.GetProcessesByName("DeepCool");
        try
        {
            return found.Length > 0;
        }
        finally
        {
            foreach (Process process in found)
                process.Dispose();
        }
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `dotnet test Mystique.slnx -c Release --filter FullyQualifiedName~DeepCoolWatcherTests`
Expected: PASS, no failures.

- [ ] **Step 5: Commit**

```bash
git add MystiqueService/DeepCoolWatcher.cs Mystique.Tests/DeepCoolWatcherTests.cs
git commit -m "feat: detect DeepCool.exe so the service yields the device pre-emptively"
```

---

## Task 12: The supervisor

The state machine that turns a fail-fast tool into a service. Seven states, closed set: `Starting → Streaming → Degraded → Reconnecting → Waiting → Yielded → Stopped`. Every transition logs one line with the reason.

Two non-negotiables enforced by tests here:
- **It never exits on a failure path.** No `Environment.Exit`, no unhandled exception escaping the loop. Giving up means a visible `Stopped` state, never a silent death.
- **After any reopen, `EnterMachineInfoMode` runs again before `Streaming` is declared.** A reopened device is not in Machine Info mode; telemetry would be ACKed while the panel shows nothing.

Backoff counters are **per subsystem** — a chatty sensor bridge must not push USB retries to the cap.

**Files:**
- Create: `MystiqueService/Supervisor.cs`
- Test: `Mystique.Tests/SupervisorTests.cs`, `Mystique.Tests/FakeMystiqueDevice.cs`, `Mystique.Tests/FakeSensorSource.cs`

**Interfaces:**
- Consumes: `IMystiqueDevice`, `ISensorSource`, `UsbFault`, `MystiqueUsbException`, `BackoffSchedule`, `FailureEscalator`, `DeepCoolWatcher`, `ILog`, `StatusSnapshot`, `ServiceState`, `TelemetryFrame`
- Produces:
  - `delegate Task DelayAsync(TimeSpan duration, CancellationToken cancellationToken)`
  - `sealed record SupervisorOptions(byte Rotation = 0, int SummaryEveryUpdates = 300)`
  - `sealed class Supervisor` — ctor `(Func<IMystiqueDevice> deviceFactory, Func<CancellationToken, Task<ISensorSource>> sensorFactory, DeepCoolWatcher deepCool, ILog log, TimeProvider time, DelayAsync delay, SupervisorOptions options)`
  - `StatusSnapshot Status { get; }`, `event Action<StatusSnapshot>? StatusChanged`
  - `Task RunAsync(CancellationToken cancellationToken)`
  - `void SetRotation(byte rotation)`, `void RequestRetryNow()`

- [ ] **Step 1: Write the fakes**

`Mystique.Tests/FakeMystiqueDevice.cs`:

```csharp
using Mystique.Core.Contracts;
using Mystique.Core.Device;

namespace Mystique.Tests;

public sealed class FakeMystiqueDevice : IMystiqueDevice
{
    public string RedactedPath => @"\\?\USB#VID_3633&PID_0009#<serial>#{guid}";

    public int InitCalls { get; private set; }
    public int TelemetryCalls { get; private set; }
    public int RecoverCalls { get; private set; }
    public int AbortCalls { get; private set; }
    public bool Disposed { get; private set; }
    public List<byte> RotationsRequested { get; } = [];

    /// <summary>Throws on the Nth telemetry call (1-based). Null disables.</summary>
    public Func<int, Exception?>? TelemetryFault { get; set; }

    public Exception? InitFault { get; set; }

    public IReadOnlyList<PipeInfo> DescribePipes() =>
    [
        new(0x01, UsbdPipeType.Bulk, 64), new(0x81, UsbdPipeType.Bulk, 64),
        new(0x02, UsbdPipeType.Bulk, 64), new(0x82, UsbdPipeType.Bulk, 64),
    ];

    public IReadOnlyList<InitStep> EnterMachineInfoMode(byte rotation)
    {
        ObjectDisposedException.ThrowIf(Disposed, this);
        InitCalls++;
        RotationsRequested.Add(rotation);
        if (InitFault is { } fault)
            throw fault;

        return [new InitStep(0x12, [0x55, 0x2E, 0x12, 0x00])];
    }

    public DeviceStatus ReadStatus()
    {
        ObjectDisposedException.ThrowIf(Disposed, this);
        return new DeviceStatus(DisplayMode.MachineInfo, 0xFF, 0);
    }

    public byte[] SendInfoRequest() => [0x55, 0x2E, 0x12, 0x00];

    public byte[] SendTelemetry(in TelemetryFrame frame)
    {
        ObjectDisposedException.ThrowIf(Disposed, this);
        TelemetryCalls++;
        if (TelemetryFault?.Invoke(TelemetryCalls) is { } fault)
            throw fault;

        return [0x55, 0x2E, 0x01, 0x00];
    }

    public void Recover(RecoveryAction action) => RecoverCalls++;

    public void AbortPendingTransfers() => AbortCalls++;

    public void Dispose() => Disposed = true;
}
```

`Mystique.Tests/FakeSensorSource.cs`:

```csharp
using Mystique.Core.Device;

namespace Mystique.Tests;

public sealed class FakeSensorSource : ISensorSource
{
    private int reads;

    public bool Disposed { get; private set; }
    public int Reads => reads;

    /// <summary>Throws on the Nth read (1-based). Null disables.</summary>
    public Func<int, Exception?>? ReadFault { get; set; }

    public Task<SensorSample> ReadAsync(CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();
        int n = Interlocked.Increment(ref reads);
        if (ReadFault?.Invoke(n) is { } fault)
            throw fault;

        return Task.FromResult(new SensorSample(55, 12, 41, 48, 4600));
    }

    public void Dispose() => Disposed = true;
}
```

- [ ] **Step 2: Write the failing tests**

`Mystique.Tests/SupervisorTests.cs`:

```csharp
using Mystique.Core.Contracts;
using Mystique.Core.Device;
using MystiqueService;

namespace Mystique.Tests;

public class SupervisorTests
{
    private static readonly SupervisorOptions Options = new(Rotation: 0, SummaryEveryUpdates: 300);

    /// <summary>No real waiting: delays complete immediately so loops run at full speed.</summary>
    private static DelayAsync NoDelay => (_, ct) => Task.FromResult(ct.IsCancellationRequested);

    private static Supervisor Build(
        Func<IMystiqueDevice> deviceFactory,
        Func<CancellationToken, Task<ISensorSource>> sensorFactory,
        Func<bool> deepCoolRunning,
        ILog? log = null,
        SupervisorOptions? options = null,
        DelayAsync? delay = null)
    {
        var time = new FakeTimeProvider();
        return new Supervisor(
            deviceFactory,
            sensorFactory,
            new DeepCoolWatcher(deepCoolRunning, time),
            log ?? new ConsoleLog(),
            time,
            delay ?? NoDelay,
            options ?? Options);
    }

    /// <summary>Runs the supervisor until <paramref name="until"/> holds, or the guard trips.</summary>
    private static async Task RunUntil(Supervisor supervisor, Func<bool> until, int guardIterations = 5000)
    {
        using var cts = new CancellationTokenSource();
        Task run = supervisor.RunAsync(cts.Token);

        for (int i = 0; i < guardIterations && !until(); i++)
            await Task.Yield();

        await cts.CancelAsync();
        await run;
    }

    [Fact]
    public async Task StreamsFramesOnTheHappyPath()
    {
        var device = new FakeMystiqueDevice();
        var sensors = new FakeSensorSource();
        var supervisor = Build(() => device, _ => Task.FromResult<ISensorSource>(sensors), () => false);

        await RunUntil(supervisor, () => device.TelemetryCalls >= 10);

        Assert.True(device.TelemetryCalls >= 10);
        Assert.Equal(1, device.InitCalls);
        Assert.Equal(ServiceState.Stopped, supervisor.Status.State);
    }

    [Fact]
    public async Task ReportsStreamingWhileFramesFlow()
    {
        var device = new FakeMystiqueDevice();
        var states = new List<ServiceState>();
        var supervisor = Build(
            () => device, _ => Task.FromResult<ISensorSource>(new FakeSensorSource()), () => false);
        supervisor.StatusChanged += s => states.Add(s.State);

        await RunUntil(supervisor, () => device.TelemetryCalls >= 5);

        Assert.Contains(ServiceState.Streaming, states);
    }

    [Fact]
    public async Task DeviceAbsentAtBootIsWaitingNotAnError()
    {
        int attempts = 0;
        var device = new FakeMystiqueDevice();
        var states = new List<ServiceState>();

        var supervisor = Build(
            () => ++attempts < 3
                ? throw new MystiqueUsbException(UsbFault.Classify(2), "CreateFile")
                : device,
            _ => Task.FromResult<ISensorSource>(new FakeSensorSource()),
            () => false);
        supervisor.StatusChanged += s => states.Add(s.State);

        await RunUntil(supervisor, () => device.TelemetryCalls >= 1);

        Assert.Contains(ServiceState.Waiting, states);
        Assert.DoesNotContain(ServiceState.Reconnecting, states);
        Assert.True(device.TelemetryCalls >= 1);
    }

    [Fact]
    public async Task AccessDeniedIsTreatedAsAYield()
    {
        var states = new List<ServiceState>();
        var supervisor = Build(
            () => throw new MystiqueUsbException(UsbFault.Classify(5), "CreateFile"),
            _ => Task.FromResult<ISensorSource>(new FakeSensorSource()),
            () => false);
        supervisor.StatusChanged += s => states.Add(s.State);

        await RunUntil(supervisor, () => states.Contains(ServiceState.Yielded));

        Assert.Contains(ServiceState.Yielded, states);
    }

    [Fact]
    public async Task DeepCoolAppearingReleasesTheHandleWithoutTouchingTheDevice()
    {
        var device = new FakeMystiqueDevice();
        bool deepCool = false;
        var supervisor = Build(
            () => device, _ => Task.FromResult<ISensorSource>(new FakeSensorSource()), () => deepCool);

        using var cts = new CancellationTokenSource();
        Task run = supervisor.RunAsync(cts.Token);
        while (device.TelemetryCalls < 3)
            await Task.Yield();

        deepCool = true;
        while (!device.Disposed)
            await Task.Yield();
        int callsAtYield = device.TelemetryCalls;

        for (int i = 0; i < 200; i++)
            await Task.Yield();

        await cts.CancelAsync();
        await run;

        Assert.Equal(callsAtYield, device.TelemetryCalls); // zero device calls while yielded
        Assert.Equal(ServiceState.Yielded, supervisor.Status.State);
    }

    [Fact]
    public async Task DeepCoolExitingReinitialisesTheDeviceNotJustReopensIt()
    {
        var devices = new List<FakeMystiqueDevice>();
        bool deepCool = true;
        var supervisor = Build(
            () => { var d = new FakeMystiqueDevice(); devices.Add(d); return d; },
            _ => Task.FromResult<ISensorSource>(new FakeSensorSource()),
            () => deepCool);

        using var cts = new CancellationTokenSource();
        Task run = supervisor.RunAsync(cts.Token);
        while (supervisor.Status.State != ServiceState.Yielded)
            await Task.Yield();

        deepCool = false;
        while (devices.Count == 0 || devices[^1].TelemetryCalls < 1)
            await Task.Yield();

        await cts.CancelAsync();
        await run;

        // Trap 2: a reopened device is not in Machine Info mode. Init must re-run.
        Assert.Equal(1, devices[^1].InitCalls);
    }

    [Fact]
    public async Task SurpriseRemovalDisposesTheDeviceAndReacquires()
    {
        var devices = new List<FakeMystiqueDevice>();
        var supervisor = Build(
            () =>
            {
                var d = new FakeMystiqueDevice();
                if (devices.Count == 0)
                    d.TelemetryFault = n => n == 3
                        ? new MystiqueUsbException(UsbFault.Classify(1167), "write")
                        : null;
                devices.Add(d);
                return d;
            },
            _ => Task.FromResult<ISensorSource>(new FakeSensorSource()),
            () => false);

        await RunUntil(supervisor, () => devices.Count >= 2 && devices[1].TelemetryCalls >= 2);

        Assert.True(devices[0].Disposed);
        Assert.Equal(1, devices[1].InitCalls);
    }

    [Fact]
    public async Task SensorFailureDegradesButKeepsTheDevice()
    {
        var device = new FakeMystiqueDevice();
        int connects = 0;
        var states = new List<ServiceState>();

        var supervisor = Build(
            () => device,
            _ => ++connects == 1
                ? Task.FromException<ISensorSource>(new TimeoutException("bridge not up"))
                : Task.FromResult<ISensorSource>(new FakeSensorSource()),
            () => false);
        supervisor.StatusChanged += s => states.Add(s.State);

        await RunUntil(supervisor, () => device.TelemetryCalls >= 3);

        Assert.Contains(ServiceState.Degraded, states);
        Assert.False(device.Disposed);
    }

    [Fact]
    public async Task AnUnknownExceptionNeverEscapesTheLoop()
    {
        int attempts = 0;
        var device = new FakeMystiqueDevice();
        var supervisor = Build(
            () => ++attempts < 4 ? throw new InvalidOperationException("nonsense") : device,
            _ => Task.FromResult<ISensorSource>(new FakeSensorSource()),
            () => false);

        await RunUntil(supervisor, () => device.TelemetryCalls >= 1);

        Assert.True(device.TelemetryCalls >= 1); // recovered rather than dying
    }

    [Fact]
    public async Task CancellationStopsCleanlyAndReleasesTheDevice()
    {
        var device = new FakeMystiqueDevice();
        var sensors = new FakeSensorSource();
        var supervisor = Build(() => device, _ => Task.FromResult<ISensorSource>(sensors), () => false);

        using var cts = new CancellationTokenSource();
        Task run = supervisor.RunAsync(cts.Token);
        while (device.TelemetryCalls < 2)
            await Task.Yield();
        await cts.CancelAsync();
        await run;

        Assert.True(device.Disposed);
        Assert.True(sensors.Disposed);
        Assert.Equal(ServiceState.Stopped, supervisor.Status.State);
        Assert.True(device.AbortCalls >= 1);
    }

    [Fact]
    public async Task RotationIsAppliedOnEveryInitSoItSurvivesReconnects()
    {
        var devices = new List<FakeMystiqueDevice>();
        var supervisor = Build(
            () =>
            {
                var d = new FakeMystiqueDevice();
                if (devices.Count == 0)
                    d.TelemetryFault = n => n == 2
                        ? new MystiqueUsbException(UsbFault.Classify(1167), "write")
                        : null;
                devices.Add(d);
                return d;
            },
            _ => Task.FromResult<ISensorSource>(new FakeSensorSource()),
            () => false,
            options: new SupervisorOptions(Rotation: 2));

        await RunUntil(supervisor, () => devices.Count >= 2 && devices[1].TelemetryCalls >= 1);

        Assert.All(devices, d => Assert.Equal([(byte)2], d.RotationsRequested));
    }

    [Fact]
    public async Task SetRotationForcesReinitialisation()
    {
        var device = new FakeMystiqueDevice();
        var supervisor = Build(
            () => device, _ => Task.FromResult<ISensorSource>(new FakeSensorSource()), () => false);

        using var cts = new CancellationTokenSource();
        Task run = supervisor.RunAsync(cts.Token);
        while (device.TelemetryCalls < 2)
            await Task.Yield();

        supervisor.SetRotation(3);
        while (device.InitCalls < 2)
            await Task.Yield();

        await cts.CancelAsync();
        await run;

        Assert.Equal([(byte)0, (byte)3], device.RotationsRequested);
    }

    [Fact]
    public async Task StatusCarriesTheLastFrameAndUpdateCount()
    {
        var device = new FakeMystiqueDevice();
        var supervisor = Build(
            () => device, _ => Task.FromResult<ISensorSource>(new FakeSensorSource()), () => false);

        await RunUntil(supervisor, () => device.TelemetryCalls >= 4);

        Assert.True(supervisor.Status.UpdatesSent >= 4);
        Assert.NotNull(supervisor.Status.Last);
        Assert.Equal(55, supervisor.Status.Last!.Value.CpuTemperatureC);
    }

    [Fact]
    public async Task APermanentlyAbsentDeviceStopsBeingProbedEverySecond()
    {
        var delays = new List<TimeSpan>();
        DelayAsync recording = (d, _) => { delays.Add(d); return Task.CompletedTask; };

        var supervisor = Build(
            () => throw new MystiqueUsbException(UsbFault.Classify(2), "CreateFile"),
            _ => Task.FromResult<ISensorSource>(new FakeSensorSource()),
            () => false,
            delay: recording);

        await RunUntil(supervisor, () => delays.Count >= 60);

        // First 30 attempts stay on the fast schedule so a boot-time arrival is
        // picked up promptly; after that it must back off rather than probe forever.
        Assert.All(delays.Take(30), d => Assert.InRange(d.TotalMilliseconds, 800, 1200));
        Assert.Contains(delays.Skip(30), d => d.TotalSeconds > 3);
    }

    [Fact]
    public async Task PerUpdateLoggingIsNotEmitted()
    {
        var device = new FakeMystiqueDevice();
        var log = new ConsoleLog();
        var supervisor = Build(
            () => device, _ => Task.FromResult<ISensorSource>(new FakeSensorSource()), () => false,
            log: log, options: new SupervisorOptions(Rotation: 0, SummaryEveryUpdates: 300));

        await RunUntil(supervisor, () => device.TelemetryCalls >= 100);

        // 100 frames must not produce 100 lines. State transitions only.
        Assert.True(log.Recent(1000).Count < 20);
    }
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `dotnet test Mystique.slnx -c Release --filter FullyQualifiedName~SupervisorTests`
Expected: FAIL — `CS0246: The type or namespace name 'Supervisor' could not be found`.

- [ ] **Step 4: Write the supervisor**

`MystiqueService/Supervisor.cs`:

```csharp
using Mystique.Core.Contracts;
using Mystique.Core.Device;

namespace MystiqueService;

/// <summary>Injectable so tests run without real time passing.</summary>
public delegate Task DelayAsync(TimeSpan duration, CancellationToken cancellationToken);

public sealed record SupervisorOptions(byte Rotation = 0, int SummaryEveryUpdates = 300);

/// <summary>
/// The state machine that keeps the panel alive. Its contract is simple and
/// absolute: it does not exit. Every failure becomes a state, a log line, and a
/// retry. Nothing here calls Environment.Exit.
/// </summary>
public sealed class Supervisor
{
    private readonly Func<IMystiqueDevice> deviceFactory;
    private readonly Func<CancellationToken, Task<ISensorSource>> sensorFactory;
    private readonly DeepCoolWatcher deepCool;
    private readonly ILog log;
    private readonly TimeProvider time;
    private readonly DelayAsync delay;
    private readonly SupervisorOptions options;

    // Separate counters per subsystem: a chatty sensor bridge must not push USB
    // retries to the cap.
    private readonly BackoffSchedule deviceFast = BackoffSchedule.Fast();
    private readonly BackoffSchedule deviceSlow = BackoffSchedule.Slow();
    private readonly BackoffSchedule sensorFast = BackoffSchedule.Fast();
    private readonly FailureEscalator deviceFailures;
    private readonly FailureEscalator sensorFailures;

    private IMystiqueDevice? device;
    private ISensorSource? sensors;
    private byte rotation;
    private volatile bool reinitRequested;
    private long updatesSent;
    private DateTimeOffset stateSince;

    public Supervisor(
        Func<IMystiqueDevice> deviceFactory,
        Func<CancellationToken, Task<ISensorSource>> sensorFactory,
        DeepCoolWatcher deepCool,
        ILog log,
        TimeProvider time,
        DelayAsync delay,
        SupervisorOptions options)
    {
        this.deviceFactory = deviceFactory;
        this.sensorFactory = sensorFactory;
        this.deepCool = deepCool;
        this.log = log;
        this.time = time;
        this.delay = delay;
        this.options = options;

        deviceFailures = new FailureEscalator(time);
        sensorFailures = new FailureEscalator(time);
        rotation = options.Rotation;
        stateSince = time.GetUtcNow();
        Status = StatusSnapshot.Initial(stateSince, rotation);
    }

    public StatusSnapshot Status { get; private set; }

    public event Action<StatusSnapshot>? StatusChanged;

    public void SetRotation(byte value)
    {
        if (value > 3)
            return;

        rotation = value;
        reinitRequested = true;
        log.Info($"rotation set to {value * 90} degrees; reinitialising");
    }

    public void RequestRetryNow()
    {
        deviceFast.Reset();
        deviceSlow.Reset();
        sensorFast.Reset();
        reinitRequested = true;
        log.Info("manual retry requested");
    }

    public async Task RunAsync(CancellationToken cancellationToken)
    {
        Transition(ServiceState.Starting, "service starting");

        try
        {
            while (!cancellationToken.IsCancellationRequested)
                await StepAsync(cancellationToken);
        }
        catch (OperationCanceledException) when (cancellationToken.IsCancellationRequested)
        {
            // Normal stop.
        }
        finally
        {
            ReleaseDevice("service stopping");
            ReleaseSensors();
            Transition(ServiceState.Stopped, "service stopped");
        }
    }

    private async Task StepAsync(CancellationToken cancellationToken)
    {
        try
        {
            if (await HandleDeepCoolAsync(cancellationToken))
                return;

            if (!await EnsureDeviceAsync(cancellationToken))
                return;

            if (!await EnsureSensorsAsync(cancellationToken))
                return;

            await PumpOneFrameAsync(cancellationToken);
        }
        catch (OperationCanceledException) when (cancellationToken.IsCancellationRequested)
        {
            throw;
        }
        catch (Exception ex)
        {
            // Deliberate catch-all. An unclassified bug must degrade the panel, not
            // terminate a service that is supposed to run from boot to shutdown.
            if (deviceFailures.OnFailure() is { } level)
                log.Write(level, $"unhandled step failure: {ex.GetType().Name}: {ex.Message}");

            ReleaseDevice("unhandled failure");
            Transition(ServiceState.Reconnecting, "recovering from unhandled failure");
            await delay(deviceSlow.NextDelay(), cancellationToken);
        }
    }

    private async Task<bool> HandleDeepCoolAsync(CancellationToken cancellationToken)
    {
        DeepCoolTransition transition = deepCool.Poll();

        if (transition == DeepCoolTransition.Appeared)
        {
            // Release pre-emptively. If we are already gone there is no handle for
            // DeepCool to tear down, and no reset war.
            ReleaseDevice("DeepCool.exe started");
            Transition(ServiceState.Yielded, "yielded to DeepCool.exe");
        }
        else if (transition == DeepCoolTransition.Exited)
        {
            log.Info("DeepCool.exe exited; reacquiring device");
            deviceFast.Reset();
            await delay(DeepCoolWatcher.SettleDelay, cancellationToken);
            return false;
        }

        if (!deepCool.IsPresent)
            return false;

        if (deepCool.ShouldLogHeartbeat())
            log.Info("still yielded to DeepCool.exe");

        await delay(DeepCoolWatcher.PollInterval, cancellationToken);
        return true;
    }

    private async Task<bool> EnsureDeviceAsync(CancellationToken cancellationToken)
    {
        if (device is not null && !reinitRequested)
            return true;

        if (device is not null && reinitRequested)
        {
            reinitRequested = false;
            try
            {
                device.EnterMachineInfoMode(rotation);
                return true;
            }
            catch (Exception ex)
            {
                log.Warn($"reinitialisation failed: {ex.Message}");
                ReleaseDevice("reinitialisation failed");
            }
        }

        try
        {
            IMystiqueDevice opened = deviceFactory();
            try
            {
                // Trap 2: a reopened device is not in Machine Info mode. Writes would
                // be ACKed while the panel showed nothing, so init is part of acquiring.
                opened.EnterMachineInfoMode(rotation);
            }
            catch
            {
                opened.Dispose();
                throw;
            }

            device = opened;
            reinitRequested = false;
            deviceFailures.OnSuccess();
            deviceFast.Reset();
            deviceSlow.Reset();
            log.Info($"device acquired: {opened.RedactedPath}, rotation {rotation * 90} degrees");
            return true;
        }
        catch (MystiqueUsbException ex)
        {
            await HandleDeviceFaultAsync(ex.Fault, ex.Message, cancellationToken);
            return false;
        }
    }

    private async Task HandleDeviceFaultAsync(
        UsbFault fault, string message, CancellationToken cancellationToken)
    {
        switch (fault.Kind)
        {
            case FaultKind.Yield:
                // Not an error and not a failure counter. Either DeepCool has it, or
                // something else does; either way we wait quietly.
                Transition(ServiceState.Yielded, "device owned by another process");
                await delay(WaitDelay(), cancellationToken);
                return;

            case FaultKind.NotPresent:
            case FaultKind.SurpriseRemoval:
                Transition(ServiceState.Waiting, fault.Description);
                if (deviceFailures.OnFailure() is { } waitLevel)
                    log.Write(waitLevel, $"waiting for device: {fault.Description}");
                await delay(WaitDelay(), cancellationToken);
                return;

            default:
                Transition(ServiceState.Reconnecting, fault.Description);
                if (deviceFailures.OnFailure() is { } level)
                    log.Write(level, $"device fault: {message}");
                device?.Recover(fault.Recovery);
                await delay(deviceSlow.NextDelay(), cancellationToken);
                return;
        }
    }

    /// <summary>
    /// Fast while arrival is still plausible, then slow. Without this the fast
    /// schedule's exhaustion is never observed, and a device left unplugged (or a
    /// DeepCool window left open all afternoon) would be probed once a second
    /// forever. This is the polling substitute for CM_Register_Notification; see
    /// "Deviations from the spec".
    /// </summary>
    private TimeSpan WaitDelay() =>
        deviceFast.Exhausted ? deviceSlow.NextDelay() : deviceFast.NextDelay();

    private async Task<bool> EnsureSensorsAsync(CancellationToken cancellationToken)
    {
        if (sensors is not null)
            return true;

        try
        {
            sensors = await sensorFactory(cancellationToken);
            sensorFailures.OnSuccess();
            sensorFast.Reset();
            log.Info("sensor bridge connected");
            return true;
        }
        catch (OperationCanceledException) when (cancellationToken.IsCancellationRequested)
        {
            throw;
        }
        catch (Exception ex)
        {
            // The device stays held: the panel keeps its last frame rather than
            // going blank, and takeover is instant when the bridge returns.
            Transition(ServiceState.Degraded, "sensor bridge unavailable");
            if (sensorFailures.OnFailure() is { } level)
                log.Write(level, $"sensor bridge unavailable: {ex.Message}");
            await delay(sensorFast.NextDelay(), cancellationToken);
            return false;
        }
    }

    private async Task PumpOneFrameAsync(CancellationToken cancellationToken)
    {
        SensorSample sample;
        try
        {
            sample = await sensors!.ReadAsync(cancellationToken);
        }
        catch (OperationCanceledException) when (cancellationToken.IsCancellationRequested)
        {
            throw;
        }
        catch (Exception ex)
        {
            ReleaseSensors();
            Transition(ServiceState.Degraded, "sensor stream ended");
            if (sensorFailures.OnFailure() is { } level)
                log.Write(level, $"sensor read failed: {ex.Message}");
            await delay(sensorFast.NextDelay(), cancellationToken);
            return;
        }

        var frame = TelemetryFrame.FromRaw(
            sample.CpuTemperature, sample.CpuUsage, sample.MemoryLoad,
            sample.GpuTemperature, sample.CpuClock);

        try
        {
            device!.SendTelemetry(frame);
        }
        catch (MystiqueUsbException ex)
        {
            if (ex.Fault.Kind is FaultKind.SurpriseRemoval or FaultKind.NotPresent
                or FaultKind.Yield or FaultKind.Programming)
                ReleaseDevice(ex.Fault.Description);

            await HandleDeviceFaultAsync(ex.Fault, ex.Message, cancellationToken);
            return;
        }

        updatesSent++;
        deviceSlow.RecordSuccess();
        deviceFast.RecordSuccess();
        deviceFailures.OnSuccess();

        Publish(ServiceState.Streaming, "streaming", frame);

        if (options.SummaryEveryUpdates > 0 && updatesSent % options.SummaryEveryUpdates == 0)
            log.Info(
                $"{updatesSent} updates sent; CPU {frame.CpuTemperatureC}C {frame.CpuUsagePercent}%, " +
                $"GPU {frame.GpuTemperatureC}C, RAM {frame.MemoryLoadPercent}%");
    }

    private void ReleaseDevice(string reason)
    {
        if (device is null)
            return;

        try
        {
            device.AbortPendingTransfers();
        }
        catch
        {
            // Best effort: we are giving the handle up regardless.
        }

        device.Dispose();
        device = null;
        log.Info($"device released: {reason}");
    }

    private void ReleaseSensors()
    {
        sensors?.Dispose();
        sensors = null;
    }

    private void Transition(ServiceState state, string detail)
    {
        if (Status.State == state && Status.Detail == detail)
            return;

        DateTimeOffset now = time.GetUtcNow();
        TimeSpan inPrevious = now - stateSince;
        if (Status.State != state)
            log.Info($"{Status.State} -> {state}: {detail} (after {inPrevious.TotalSeconds:F1}s)");

        stateSince = now;
        Publish(state, detail, Status.Last);
    }

    private void Publish(ServiceState state, string detail, TelemetryFrame? frame)
    {
        Status = new StatusSnapshot(state, detail, updatesSent, stateSince, rotation, frame);
        StatusChanged?.Invoke(Status);
    }
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `dotnet test Mystique.slnx -c Release --filter FullyQualifiedName~SupervisorTests`
Expected: PASS, no failures.

- [ ] **Step 6: Run the whole non-hardware suite**

Run: `dotnet test Mystique.slnx -c Release --filter "Category!=Hardware"`
Expected: PASS, no hangs. If any supervisor test hangs, the guard iteration count in `RunUntil` is masking a loop that never makes progress — fix the supervisor, do not raise the guard.

- [ ] **Step 7: Commit**

```bash
git add MystiqueService/Supervisor.cs Mystique.Tests
git commit -m "feat: add supervisor state machine that recovers instead of exiting"
```

---

## Task 13: Service host and console mode

Wraps the supervisor in a `ServiceBase` and gives it a console mode for debugging.

**On stop, send nothing to the LCD.** `WaitToKillServiceTimeout` on this machine is **5000 ms total**. A mode reset is 13 exchanges with sleeps; at a 1500 ms pipe timeout it could exceed 20 s, which guarantees a force-kill *mid-sequence* and leaves the device in an indeterminate state — worse than a known one. The panel is PSU-powered, so at S5 it goes dark anyway and the next boot re-inits.

**Deviation from the spec, recorded deliberately:** the spec suggested opting into `SERVICE_CONTROL_PRESHUTDOWN`. `ServiceBase` cannot accept `SERVICE_ACCEPT_PRESHUTDOWN` — it would require owning `HandlerEx`. Since stop does no device I/O and completes in milliseconds, the generous window buys nothing. `CanShutdown = true` is set (it defaults to **false**, and without it `SERVICE_CONTROL_SHUTDOWN` never arrives at all).

**Files:**
- Create: `MystiqueService/ServiceConfig.cs`, `MystiqueService/MystiqueWindowsService.cs`
- Modify: `MystiqueService/Program.cs` (replace the Task 10 stub)
- Test: `Mystique.Tests/ServiceConfigTests.cs`

**Interfaces:**
- Consumes: `Supervisor`, `SupervisorOptions`, `RollingFileLog`, `ConsoleLog`, `CompositeLog`, `DeepCoolWatcher`, `MystiqueDevice`, `SensorBridgeClient`, `MystiqueCommand`
- Produces:
  - `sealed record ServiceConfig(byte Rotation)` with `static ServiceConfig Load(string path)`, `void Save(string path)`, `static string DefaultPath`
  - `sealed class MystiqueWindowsService : ServiceBase` with `const string WindowsServiceName = "MystiqueService"` (not `ServiceName` — `ServiceBase` already defines that as a settable property)
  - `Program.Main` handling `--service`, `--console`, `install`, `uninstall`

- [ ] **Step 1: Write the failing config tests**

`Mystique.Tests/ServiceConfigTests.cs`:

```csharp
using MystiqueService;

namespace Mystique.Tests;

public sealed class ServiceConfigTests : IDisposable
{
    private readonly string path = Path.Combine(
        Path.GetTempPath(), $"mystique-config-{Guid.NewGuid():N}.json");

    public void Dispose()
    {
        if (File.Exists(path))
            File.Delete(path);
    }

    [Fact]
    public void MissingFileYieldsDefaults()
    {
        ServiceConfig config = ServiceConfig.Load(path);

        Assert.Equal(0, config.Rotation);
    }

    [Fact]
    public void RoundTripsThroughDisk()
    {
        new ServiceConfig(Rotation: 3).Save(path);

        Assert.Equal(3, ServiceConfig.Load(path).Rotation);
    }

    [Fact]
    public void CorruptFileFallsBackToDefaultsRatherThanThrowing()
    {
        File.WriteAllText(path, "{ this is not json");

        Assert.Equal(0, ServiceConfig.Load(path).Rotation);
    }

    [Fact]
    public void OutOfRangeRotationIsClampedOnLoad()
    {
        File.WriteAllText(path, """{ "Rotation": 9 }""");

        Assert.Equal(0, ServiceConfig.Load(path).Rotation);
    }

    [Fact]
    public void SaveCreatesTheParentDirectory()
    {
        string nested = Path.Combine(
            Path.GetTempPath(), $"mystique-cfg-{Guid.NewGuid():N}", "config.json");
        try
        {
            new ServiceConfig(1).Save(nested);
            Assert.True(File.Exists(nested));
        }
        finally
        {
            Directory.Delete(Path.GetDirectoryName(nested)!, recursive: true);
        }
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `dotnet test Mystique.slnx -c Release --filter FullyQualifiedName~ServiceConfigTests`
Expected: FAIL — `CS0246: The type or namespace name 'ServiceConfig' could not be found`.

- [ ] **Step 3: Write the config**

`MystiqueService/ServiceConfig.cs`:

```csharp
using System.Text.Json;

namespace MystiqueService;

/// <summary>
/// Persisted service settings. Deliberately tiny: anything the tray can change
/// must survive a reconnect and a reboot, and nothing else belongs here.
/// </summary>
public sealed record ServiceConfig(byte Rotation = 0)
{
    public static string DefaultPath => Path.Combine(
        Environment.GetFolderPath(Environment.SpecialFolder.CommonApplicationData),
        "Mystique",
        "config.json");

    /// <summary>Never throws. A bad config must not stop the panel from lighting up.</summary>
    public static ServiceConfig Load(string path)
    {
        try
        {
            if (!File.Exists(path))
                return new ServiceConfig();

            ServiceConfig? loaded = JsonSerializer.Deserialize<ServiceConfig>(File.ReadAllText(path));
            if (loaded is null || loaded.Rotation > 3)
                return new ServiceConfig();

            return loaded;
        }
        catch (Exception ex) when (ex is IOException or JsonException or UnauthorizedAccessException)
        {
            return new ServiceConfig();
        }
    }

    public void Save(string path)
    {
        string? directory = Path.GetDirectoryName(path);
        if (!string.IsNullOrEmpty(directory))
            Directory.CreateDirectory(directory);

        File.WriteAllText(path, JsonSerializer.Serialize(
            this, new JsonSerializerOptions { WriteIndented = true }));
    }
}
```

- [ ] **Step 4: Run the config tests to verify they pass**

Run: `dotnet test Mystique.slnx -c Release --filter FullyQualifiedName~ServiceConfigTests`
Expected: PASS, no failures.

- [ ] **Step 5: Write the service host**

`MystiqueService/MystiqueWindowsService.cs`:

```csharp
using System.ServiceProcess;
using Mystique.Core.Contracts;
using Mystique.Core.Device;

namespace MystiqueService;

public sealed class MystiqueWindowsService : ServiceBase
{
    public const string WindowsServiceName = "MystiqueService";

    private readonly ILog log;
    private readonly Supervisor supervisor;
    private readonly CancellationTokenSource stopping = new();
    private Task? worker;

    public MystiqueWindowsService(ILog log, Supervisor supervisor)
    {
        this.log = log;
        this.supervisor = supervisor;

        ServiceName = WindowsServiceName;
        // Defaults to false. Without it SERVICE_CONTROL_SHUTDOWN never arrives and
        // the process is simply killed at shutdown.
        CanShutdown = true;
        CanStop = true;
        CanPauseAndContinue = false;
        AutoLog = false;
    }

    protected override void OnStart(string[] args)
    {
        log.Info($"service starting (pid {Environment.ProcessId}, session {Environment.ProcessId switch { _ => "0" }})");
        worker = Task.Run(() => supervisor.RunAsync(stopping.Token));
    }

    protected override void OnStop() => Shutdown("SCM stop");

    protected override void OnShutdown() => Shutdown("system shutdown");

    protected override void OnCustomCommand(int command)
    {
        // The only untrusted input the service accepts is an integer in this fixed
        // set. There is no payload, no path, and deliberately no raw-packet command.
        if (MystiqueCommand.ToRotation(command) is { } rotation)
        {
            supervisor.SetRotation(rotation);
            new ServiceConfig(rotation).Save(ServiceConfig.DefaultPath);
            return;
        }

        switch (command)
        {
            case MystiqueCommand.RotateNext:
                byte next = (byte)((supervisor.Status.Rotation + 1) % 4);
                supervisor.SetRotation(next);
                new ServiceConfig(next).Save(ServiceConfig.DefaultPath);
                break;

            case MystiqueCommand.RetryNow:
            case MystiqueCommand.Reacquire:
                supervisor.RequestRetryNow();
                break;

            case MystiqueCommand.ReloadConfig:
                supervisor.SetRotation(ServiceConfig.Load(ServiceConfig.DefaultPath).Rotation);
                break;

            default:
                log.Warn($"ignoring unknown custom command {command}");
                break;
        }
    }

    private void Shutdown(string reason)
    {
        log.Info($"stopping: {reason}");
        stopping.Cancel();

        // Nothing is sent to the LCD here. WaitToKillServiceTimeout is 5000 ms and a
        // mode reset is 13 exchanges; attempting one would guarantee a force-kill
        // mid-sequence and leave the device in an indeterminate state.
        try
        {
            worker?.Wait(TimeSpan.FromSeconds(3));
        }
        catch (AggregateException)
        {
            // Cancellation surfaces here; the handle is released either way.
        }

        log.Info("stopped");
    }

    protected override void Dispose(bool disposing)
    {
        if (disposing)
        {
            stopping.Dispose();
            (log as IDisposable)?.Dispose();
        }

        base.Dispose(disposing);
    }
}
```

Delete the odd `Environment.ProcessId switch` expression in `OnStart` if it trips the compiler; the intent is just `log.Info($"service starting (pid {Environment.ProcessId})")`.

- [ ] **Step 6: Write the entry point**

`MystiqueService/Program.cs` — replace the stub:

```csharp
using System.ServiceProcess;
using Mystique.Core.Device;

namespace MystiqueService;

internal static class Program
{
    private const string Usage =
        "Usage: MystiqueService [--service|--console|install|uninstall|status]";

    private static int Main(string[] args)
    {
        // Explicit flag, not Environment.UserInteractive: that property is
        // unreliable on modern Windows and picking the wrong mode fails opaquely.
        string mode = args.Length == 0 ? "--console" : args[0].ToLowerInvariant();

        return mode switch
        {
            "--service" => RunAsService(),
            "--console" => RunInConsole(),
            "install" => ServiceInstaller.Install(),
            "uninstall" => ServiceInstaller.Uninstall(),
            "status" => ServiceInstaller.PrintStatus(),
            _ => Fail(),
        };
    }

    private static int Fail()
    {
        Console.Error.WriteLine(Usage);
        return 2;
    }

    private static int RunAsService()
    {
        // LocalSystem's working directory is system32; make paths unambiguous.
        Directory.SetCurrentDirectory(AppContext.BaseDirectory);

        var log = new RollingFileLog(RollingFileLog.DefaultDirectory, TimeProvider.System);
        var supervisor = BuildSupervisor(log);
        using var service = new MystiqueWindowsService(log, supervisor);
        ServiceBase.Run(service);
        return 0;
    }

    private static int RunInConsole()
    {
        var log = new CompositeLog(
            new RollingFileLog(RollingFileLog.DefaultDirectory, TimeProvider.System),
            new ConsoleLog());
        var supervisor = BuildSupervisor(log);

        using var stopping = new CancellationTokenSource();
        Console.CancelKeyPress += (_, e) =>
        {
            e.Cancel = true;
            stopping.Cancel();
        };

        Console.WriteLine("Running in console mode. Ctrl+C to stop.");
        supervisor.RunAsync(stopping.Token).GetAwaiter().GetResult();
        return 0;
    }

    private static Supervisor BuildSupervisor(ILog log)
    {
        ServiceConfig config = ServiceConfig.Load(ServiceConfig.DefaultPath);

        return new Supervisor(
            deviceFactory: MystiqueDevice.Open,
            sensorFactory: ct => SensorBridgeClient.ConnectAsync(ct)
                .ContinueWith(t => (ISensorSource)t.GetAwaiter().GetResult(), ct,
                    TaskContinuationOptions.ExecuteSynchronously, TaskScheduler.Default),
            deepCool: new DeepCoolWatcher(DeepCoolWatcher.ProcessIsRunning, TimeProvider.System),
            log: log,
            time: TimeProvider.System,
            delay: Task.Delay,
            options: new SupervisorOptions(config.Rotation));
    }
}
```

If the `ContinueWith` cast is awkward under `TreatWarningsAsErrors`, replace `sensorFactory` with a local async function:

```csharp
        static async Task<ISensorSource> ConnectSensorsAsync(CancellationToken ct) =>
            await SensorBridgeClient.ConnectAsync(ct);
```

and pass `ConnectSensorsAsync`.

- [ ] **Step 7: Verify console mode drives the panel**

`ServiceInstaller` does not exist yet, so stub it for this step so the project compiles:

```csharp
namespace MystiqueService;

internal static class ServiceInstaller
{
    internal static int Install() => throw new NotImplementedException("Task 14");
    internal static int Uninstall() => throw new NotImplementedException("Task 14");
    internal static int PrintStatus() => throw new NotImplementedException("Task 14");
}
```

Then:

```powershell
dotnet run --project MystiqueService -c Release -- --console
```

Expected: state transitions logged with timestamps, `Starting -> Streaming`, the panel updating, and **no per-update line**. Let it run 2 minutes and confirm a single summary line appears at 300 updates. Ctrl+C stops cleanly.

- [ ] **Step 8: Verify recovery in console mode**

While it runs, in another terminal:

```powershell
Restart-Service 'Deep Cool Helper Service'
```

Expected: `Streaming -> Degraded`, then `sensor bridge connected`, then `Degraded -> Streaming`, with the process still alive. This is test 2 from the spec's testing plan.

Then launch `C:\DeepCool\DeepCool.exe` and confirm `Streaming -> Yielded` within ~2 s with no ERROR lines, and closing it returns to `Streaming` within ~5 s **with a fresh `device acquired` line** (proving re-init ran). This is test 5.

- [ ] **Step 9: Commit**

```bash
git add MystiqueService Mystique.Tests
git commit -m "feat: host the supervisor in a Windows service with console mode"
```

---

## Task 14: Install, deploy, and verify from a cold boot

The payoff task. After this the LCD is live at the login screen with nobody logged on.

**Deploy to `%ProgramFiles%\Mystique\`, not `bin/Release`.** Windows locks a running image, so a service running from its own build output makes the next `dotnet build` fail.

**No `DependOnService`.** Measured: the Helper Service reports RUNNING at boot+20.0 s but its pipe does not exist until boot+24.2 s. SCM dependencies wait for RUNNING, not for *ready*, so the dependency buys nothing the retry loop must provide anyway — while adding a hard failure if DeepCool renames the service (its key is literally `Deep Cool Helper Service`, and `nssm.exe` ships alongside, so its packaging is not fixed).

**Files:**
- Modify: `MystiqueService/ServiceInstaller.cs` (replace the Task 13 stub)
- Modify: `README.md`

**Interfaces:**
- Consumes: `MystiqueWindowsService.WindowsServiceName`
- Produces: `static int ServiceInstaller.Install()`, `Uninstall()`, `PrintStatus()`, and `static string ServiceInstaller.InstallDirectory`

- [ ] **Step 1: Write the installer**

`MystiqueService/ServiceInstaller.cs`:

```csharp
using System.Diagnostics;
using System.Security.Principal;
using System.ServiceProcess;

namespace MystiqueService;

/// <summary>
/// Registers the service via sc.exe. Chosen over P/Invoking CreateService because
/// every step below is inspectable and reversible with one command by hand.
/// </summary>
internal static class ServiceInstaller
{
    internal static string InstallDirectory => Path.Combine(
        Environment.GetFolderPath(Environment.SpecialFolder.ProgramFiles), "Mystique");

    internal static int Install()
    {
        if (!IsElevated())
        {
            Console.Error.WriteLine("Run from an elevated prompt: registering a service requires admin.");
            return 3;
        }

        string exe = Path.Combine(InstallDirectory, "MystiqueService.exe");
        if (!File.Exists(exe))
        {
            Console.Error.WriteLine($"Not found: {exe}");
            Console.Error.WriteLine(
                $"Publish first: dotnet publish MystiqueService -c Release -o \"{InstallDirectory}\"");
            return 4;
        }

        // SERVICE_AUTO_START, not delayed-auto: delayed would push the first frame
        // out by roughly two minutes.
        Run("create", MystiqueWindowsService.WindowsServiceName,
            $"binPath= \"\\\"{exe}\\\" --service\"", "start= auto",
            "DisplayName= \"Mystique LCD Telemetry\"");

        Run("description", MystiqueWindowsService.WindowsServiceName,
            "\"Drives the DeepCool Mystique LCD with system telemetry from boot.\"");

        // unrestricted gives us NT SERVICE\MystiqueService for ACLs without removing
        // any group SIDs. Never 'restricted' - that adds a write-restricted token and
        // is a realistic way to break WinUSB access.
        Run("sidtype", MystiqueWindowsService.WindowsServiceName, "unrestricted");

        // Crash backstop, distinct from the in-process reconnect loop.
        Run("failure", MystiqueWindowsService.WindowsServiceName,
            "reset= 86400", "actions= restart/5000/restart/10000/restart/30000");

        Run("start", MystiqueWindowsService.WindowsServiceName);

        Console.WriteLine("Installed and started.");
        return PrintStatus();
    }

    internal static int Uninstall()
    {
        if (!IsElevated())
        {
            Console.Error.WriteLine("Run from an elevated prompt.");
            return 3;
        }

        Run("stop", MystiqueWindowsService.WindowsServiceName, allowFailure: true);
        Run("delete", MystiqueWindowsService.WindowsServiceName);
        Console.WriteLine("Uninstalled. Log files under %ProgramData%\\Mystique were left in place.");
        return 0;
    }

    internal static int PrintStatus()
    {
        try
        {
            using var controller = new ServiceController(MystiqueWindowsService.WindowsServiceName);
            Console.WriteLine($"{controller.ServiceName}: {controller.Status}");
            return 0;
        }
        catch (InvalidOperationException)
        {
            Console.WriteLine($"{MystiqueWindowsService.WindowsServiceName}: not installed");
            return 1;
        }
    }

    private static bool IsElevated()
    {
        using var identity = WindowsIdentity.GetCurrent();
        return new WindowsPrincipal(identity).IsInRole(WindowsBuiltInRole.Administrator);
    }

    private static void Run(string verb, params string[] rest) => Run(verb, false, rest);

    private static void Run(string verb, bool allowFailure, params string[] rest)
    {
        var startInfo = new ProcessStartInfo("sc.exe") { UseShellExecute = false };
        startInfo.ArgumentList.Add(verb);
        foreach (string argument in rest)
            startInfo.ArgumentList.Add(argument);

        using Process process = Process.Start(startInfo)
            ?? throw new InvalidOperationException("Could not start sc.exe");
        process.WaitForExit();

        if (process.ExitCode != 0 && !allowFailure)
            throw new InvalidOperationException($"sc.exe {verb} failed with exit code {process.ExitCode}");
    }
}
```

The `Run(verb, allowFailure: true)` call in `Uninstall` binds to the `params` overload; change that call site to `Run("stop", true)` and the compiler resolves it correctly.

- [ ] **Step 2: Build and test**

Run: `dotnet build Mystique.slnx -c Release && dotnet test Mystique.slnx -c Release --filter "Category!=Hardware"`
Expected: clean build, all tests pass.

- [ ] **Step 3: Stop anything holding the device, then publish**

```powershell
Get-Process MystiqueUsbTool,DeepCool -ErrorAction SilentlyContinue | Stop-Process
dotnet publish MystiqueService -c Release -o "$env:ProgramFiles\Mystique"
```

Expected: `MystiqueService.exe` plus its DLLs land in `C:\Program Files\Mystique\`.

- [ ] **Step 4: Install (elevated)**

From an **elevated** PowerShell:

```powershell
& "$env:ProgramFiles\Mystique\MystiqueService.exe" install
```

Expected: `Installed and started.` then `MystiqueService: Running`, and the LCD begins updating within a few seconds.

- [ ] **Step 5: Confirm the service configuration is what the design called for**

```powershell
sc.exe qc MystiqueService
sc.exe qsidtype MystiqueService
sc.exe qfailure MystiqueService
```

Expected: `START_TYPE : 2 AUTO_START`, `SERVICE_START_NAME : LocalSystem`, `DEPENDENCIES` **empty**, `SERVICE_SID_TYPE: UNRESTRICTED`, and three restart actions.

- [ ] **Step 6: Cold-boot verification — the actual goal (spec test 1)**

Reboot. **Do not log in.** Watch the LCD at the login screen.

Expected: telemetry appears at roughly boot + 25 s, gated by USB enumeration rather than the sensor bridge.

After logging in, confirm from the log:

```powershell
Get-Content "$env:ProgramData\Mystique\logs\mystique.log" -Head 40
```

Expected: `Starting -> Waiting` or `Starting -> Streaming`, a `device acquired` line, `sensor bridge connected`, `-> Streaming`. Any `Waiting` entries before that are correct behaviour, not errors — error 2 at boot means "not enumerated yet".

- [ ] **Step 7: Verify the remaining hardware scenarios (spec tests 3, 4, 6)**

```powershell
# 3: unplug and replug the AIO USB cable
# 4: sleep from the Start menu, resume; then let it idle-sleep and resume
# 6: stop the service mid-stream
Measure-Command { Stop-Service MystiqueService }
Start-Service MystiqueService
```

Expected: replug returns to `Streaming` with a fresh `device acquired`; both sleep paths recover without any code depending on `SystemEvents.PowerModeChanged` (which does not fire on Windows 11 for manual Start-menu sleep — dotnet/runtime#78162); `Stop-Service` completes in well under the 5 s budget with no force-kill in the Event Log.

- [ ] **Step 8: Confirm no device identifier reaches the log (spec test 9)**

```powershell
Select-String -Path "$env:ProgramData\Mystique\logs\mystique.log" -Pattern 'AABBCCDD|41 41 42 42 43 43 44 44'
```

Expected: no matches.

- [ ] **Step 9: Rewrite the README**

Replace `README.md` entirely. It must:
- describe the service as the primary way to run, with `install` / `uninstall` / `status`
- keep the firmware-safety warning from the current `README.md:38-43` verbatim in substance — no firmware capability ships
- **remove the absolute `C:\Users\nguyen` paths** at the current `README.md:21`, `:35`, `:36`
- correct the current `README.md:8` claim into something precise: the sensor bridge's ~1.08 s push cadence is the refresh bottleneck, not the Full-Speed USB link
- document `MystiqueUsbTool` as the diagnostic tool, including `--show-device-id`
- state that the two DeepCool services must remain running (they supply the sensors) while `DeepCool.exe` itself must not be running

- [ ] **Step 10: Start a long-run soak**

Leave the service running. The baseline to beat is the current 12 h run of 39,227 updates with zero errors; the target is 72 h+. Check periodically:

```powershell
Select-String -Path "$env:ProgramData\Mystique\logs\mystique.log" -Pattern 'ERROR' | Measure-Object
```

- [ ] **Step 11: Commit — Phase 2 complete**

```bash
git add MystiqueService README.md
git commit -m "feat: install MystiqueService as an auto-start LocalSystem service"
```

---

## Deviations from the spec

Recorded so a reviewer can reject them individually rather than discovering them in the diff.

1. **Device arrival uses bounded polling, not `CM_Register_Notification`.** The spec called for windowless notification via `CM_NOTIFY_FILTER_TYPE_DEVICEINTERFACE`. The supervisor's `Waiting` state already retries on a schedule, so notification would only shorten the detection gap — bounded by one fast-schedule tick (~1 s) while arrival is plausible. Cost avoided: another callback-threading P/Invoke surface where doing USB I/O on the callback thread is a classic deadlock, plus registration/unregistration lifetime across every reconnect. Cost accepted: up to one second of extra latency on replug, and a `CM_Get_Device_Interface_List` call on the retry cadence. `Supervisor.WaitDelay` escalates fast → slow after 30 attempts specifically so an indefinitely absent device settles at a 30 s probe rather than a 1 s one. Add notification later as a pure optimisation if replug latency ever matters.

2. **`SERVICE_CONTROL_PRESHUTDOWN` is not requested.** `ServiceBase` cannot accept it without owning `HandlerEx`. Since stop performs no device I/O, the generous window buys nothing. Detail in Task 13.

3. **Seven states, not six.** The spec's prose says "Six states" and then lists seven. The plan implements all seven listed, including `Stopped`.

4. **A test project exists that the spec's project table does not list.** `Mystique.Tests` carries xUnit; the spec's two-package budget applies to shipping projects only, and this is stated in Global Constraints.

5. **The spec's "no `.git` exists yet — the history starts clean" is no longer true.** The repo is initialised, committed, and pushed. Consequences are in the Phase 4 note below.

## Deferred to later plans

- **Phase 3 (tray):** `MystiqueTray`, the `Mystique.Status.v1` one-way pipe and its DACL, and wiring `ServiceController.ExecuteCommand` to the SCM control codes already handled in `OnCustomCommand`. Spec tests 7 and 8.
- **Phase 4 (publishing):** the repo is already `git init`-ed and **pushed** to `github.com/NguyenPhan2810/Deepcool-AIO-LCD` (currently **private** — anonymous API returns 404). Its history contains `deepcool-worker.js`, `mystique-live.log` with the unredacted device ID, and `bin/`+`obj/` carrying the username. Task 1 untracks them going forward but **does not rewrite history**. Before this repo can go public it needs an orphan-branch rebuild or filter, a force-push, `LICENSE`, and a NOTICE disclaiming any rights in DeepCool's software, firmware, or trademarks.
- **Open items carried forward from the spec:** whether `LocalService` can open the device (would shrink the blast radius substantially); whether `DeepCoolDisplayService` can be set to Manual — it is suspect #1 if a pre-logon conflict ever appears, though 25 h of concurrent running produced zero device-open lines in its logs; and folding `SensorPipeProbe` into a `sensors` verb so the reverse-engineered handshake is not maintained in two places.
