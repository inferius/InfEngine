//! W448 - natural weather cycle, ported from MC
//! `ServerLevel.advanceWeatherCycle` (26.1.2): precipitation runs
//! for a random 12000..24000 ticks (10-20 min) and clear gaps last
//! 12000..180000 ticks (10-150 min). Before this the sky only ever
//! changed via the F4 debug key / `/weather` (the R31 parity
//! question). The host (or offline player) owns the timer and
//! broadcasts flips over the existing `vx.weather` message.
//!
//! W460: the second, independent MC thunder timer landed
//! (thundering 3600+rand(12000) ticks, clear-of-thunder
//! 12000+rand(168000)); lightning gates on `isThundering &&
//! isRaining` like MC, and both cycles persist in world.json (the
//! level.dat rainTime/thunderTime family). Remaining honest gap:
//! the precipitation FORM is chosen once per flip from the biome
//! at the host player's column (MC resolves rain vs snow per
//! column at render/tick time).

use crate::{NetRole, Voxel};

/// Roll how long the CURRENT state lasts, in seconds. MC:
/// precipitation 12000 + rand(12000) ticks, clear 12000 +
/// rand(168000) ticks.
pub(crate) fn roll_duration_secs(rng: &mut u32, precipitating: bool) -> f32 {
    let mut x = *rng;
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    *rng = x;
    let r = (x as f32 / u32::MAX as f32).clamp(0.0, 1.0);
    if precipitating {
        600.0 + r * 600.0
    } else {
        600.0 + r * 8400.0
    }
}

/// Advance the cycle. Clients never run it - the host's flips
/// arrive over `vx.weather` like the debug key's always have.
/// W460: how long the NEW thunder state lasts, in seconds. MC
/// `advanceWeatherCycle`: entering thunder 3600 + rand(12000)
/// ticks (180..780 s), leaving 12000 + rand(168000) ticks
/// (600..9000 s).
pub(crate) fn roll_thunder_secs(rng: &mut u32, thundering: bool) -> f32 {
    let mut x = *rng;
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    *rng = x;
    let r = (x as f32 / u32::MAX as f32).clamp(0.0, 1.0);
    if thundering {
        180.0 + r * 600.0
    } else {
        600.0 + r * 8400.0
    }
}

pub(crate) fn tick_weather(vx: &mut Voxel, dt: f32) {
    if matches!(vx.net, NetRole::Client { .. }) {
        return;
    }
    if vx.weather_rng == 0 {
        // First tick: seed the stream and roll the opening stretch
        // of clear sky (+ the independent thunder gap, W460).
        vx.weather_rng = 0x9E37_79B1 ^ (vx.net_tick as u32) | 1;
        let precip = vx.weather != 0;
        vx.weather_ticks_left =
            roll_duration_secs(&mut vx.weather_rng, precip);
        vx.thunder_ticks_left =
            roll_thunder_secs(&mut vx.weather_rng, vx.thundering);
        return;
    }
    // W460: the thunder countdown runs independently of the rain
    // one, exactly like MC's second timer. Thunder only SHOWS
    // (lightning machine) while it also precipitates.
    vx.thunder_ticks_left -= dt;
    if vx.thunder_ticks_left <= 0.0 {
        vx.thundering = !vx.thundering;
        vx.thunder_ticks_left =
            roll_thunder_secs(&mut vx.weather_rng, vx.thundering);
        broadcast_thunder(vx);
    }
    vx.weather_ticks_left -= dt;
    if vx.weather_ticks_left > 0.0 {
        return;
    }
    let new_weather = if vx.weather == 0 {
        // Precipitation starts: rain, or snow when the host player's
        // column is cold enough (approximates MC's per-column form).
        let cold = matches!(
            vx.current_worldgen_ctx().profile,
            crate::worldgen::WorldGen::Mc
        ) && {
            let ctx = vx.current_worldgen_ctx();
            let px = vx.player_pos.x.floor() as i32;
            let py = vx.player_pos.y.floor() as i32;
            let pz = vx.player_pos.z.floor() as i32;
            let mc = crate::worldgen::mcgen::terrain::mc_full_biome_at(
                &ctx, px, pz,
            );
            crate::worldgen::mcgen::vegetation::cold_enough_to_snow_at(
                mc, px, py, pz,
            )
        };
        if cold { 2 } else { 1 }
    } else {
        0
    };
    set_weather(vx, new_weather);
}

/// Apply a weather state + re-roll its natural duration + broadcast.
/// The F4 debug key and `/weather` route here too, so a manual
/// override lasts one natural stretch instead of being flipped away
/// on the very next timer expiry.
pub(crate) fn set_weather(vx: &mut Voxel, weather: u8) {
    vx.weather = weather;
    if vx.weather_rng == 0 {
        vx.weather_rng = 0x9E37_79B1;
    }
    vx.weather_ticks_left =
        roll_duration_secs(&mut vx.weather_rng, weather != 0);
    if let NetRole::Server { server, .. } = &mut vx.net {
        let cfg = bincode::config::standard();
        if let Ok(buf) = bincode::serde::encode_to_vec(vx.weather, cfg) {
            server.broadcast_bytes("vx.weather", buf);
        }
    }
}

/// W460: ship the thunder flag to clients (their lightning machine
/// gates on it like the host's).
pub(crate) fn broadcast_thunder(vx: &mut Voxel) {
    if let NetRole::Server { server, .. } = &mut vx.net {
        let cfg = bincode::config::standard();
        if let Ok(buf) = bincode::serde::encode_to_vec(vx.thundering, cfg)
        {
            server.broadcast_bytes("vx.thunder", buf);
        }
    }
}
