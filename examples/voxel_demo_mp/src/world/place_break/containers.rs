//! W47.14 - container / sleep submodule (split from W47.13 monolith).
//!
//! Three small RMB UI-opener helpers (W47.13i):
//! - `open_furnace` - mint container + runtime entry, mark
//!   furnace_open, notify host on Client
//! - `open_chest` - mint container, mark chest_open, notify host
//!   on Client
//! - `try_sleep_bed` - day-phase gated night skip (host bumps the
//!   clock, client ships a vx.sleep request)

/// W47.13i: RMB on a furnace. Mints the container + runtime entry
/// if this is the first open, marks `furnace_open`, and tells the
/// host (when running as Client) so it can broadcast state
/// snapshots. Clears the pending mouse flags so the open click
/// doesn't replay as an edit.
pub(crate) fn open_furnace(vx: &mut crate::Voxel, pos: [i32; 3]) {
    use crate::block;
    use crate::net::NetRole;
    block::furnace::place_furnace(
        &mut vx.furnaces,
        &mut vx.furnace_runtimes,
        pos,
    );
    vx.furnace_open = Some(pos);
    if let NetRole::Client { client, .. } = &mut vx.net {
        let cfg = bincode::config::standard();
        let msg = crate::net::FurnaceOpenMsg { pos };
        if let Ok(buf) = bincode::serde::encode_to_vec(msg, cfg) {
            let _ = client.send_bytes("vx.furnace_open", buf);
        }
    }
    vx.lmb_pending = false;
    vx.rmb_pending = false;
}

/// W47.13i: RMB on a chest. Mints an empty container if this is
/// the first open, marks `chest_open`, and asks the host for the
/// authoritative state when running as Client. Pending mouse flags
/// cleared so the click doesn't replay against the world.
pub(crate) fn open_chest(vx: &mut crate::Voxel, pos: [i32; 3]) {
    use crate::net::NetRole;
    // W472: a double chest opens as ONE 54-slot menu keyed by the
    // canonical PRIMARY half (both halves minted so the pairing
    // check passes on freshly-streamed clients too).
    let (primary, _) = crate::net::container_apply::chest_canonical(vx, pos);
    vx.chest_open = Some(primary);
    // W126: MC chest-open creak (real OGG; silent without the pack).
    let spos = [pos[0] as f32 + 0.5, pos[1] as f32 + 0.5, pos[2] as f32 + 0.5];
    vx.sfx_at("container/chest/open", spos, |_, _| {});
    if let NetRole::Client { client, .. } = &mut vx.net {
        let cfg = bincode::config::standard();
        let msg = crate::net::ChestOpenMsg { pos: primary };
        if let Ok(buf) = bincode::serde::encode_to_vec(msg, cfg) {
            let _ = client.send_bytes("vx.chest_open", buf);
        }
    }
    vx.lmb_pending = false;
    vx.rmb_pending = false;
}

/// W50.5: RMB on a brewing_stand block. Mints an empty container
/// + runtime entry if missing, marks `brewing_open`. Closes any
///   other open container UI for consistency with chest / furnace.
pub(crate) fn open_brewing_stand(vx: &mut crate::Voxel, pos: [i32; 3]) {
    use crate::block;
    block::brewing::place_brewing_stand(
        &mut vx.brewing_stands,
        &mut vx.brewing_runtimes,
        pos,
    );
    vx.brewing_open = Some(pos);
    vx.chest_open = None;
    vx.furnace_open = None;
    vx.crafting_table_open = None;
    vx.lmb_pending = false;
    vx.rmb_pending = false;
}

/// W50.3: RMB on a crafting_table block. Marks the table open and
/// clears any other open UI marker so the 3x3 panel doesn't fight
/// with the chest / furnace panels. The 3x3 grid + held stack live
/// directly on Voxel (client-side only - crafting tables aren't a
/// shared server resource).
pub(crate) fn open_crafting_table(vx: &mut crate::Voxel, pos: [i32; 3]) {
    vx.crafting_table_open = Some(pos);
    vx.chest_open = None;
    vx.furnace_open = None;
    vx.lmb_pending = false;
    vx.rmb_pending = false;
}

/// W171: RMB on a stonecutter. Marks it open + clears the other panel
/// markers. Client-side only (a cut is a pure inventory transform, no
/// shared server state), like the crafting table.
pub(crate) fn open_stonecutter(vx: &mut crate::Voxel, pos: [i32; 3]) {
    vx.stonecutter_open = Some(pos);
    vx.chest_open = None;
    vx.furnace_open = None;
    vx.crafting_table_open = None;
    vx.brewing_open = None;
    vx.smithing_open = None;
    vx.lmb_pending = false;
    vx.rmb_pending = false;
}

