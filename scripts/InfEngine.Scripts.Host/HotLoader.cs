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
    // [BlueprintCallable] methods harvested from the loaded game
    // assembly. Keyed by "ClassName.MethodName" so the Rust side can
    // look them up by the same name it stored in the blueprint's
    // params["method"]. Built fresh on every LoadGame so renames /
    // additions to the game source pick up after a rebuild + reload.
    private static System.Collections.Generic.Dictionary<string, MethodInfo> _bpMethods =
        new();

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

            // Harvest every [BlueprintCallable] static method from the
            // loaded game assembly. Keyed by "Class.Method" so the
            // editor's scan-driven blueprint nodes resolve to the
            // actual MethodInfo.
            _bpMethods = new System.Collections.Generic.Dictionary<string, MethodInfo>();
            foreach (var t in asm.GetTypes())
            {
                foreach (var m in t.GetMethods(BindingFlags.Public | BindingFlags.Static))
                {
                    if (m.GetCustomAttribute<BlueprintCallableAttribute>() != null)
                    {
                        var key = t.Name + "." + m.Name;
                        _bpMethods[key] = m;
                    }
                }
            }
            Engine.Log(
                "[hot] game assembly loaded: " + assemblyPath
                + " (" + _bpMethods.Count + " [BlueprintCallable] methods)");
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
        _bpMethods = new System.Collections.Generic.Dictionary<string, MethodInfo>();
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

    /// <summary>
    /// Reverse-call dispatcher for blueprints. Rust pushes a UTF-8
    /// method name ("Class.Method") and a destination slot for an
    /// optional float return. Returns:
    ///   0 - method invoked, no return value (void).
    ///   1 - method invoked, return value written to *outResult.
    ///  -1 - method not found / invocation threw.
    /// v1 supports parameter-less methods returning void or float;
    /// arg marshalling lands in a follow-up. Methods with other
    /// shapes return -1 so the blueprint VM keeps running.
    /// </summary>
    [System.Runtime.InteropServices.UnmanagedCallersOnly(
        CallConvs = new[] { typeof(System.Runtime.CompilerServices.CallConvCdecl) })]
    public static unsafe int InvokeBlueprintMethod(byte* namePtr, nuint nameLen, float* outResult)
    {
        try
        {
            if (namePtr == null || nameLen == 0)
            {
                return -1;
            }
            var key = Encoding.UTF8.GetString(namePtr, (int)nameLen);
            if (!_bpMethods.TryGetValue(key, out var method))
            {
                Engine.Log("[hot] InvokeBlueprintMethod: unknown method " + key);
                return -1;
            }
            // v1 ignores parameters; reject any signature with required args.
            var pars = method.GetParameters();
            if (pars.Length > 0)
            {
                Engine.Log("[hot] " + key + ": parameters not yet marshalled (v1 limit)");
                return -1;
            }
            object? ret = method.Invoke(null, null);
            if (method.ReturnType == typeof(void))
            {
                return 0;
            }
            if (method.ReturnType == typeof(float))
            {
                if (outResult != null && ret is float f)
                {
                    *outResult = f;
                }
                return 1;
            }
            if (method.ReturnType == typeof(int))
            {
                if (outResult != null && ret is int i)
                {
                    *outResult = i;
                }
                return 1;
            }
            // Unknown return type - call still happened, but caller
            // can't read it. Treat as void.
            return 0;
        }
        catch (Exception ex)
        {
            Engine.Log("[hot] InvokeBlueprintMethod threw: " + ex.Message);
            return -1;
        }
    }
}
