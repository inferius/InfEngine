using System;
using InfEngine.Scripts.Host;

namespace InfEngine.Scripts.Game;

/// <summary>
/// Hot-reloadable game logic. The host loads this assembly into a
/// collectible <see cref="System.Runtime.Loader.AssemblyLoadContext"/>
/// and resolves <see cref="OnTick"/> via reflection - so the method
/// must stay a plain `public static void method(float)`. Editing this
/// file, rebuilding, and calling the host's reload entry point swaps
/// the implementation without restarting the runtime.
/// </summary>
public static class Player
{
    private static float _accumulated;
    private static int _frame;

    public static void OnTick(float dt)
    {
        _accumulated += dt;
        _frame++;

        // Figure-eight in world units.
        var t = _accumulated;
        float x = MathF.Sin(t) * 200f;
        float y = MathF.Sin(t * 2f) * 120f;
        Engine.SetPlayerPos(x, y);

        if (_frame % 120 == 0)
        {
            var (px, py) = Engine.GetPlayerPos();
            Engine.Log($"OnTick frame={_frame} t={t:F2} pos=({px:F1}, {py:F1})");
        }
    }
}
