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
