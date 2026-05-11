using System;
using System.IO;
using System.Linq;
using System.Reflection;
using System.Runtime.Loader;
using System.Text;

namespace InfEngine.Scripts.Host;

/// <summary>
/// Collectible ALC that falls back to the default ALC for already-loaded
/// shared assemblies (e.g. `InfEngine.Scripts.Host`). Without the
/// fallback, the game assembly's reference to the host would fail to
/// resolve once the host lives outside the collectible ALC.
/// </summary>
internal sealed class GameLoadContext : AssemblyLoadContext
{
    private static readonly AssemblyLoadContext _hostAlc =
        GetLoadContext(typeof(HotLoader).Assembly)!;

    public GameLoadContext() : base("InfEngineGame", isCollectible: true) { }

    protected override Assembly? Load(AssemblyName assemblyName)
    {
        // Resolve shared references (Engine, EngineApiPtrs, ...) to the
        // host assembly already loaded by the hostfxr-managed ALC. If
        // we returned null here, the runtime would look in Default
        // and miss the host.
        var hit = _hostAlc.Assemblies
            .FirstOrDefault(a => a.GetName().Name == assemblyName.Name);
        return hit;
    }
}

/// <summary>
/// Loads the game assembly into a collectible <see cref="AssemblyLoadContext"/>
/// and exposes a stable <c>Tick(float)</c> entry point that forwards into
/// the currently-loaded game. Unload + Load lets the host hot-swap game
/// logic without restarting the runtime.
/// </summary>
public static class HotLoader
{
    private static AssemblyLoadContext? _alc;
    private static Action<float>? _tickDelegate;

    /// <summary>Load (or reload) the game assembly from <paramref name="assemblyPath"/>.</summary>
    [System.Runtime.InteropServices.UnmanagedCallersOnly(
        CallConvs = new[] { typeof(System.Runtime.CompilerServices.CallConvCdecl) })]
    public static unsafe byte LoadGame(byte* pathBytes, nuint pathLen)
    {
        try
        {
            UnloadInternal();
            string assemblyPath = Encoding.UTF8.GetString(pathBytes, (int)pathLen);
            if (!File.Exists(assemblyPath))
            {
                Engine.Log("[hot] game assembly missing: " + assemblyPath);
                return 0;
            }

            var alc = new GameLoadContext();
            // Load via byte stream so the file is not locked by the host
            // (lets the user overwrite the .dll while the runtime is up).
            using var fs = new FileStream(assemblyPath, FileMode.Open, FileAccess.Read, FileShare.Read);
            var asm = alc.LoadFromStream(fs);
            _alc = alc;

            var playerType = asm.GetType("InfEngine.Scripts.Game.Player", throwOnError: false);
            if (playerType == null)
            {
                Engine.Log("[hot] type InfEngine.Scripts.Game.Player not found");
                UnloadInternal();
                return 0;
            }
            var method = playerType.GetMethod(
                "OnTick",
                BindingFlags.Public | BindingFlags.Static,
                binder: null,
                types: new[] { typeof(float) },
                modifiers: null);
            if (method == null)
            {
                Engine.Log("[hot] Player.OnTick(float) not found");
                UnloadInternal();
                return 0;
            }
            _tickDelegate = (Action<float>)Delegate.CreateDelegate(typeof(Action<float>), method);
            Engine.Log("[hot] game assembly loaded: " + assemblyPath);
            return 1;
        }
        catch (Exception ex)
        {
            Engine.Log("[hot] LoadGame failed: " + ex.Message);
            return 0;
        }
    }

    /// <summary>Drop the current game assembly and let the GC collect the ALC.</summary>
    [System.Runtime.InteropServices.UnmanagedCallersOnly(
        CallConvs = new[] { typeof(System.Runtime.CompilerServices.CallConvCdecl) })]
    public static void UnloadGame() => UnloadInternal();

    private static void UnloadInternal()
    {
        if (_alc == null) return;
        _tickDelegate = null;
        _alc.Unload();
        _alc = null;
        // Encourage prompt collection; collectible ALCs only unload once
        // all references are gone.
        GC.Collect();
        GC.WaitForPendingFinalizers();
        GC.Collect();
    }

    /// <summary>Per-frame entry the Rust host calls; forwards to the loaded game.</summary>
    [System.Runtime.InteropServices.UnmanagedCallersOnly(
        CallConvs = new[] { typeof(System.Runtime.CompilerServices.CallConvCdecl) })]
    public static void Tick(float dt) => _tickDelegate?.Invoke(dt);
}
