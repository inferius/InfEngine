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
}
