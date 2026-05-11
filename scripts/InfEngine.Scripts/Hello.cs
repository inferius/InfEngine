using System;
using System.Runtime.InteropServices;

namespace InfEngine.Scripts;

/// <summary>
/// Entry-point demos for the Phase 8 C# scripting host.
///
/// Methods marked with <see cref="UnmanagedCallersOnlyAttribute"/> are
/// loaded from Rust via <c>get_function_with_unmanaged_callers_only</c>
/// in the <c>netcorehost</c> crate.
/// </summary>
public static class Hello
{
    /// <summary>Prints a greeting and exits.</summary>
    [UnmanagedCallersOnly]
    public static void HelloFromCSharp()
    {
        Console.WriteLine($"[InfEngine] Hello from C# {Environment.Version}!");
    }

    /// <summary>Returns 42 to the caller so the marshalling path is verified.</summary>
    [UnmanagedCallersOnly]
    public static int LifeUniverseEverything()
    {
        return 42;
    }
}
