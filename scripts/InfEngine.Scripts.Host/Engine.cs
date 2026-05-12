using System;
using System.Runtime.InteropServices;
using System.Text;

namespace InfEngine.Scripts.Host;

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
    /// <summary>
    /// Broadcast / send a named network event with a single float
    /// payload. Routes through the host's active <c>NetSession</c>:
    /// servers broadcast to all clients, clients send to the server.
    /// No-op when networking is offline.
    /// </summary>
    public delegate* unmanaged[Cdecl]<byte*, nuint, float, void> NetSendEvent;
    /// <summary>1 = host runs as networking server, 0 otherwise.</summary>
    public delegate* unmanaged[Cdecl]<byte> NetIsServer;
}

/// <summary>
/// Static facade game scripts call into. Lives in the host assembly
/// (default ALC) so reloading the game assembly does not lose the API.
/// </summary>
public static unsafe class Engine
{
    private static EngineApiPtrs _api;

    [UnmanagedCallersOnly(CallConvs = new[] { typeof(System.Runtime.CompilerServices.CallConvCdecl) })]
    public static void RegisterApi(EngineApiPtrs* api)
    {
        _api = *api;
        Log("[CS] engine API registered");
    }

    [BlueprintCallable(
        DisplayName = "Log Message",
        Category = "Debug",
        Tooltip = "Send a string to the engine's tracing log."
    )]
    public static void Log(string message)
    {
        if (_api.LogInfo == null)
        {
            Console.Error.WriteLine("[CS] Engine API not registered: " + message);
            return;
        }
        var bytes = Encoding.UTF8.GetBytes(message);
        fixed (byte* ptr = bytes)
        {
            _api.LogInfo(ptr, (nuint)bytes.Length);
        }
    }

    [BlueprintCallable(
        DisplayName = "Time Seconds",
        Category = "Time",
        Tooltip = "Wall-clock seconds since the host started."
    )]
    public static float TimeSeconds()
    {
        if (_api.TimeSecs == null) return 0f;
        return _api.TimeSecs();
    }

    public static (float X, float Y) GetPlayerPos()
    {
        if (_api.GetPlayerPos == null) return (0f, 0f);
        float x = 0f, y = 0f;
        _api.GetPlayerPos(&x, &y);
        return (x, y);
    }

    public static void SetPlayerPos(float x, float y)
    {
        if (_api.SetPlayerPos == null) return;
        _api.SetPlayerPos(x, y);
    }

    /// <summary>
    /// Send a named network event with a float payload through the
    /// engine's active <c>NetSession</c>. Server broadcasts to every
    /// connected client; client sends to its server. No-op when
    /// networking is offline.
    /// </summary>
    [BlueprintCallable(
        DisplayName = "Net Send Event",
        Category = "Network",
        Tooltip = "Broadcast (server) or send-to-server (client) a named event."
    )]
    public static void NetSendEvent(string eventName, float payload)
    {
        if (_api.NetSendEvent == null) return;
        var bytes = Encoding.UTF8.GetBytes(eventName);
        fixed (byte* ptr = bytes)
        {
            _api.NetSendEvent(ptr, (nuint)bytes.Length, payload);
        }
    }

    /// <summary>True when the engine's active session is the
    /// networking authority (server). Always false in offline /
    /// single-player play.</summary>
    [BlueprintCallable(
        DisplayName = "Is Server",
        Category = "Network",
        Tooltip = "True on the networking server; false on clients / offline."
    )]
    public static bool NetIsServer()
    {
        if (_api.NetIsServer == null) return false;
        return _api.NetIsServer() != 0;
    }
}
