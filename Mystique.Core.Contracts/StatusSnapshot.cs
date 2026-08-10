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
