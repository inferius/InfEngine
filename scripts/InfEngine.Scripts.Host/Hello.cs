using System;
using System.Runtime.InteropServices;

namespace InfEngine.Scripts.Host;

/// <summary>
/// Smoke-test entry points kept from Phase 8.1 - host-side.
/// </summary>
public static class Hello
{
    [UnmanagedCallersOnly(CallConvs = new[] { typeof(System.Runtime.CompilerServices.CallConvCdecl) })]
    public static void HelloFromCSharp()
    {
        Console.WriteLine($"[InfEngine] Hello from C# {Environment.Version}!");
    }

    [UnmanagedCallersOnly(CallConvs = new[] { typeof(System.Runtime.CompilerServices.CallConvCdecl) })]
    public static int LifeUniverseEverything() => 42;

    [UnmanagedCallersOnly(CallConvs = new[] { typeof(System.Runtime.CompilerServices.CallConvCdecl) })]
    public static void EngineRoundTrip()
    {
        Engine.Log($"runtime version = {Environment.Version}");
        Engine.Log($"host time = {Engine.TimeSeconds():F3} s");
    }
}
