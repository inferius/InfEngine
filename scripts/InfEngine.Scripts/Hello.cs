using System;
using System.Runtime.InteropServices;

namespace InfEngine.Scripts;

/// <summary>
/// Entry-point demos for the Phase 8 C# scripting host.
/// </summary>
public static class Hello
{
    /// <summary>Prints a greeting and exits.</summary>
    [UnmanagedCallersOnly(CallConvs = new[] { typeof(System.Runtime.CompilerServices.CallConvCdecl) })]
    public static void HelloFromCSharp()
    {
        Console.WriteLine($"[InfEngine] Hello from C# {Environment.Version}!");
    }

    /// <summary>Returns 42 to the caller so the marshalling path is verified.</summary>
    [UnmanagedCallersOnly(CallConvs = new[] { typeof(System.Runtime.CompilerServices.CallConvCdecl) })]
    public static int LifeUniverseEverything()
    {
        return 42;
    }

    /// <summary>
    /// Uses the host-provided Engine API: logs back through Rust's tracing
    /// pipeline and reads the host clock.
    /// </summary>
    [UnmanagedCallersOnly(CallConvs = new[] { typeof(System.Runtime.CompilerServices.CallConvCdecl) })]
    public static void EngineRoundTrip()
    {
        Engine.Log($"runtime version = {Environment.Version}");
        Engine.Log($"host time = {Engine.TimeSeconds():F3} s");
    }
}
