using System;
using System.Runtime.InteropServices;

namespace InfEngine.Scripts;

/// <summary>
/// Sample game logic driving the player entity. The host calls
/// <see cref="OnTick"/> every frame; this script ignores any keyboard
/// input the host already applied and instead drives the player on a
/// gentle figure-eight using the host clock.
/// </summary>
public static class Player
{
    private static float _accumulated;
    private static int _frame;

    [UnmanagedCallersOnly(CallConvs = new[] { typeof(System.Runtime.CompilerServices.CallConvCdecl) })]
    public static void OnTick(float dt)
    {
        _accumulated += dt;
        _frame++;

        // Figure-eight in world units. World axes match the renderer's
        // 1 unit = 1 logical pixel mapping, so 200 px radius is fine.
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
