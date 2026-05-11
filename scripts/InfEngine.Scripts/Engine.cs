using System;
using System.Runtime.InteropServices;
using System.Text;

namespace InfEngine.Scripts;

/// <summary>
/// FFI-stable struct mirroring <c>infengine_scripting::EngineApi</c>.
/// Field order must match the Rust side exactly.
/// </summary>
[StructLayout(LayoutKind.Sequential)]
public unsafe struct EngineApiPtrs
{
    public delegate* unmanaged[Cdecl]<byte*, nuint, void> LogInfo;
    public delegate* unmanaged[Cdecl]<float> TimeSecs;
    public delegate* unmanaged[Cdecl]<float*, float*, void> GetPlayerPos;
    public delegate* unmanaged[Cdecl]<float, float, void> SetPlayerPos;
}

/// <summary>
/// Static facade that managed code uses to talk back to the host.
/// Call <see cref="RegisterApi"/> once at startup before invoking any
/// other helper - it stores the function-pointer table.
/// </summary>
public static unsafe class Engine
{
    private static EngineApiPtrs _api;

    /// <summary>Host bootstrap: copy the API table into the managed side.</summary>
    [UnmanagedCallersOnly(CallConvs = new[] { typeof(System.Runtime.CompilerServices.CallConvCdecl) })]
    public static void RegisterApi(EngineApiPtrs* api)
    {
        _api = *api;
        Log("[CS] engine API registered");
    }

    /// <summary>Send a UTF-8 log line back to the host's tracing pipeline.</summary>
    public static void Log(string message)
    {
        if (_api.LogInfo == null)
        {
            Console.Error.WriteLine("[CS] Engine API not registered; falling back to stdout: " + message);
            return;
        }
        var bytes = Encoding.UTF8.GetBytes(message);
        fixed (byte* ptr = bytes)
        {
            _api.LogInfo(ptr, (nuint)bytes.Length);
        }
    }

    /// <summary>Seconds since the host's scripting subsystem started.</summary>
    public static float TimeSeconds()
    {
        if (_api.TimeSecs == null) return 0f;
        return _api.TimeSecs();
    }

    /// <summary>Read the player's current world-space position.</summary>
    public static (float X, float Y) GetPlayerPos()
    {
        if (_api.GetPlayerPos == null) return (0f, 0f);
        float x = 0f, y = 0f;
        _api.GetPlayerPos(&x, &y);
        return (x, y);
    }

    /// <summary>Override the player's world-space position.</summary>
    public static void SetPlayerPos(float x, float y)
    {
        if (_api.SetPlayerPos == null) return;
        _api.SetPlayerPos(x, y);
    }
}
