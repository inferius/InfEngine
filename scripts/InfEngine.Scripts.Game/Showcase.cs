using System;
using InfEngine.Scripts.Host;

namespace InfEngine.Scripts.Game;

/// <summary>
/// Showcase project's game-side logic. Lives in the hot-reloadable
/// game assembly alongside <see cref="Player"/>; rebuilding this
/// file and triggering the host's reload entry point swaps the
/// behaviour live without restarting the runtime.
///
/// Methods carrying <see cref="BlueprintCallableAttribute"/> are
/// the ones the blueprint editor will surface as nodes once the
/// reflection-driven manifest pipeline lands (see
/// <c>docs/design/blueprint_csharp_callable.md</c>). Today the
/// blueprint's <c>call_cs_method</c> node logs the call through
/// the stub host; once the reverse-call wiring is in place every
/// method below becomes directly invokable from a blueprint
/// without any extra glue.
/// </summary>
public static class Showcase
{
    private static int _pickupsCollected;
    private static float _lastAnnounce;

    /// <summary>
    /// Hot-reloadable scene tick. The host calls this every frame
    /// while the showcase scene is active. Mirrors
    /// <see cref="Player.OnTick"/>; the dispatcher uses the method
    /// name to resolve the entry point.
    /// </summary>
    public static void OnTick(float dt)
    {
        _lastAnnounce += dt;
        if (_lastAnnounce >= 5f)
        {
            _lastAnnounce = 0f;
            Engine.Log(
                $"[Showcase.cs] tick - {_pickupsCollected} pickup(s) collected so far");
        }
    }

    /// <summary>
    /// Increments the showcase's pickup counter. Designed to be
    /// called from a Pickup entity's blueprint when the player
    /// overlaps it.
    /// </summary>
    [BlueprintCallable(
        DisplayName = "Pickup Collected",
        Category = "Showcase",
        Tooltip = "Bump the global pickup counter and announce the score."
    )]
    public static void OnPickupCollected()
    {
        _pickupsCollected++;
        Engine.Log($"[Showcase.cs] Pickup collected! Score = {_pickupsCollected}");
    }

    /// <summary>
    /// Returns the running pickup count. Used by the scene-level
    /// blueprint to gate a "Goal reached" branch once enough
    /// pickups are claimed.
    /// </summary>
    [BlueprintCallable(
        DisplayName = "Get Pickup Count",
        Category = "Showcase",
        Tooltip = "Total pickups collected since Play started."
    )]
    public static float GetPickupCount()
    {
        return _pickupsCollected;
    }

    /// <summary>
    /// Resets the pickup counter. Hooked up to OnStart of the
    /// scene blueprint so reloading Play restarts the score.
    /// </summary>
    [BlueprintCallable(
        DisplayName = "Reset Score",
        Category = "Showcase"
    )]
    public static void ResetScore()
    {
        _pickupsCollected = 0;
        _lastAnnounce = 0f;
        Engine.Log("[Showcase.cs] score reset to 0");
    }
}
