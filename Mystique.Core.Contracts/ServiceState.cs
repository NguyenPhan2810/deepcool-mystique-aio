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

    /// <summary>Device not present at all. Waiting for it to arrive.</summary>
    Waiting,

    /// <summary>DeepCool.exe is running. We hold no handle by choice.</summary>
    Yielded,

    /// <summary>Gave up or was stopped. Never entered silently.</summary>
    Stopped,
}
