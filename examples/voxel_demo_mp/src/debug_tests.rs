//! Q.3 regression tests: the `/debug feathers` toggle + the
//! `tick_chicken_feather_lay` gate that fronts the periodic feather
//! pickup loop.
//!
//! The free-fn gate (`crate::tick_chicken_feather_lay`) returns the
//! "should pickup now?" decision without touching `Voxel`, so these
//! tests can drive thousands of simulated ticks without a renderer
//! or net role. The pre-Q.3 bug fired one feather per ~12 in-world
//! seconds while flying mid-air; this regression suite locks the
//! gate so the loop stays off by default.

use crate::mob_meshes::tick_chicken_feather_lay;

const TICK_DT: f32 = 1.0 / 60.0;

/// Drive 1000 ticks at 60 Hz (~16.7 s, more than one nominal lay
/// window) and count "should pickup" hits.
fn run_n_ticks(
    debug_enabled: bool,
    host_role: bool,
    chicken_alive: bool,
    ticks: usize,
) -> u32 {
    let mut accum = 0.0;
    let mut fired = 0u32;
    for _ in 0..ticks {
        if tick_chicken_feather_lay(
            debug_enabled,
            host_role,
            chicken_alive,
            &mut accum,
            TICK_DT,
        ) {
            fired += 1;
        }
    }
    fired
}

#[test]
fn debug_off_never_fires_even_with_chicken_on_host() {
    // The pre-Q.3 bug: feathers spawned even when the player was
    // mid-air with no input. Default state must keep the loop
    // entirely silent.
    let fired = run_n_ticks(false, true, true, 1000);
    assert_eq!(fired, 0, "loop must stay off by default");
}

#[test]
fn debug_on_fires_when_host_and_chicken_present() {
    // Sanity: the loop CAN still fire when explicitly enabled, so
    // the debug toggle isn't a no-op. 1000 ticks at 1/60 s =
    // 16.66 s, which crosses the 12 s threshold once.
    let fired = run_n_ticks(true, true, true, 1000);
    assert_eq!(fired, 1, "exactly one feather per 12 s window");
}

#[test]
fn debug_on_no_host_role_does_not_fire() {
    // Clients must never run the host-side loop, even when the
    // debug toggle is on - prevents a client setting from forging
    // server-side state.
    let fired = run_n_ticks(true, false, true, 1000);
    assert_eq!(fired, 0);
}

#[test]
fn debug_on_no_chicken_does_not_fire() {
    // The original loop only laid feathers while a chicken was
    // alive; the gate keeps that guard so the toggle isn't a way
    // to mint feathers without livestock.
    let fired = run_n_ticks(true, true, false, 1000);
    assert_eq!(fired, 0);
}

#[test]
fn flipping_debug_off_resets_accumulator() {
    // The toggle drains the accumulator on every gated-off frame.
    // Run the loop to within 0.5 s of firing, then flip off for a
    // tick; flip back on and confirm we don't immediately fire on
    // the next tick (i.e. the accumulator restarts from zero).
    let mut accum = 0.0;
    // Pump 11.0 seconds with the toggle on so we're just under
    // the 12.0 s threshold.
    for _ in 0..(11.0 / TICK_DT) as usize {
        let _ = tick_chicken_feather_lay(true, true, true, &mut accum, TICK_DT);
    }
    assert!((10.9..12.0).contains(&accum));
    // One frame with toggle off resets the accumulator.
    let fired = tick_chicken_feather_lay(false, true, true, &mut accum, TICK_DT);
    assert!(!fired);
    assert_eq!(accum, 0.0);
    // Pump 0.1 seconds with toggle on - the accumulator is back
    // near 0 so no fire.
    for _ in 0..(0.1 / TICK_DT) as usize {
        let _ = tick_chicken_feather_lay(true, true, true, &mut accum, TICK_DT);
    }
    assert!(accum < 1.0);
}

#[test]
fn long_run_with_loop_off_does_not_accumulate_inventory() {
    // 6000 ticks (~100 s) at 60 Hz, loop off: no pickup fires.
    // Stand-in for the user complaint "feathers spawn in
    // inventory while flying for a minute".
    let fired = run_n_ticks(false, true, true, 6000);
    assert_eq!(fired, 0);
}

// ---------------------------------------------------------------------------
// W324 - fell-through-world-after-load regression pins.
// ---------------------------------------------------------------------------

/// Standing on a snow layer keeps the feet FRACTIONALLY inside the
/// snow cell (partial-height block). That must NOT read as buried:
/// the old feet-level probe did, and the load-time fallback then
/// teleported the player to the spawn column outside the generated
/// ring (the fall-through chain).
#[test]
fn w324_standing_on_snow_layer_is_not_embedded() {
    let mut v = crate::Voxel::new();
    // World.set never auto-creates chunks - seed empty ones first.
    for cx in -1..=0 {
        for cz in -1..=0 {
            v.world
                .insert_chunk([cx, 4, cz], infengine_voxel::Chunk::filled(infengine_voxel::BlockId(0)));
        }
    }
    // Ground at y=72, snow layer cell at y=73.
    for x in -1..=1 {
        for z in -1..=1 {
            v.world.set([x, 72, z], infengine_voxel::BlockId(1));
            v.world.set([x, 73, z], infengine_voxel::BlockId(15));
        }
    }
    // Saved head-height pose of a player standing on the layer:
    // feet at 73.01 (inside the snow cell), head at 74.71.
    let pos = glam::Vec3::new(0.5, 74.71, 0.5);
    assert!(
        !v.capsule_embedded(pos),
        "feet fractionally inside a snow layer must not count as buried",
    );
}

/// A player whose head sits inside solid blocks IS embedded - the
/// eye-level probe must keep detecting the genuinely buried case
/// (that is what the spawn fallback exists for).
#[test]
fn w324_fully_buried_player_is_embedded() {
    let mut v = crate::Voxel::new();
    for cy in 3..=5 {
        v.world
            .insert_chunk([0, cy, 0], infengine_voxel::Chunk::filled(infengine_voxel::BlockId(0)));
    }
    for y in 60..=80 {
        v.world.set([0, y, 0], infengine_voxel::BlockId(96));
    }
    let pos = glam::Vec3::new(0.5, 74.71, 0.5);
    assert!(
        v.capsule_embedded(pos),
        "a head inside deepslate must still read as buried",
    );
}