/// W410: RMB on a smithing table. Client-side session UI like the
/// stonecutter - the upgrade is a pure inventory transform.
pub(crate) fn open_smithing_table(vx: &mut crate::Voxel, pos: [i32; 3]) {
    vx.smithing_open = Some(pos);
    vx.chest_open = None;
    vx.furnace_open = None;
    vx.crafting_table_open = None;
    vx.brewing_open = None;
    vx.stonecutter_open = None;
    vx.lmb_pending = false;
    vx.rmb_pending = false;
}

/// W164: RMB on a note block. Cycles the per-position pitch one step
/// (MC's 25 steps, F#3..F#5) and plays the note. MC tunes the pitch
/// on right-click and plays the resulting tone; the instrument by the
/// block beneath + the note particle are documented follow-ups. The
/// note block is a tonal instrument, so the synth `play_beep` tone is
/// a faithful stand-in when the pack ships no OGG.
/// MC note-block pitch -> tone frequency. Pitch 0 = F#3 (~185 Hz),
/// one semitone per step, pitch 24 = F#5 (~740 Hz).
pub(crate) fn note_block_freq(pitch: u8) -> f32 {
    185.0_f32 * 2.0_f32.powf(pitch as f32 / 12.0)
}

pub(crate) fn play_note_block(vx: &mut crate::Voxel, pos: [i32; 3]) {
    let entry = vx.note_pitches.entry(pos).or_insert(0);
    *entry = (*entry + 1) % 25;
    let freq = note_block_freq(*entry);
    let spos = [pos[0] as f32 + 0.5, pos[1] as f32 + 0.5, pos[2] as f32 + 0.5];
    vx.sfx_at("noteblock/harp", spos, move |a, g| {
        let _ = a.play_beep(freq, 0.5, (g * 0.5).clamp(0.0, 1.0));
    });
    vx.lmb_pending = false;
    vx.rmb_pending = false;
}

/// W47.13i: RMB on a bed. Checks the day phase: outside night the
/// player gets a chat refusal; at night offline/host bumps the day
/// clock forward, client ships a `vx.sleep` request to the host.
pub(crate) fn try_sleep_bed(vx: &mut crate::Voxel) {
    use crate::DAY_LENGTH_S;
    use crate::net::NetRole;
    // W366: clicking a bed sets the respawn point whatever the time
    // of day (MC: "Respawn point set" even when sleeping fails).
    // Spawn floats above the bed cell; safe_spawn settles on death.
    if let Some(hit) = vx.target
        && crate::block::is_bed_block(vx.world.get(hit.voxel).0)
    {
        let p = hit.voxel;
        let new_spawn = [p[0] as f32 + 0.5, p[1] as f32 + 2.6, p[2] as f32 + 0.5];
        if vx.spawn_pos != Some(new_spawn) {
            vx.spawn_pos = Some(new_spawn);
            vx.chat_log.push((
                std::time::Instant::now(),
                "Respawn point set".to_string(),
            ));
        }
    }
    let phase = (vx.day_time / DAY_LENGTH_S).rem_euclid(1.0);
    let elev = (phase * std::f32::consts::TAU).sin();
    let is_night = elev < -0.05;
    if !is_night {
        vx.chat_log.push((
            std::time::Instant::now(),
            "You can only sleep at night.".to_string(),
        ));
        return;
    }
    match &mut vx.net {
        NetRole::Offline | NetRole::Server { .. } => {
            vx.day_time = DAY_LENGTH_S * 0.20;
            vx.last_day_broadcast = None;
            // W479: sleeping resets the MC TIME_SINCE_REST stat -
            // the phantom insomnia clock starts over.
            vx.ticks_since_rest = 0;
            vx.chat_log.push((
                std::time::Instant::now(),
                "* sleeps through the night".to_string(),
            ));
        }
        NetRole::Client { client, .. } => {
            let _ = client.send_bytes("vx.sleep", Vec::new());
            vx.chat_log.push((
                std::time::Instant::now(),
                "Asking host to skip the night...".to_string(),
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::note_block_freq;

    /// W164: note-block pitch spans exactly two octaves (F#3..F#5),
    /// with the octave doubling at +12 semitones.
    #[test]
    fn note_block_freq_spans_two_octaves() {
        let lo = note_block_freq(0);
        let mid = note_block_freq(12);
        let hi = note_block_freq(24);
        assert!((lo - 185.0).abs() < 0.5, "pitch 0 ~= 185 Hz, got {lo}");
        assert!((mid - 370.0).abs() < 1.0, "pitch 12 = one octave up, got {mid}");
        assert!((hi - 740.0).abs() < 2.0, "pitch 24 = two octaves up, got {hi}");
    }
}
