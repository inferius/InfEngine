//! W123 - world ambience audio: underwater enter/exit/loop,
//! rare cave stings, and lazy-loaded background music.
//!
//! MC mirror:
//! - `ambient.underwater.enter` / `.exit` fire on the submerge
//!   boundary; `ambient.underwater.loop` runs while the camera is
//!   under the surface (`net/minecraft/client/sounds` ambient
//!   handlers).
//! - `ambient.cave` is the "mood" sound: it plays rarely when the
//!   player lingers in dark underground spaces.
//! - `music.game` / `music.creative` rotate on a multi-minute gap
//!   (MC waits 5-20 min between tracks).
//!
//! The rain segment loop lives in `update_play` next to the
//! lightning state machine (it needs the weather gate there).

use crate::Voxel;
use crate::game_mode::GameMode;
use crate::worldgen::climate::SEA_LEVEL;
use infengine_voxel::CHUNK_SIZE;

/// Seconds between `ambient.underwater.loop` refires while
/// submerged. The loop file is a long bed; a slight overlap at the
/// boundary blends fine.
const UNDERWATER_LOOP_PERIOD: f32 = 11.0;

/// Cave-sting re-roll window (seconds). MC's mood algorithm lands
/// around one sting per few minutes of dark-cave time.
const CAVE_MIN: f32 = 120.0;
const CAVE_SPAN: f32 = 240.0;

/// Gap between background-music tracks (seconds). MC uses 5-20
/// minutes; we sit at the friendly end so the feature is audible.
const MUSIC_MIN: f32 = 300.0;
const MUSIC_SPAN: f32 = 480.0;

/// Linear gain for background music (under the SFX bed).
const MUSIC_GAIN: f32 = 0.35;

/// Per-frame ambience tick. Call once per update while playing.
pub(crate) fn tick(vx: &mut Voxel, dt: f32) {
    let eye = vx.player_pos;
    let eye_voxel = [
        eye.x.floor() as i32,
        eye.y.floor() as i32,
        eye.z.floor() as i32,
    ];
    let pos = [eye.x, eye.y, eye.z];

    // --- Underwater enter / exit / loop -------------------------
    // W203: waterlogged plant cells (kelp/seagrass) count as water -
    // swimming through a kelp forest keeps the underwater ambience.
    let underwater = crate::block::block_is_watery(vx.world.get(eye_voxel).0);
    if underwater != vx.underwater_was {
        let key = if underwater {
            "ambient/underwater_enter"
        } else {
            "ambient/underwater_exit"
        };
        vx.sfx_at(key, pos, |_, _| {});
        vx.underwater_was = underwater;
        vx.underwater_sfx_accum = 0.0;
    }
    if underwater {
        vx.underwater_sfx_accum -= dt;
        if vx.underwater_sfx_accum <= 0.0 {
            vx.underwater_sfx_accum = UNDERWATER_LOOP_PERIOD;
            vx.sfx_at("ambient/underwater", pos, |_, _| {});
        }
    }

    // --- Cave mood sting -----------------------------------------
    // Eligible while the eye sits in a sky-light-0 cell below sea
    // level (a dark underground space). The timer only counts down
    // while eligible, so surface time doesn't bank mood.
    if eye_voxel[1] < SEA_LEVEL && sky_light_at(vx, eye_voxel) == 0 {
        vx.cave_sfx_in -= dt;
        if vx.cave_sfx_in <= 0.0 {
            vx.cave_sfx_in = CAVE_MIN + rand01(vx.net_tick, 0x5EED_CA7E) * CAVE_SPAN;
            vx.sfx_at("ambient/cave", pos, |_, _| {});
        }
    }

    // --- Background music ----------------------------------------
    vx.music_in -= dt;
    if vx.music_in <= 0.0 {
        vx.music_in = MUSIC_MIN + rand01(vx.net_tick, 0x0D15_EA5E) * MUSIC_SPAN;
        play_next_music_track(vx);
    }
}

/// Sky-light nibble at a world voxel (15 = full sky). Missing
/// chunk light defaults to full sky so caves never false-positive
/// while streaming.
fn sky_light_at(vx: &Voxel, world: [i32; 3]) -> u8 {
    let cs = CHUNK_SIZE as i32;
    let cc = [
        world[0].div_euclid(cs),
        world[1].div_euclid(cs),
        world[2].div_euclid(cs),
    ];
    let lc = [
        world[0].rem_euclid(cs) as u32,
        world[1].rem_euclid(cs) as u32,
        world[2].rem_euclid(cs) as u32,
    ];
    match vx.lighting.get(cc) {
        Some(g) => g.get_sky(lc),
        None => 15,
    }
}

/// Cheap deterministic 0..1 roll seeded off the net tick.
fn rand01(tick: u64, salt: u32) -> f32 {
    let mut x = (tick as u32).wrapping_mul(0x9E37_79B9) ^ salt;
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    (x as f32 / u32::MAX as f32).clamp(0.0, 1.0)
}

/// Lazy music rotation: resolve the mode's MC music event, pick one
/// variant, decode it into the bank under the `"music"` key
/// (replacing the previous track so only one stays resident), play
/// it non-spatially at a low gain.
fn play_next_music_track(vx: &mut Voxel) {
    let engine_key = match vx.game_mode {
        GameMode::Creative => "music/creative",
        _ => "music/game",
    };
    let variants = vx.resource_pack.sound_paths(engine_key);
    if variants.is_empty() {
        return;
    }
    let pick = (rand01(vx.net_tick, 0xBEA7_5EED) * variants.len() as f32) as usize;
    let path = variants[pick.min(variants.len() - 1)].clone();
    let Some(audio) = vx.audio.as_mut() else {
        return;
    };
    // Decode just this one track; the insert replaces the previous
    // track's samples so long sessions don't accumulate memory.
    let n = vx.audio_bank.load_variants(audio, "music", &[path]);
    if n == 0 {
        return;
    }
    vx.audio_bank.play(audio, "music", 0, MUSIC_GAIN);
    tracing::info!(key = engine_key, "background music track started");
}
