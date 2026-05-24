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
    use crate::block;
    use crate::net::NetRole;
    vx.chests
        .entry(pos)
        .or_insert_with(block::chest::new_chest);
    vx.chest_open = Some(pos);
    if let NetRole::Client { client, .. } = &mut vx.net {
        let cfg = bincode::config::standard();
        let msg = crate::net::ChestOpenMsg { pos };
        if let Ok(buf) = bincode::serde::encode_to_vec(msg, cfg) {
            let _ = client.send_bytes("vx.chest_open", buf);
        }
    }
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

/// W47.13i: RMB on a bed. Checks the day phase: outside night the
/// player gets a chat refusal; at night offline/host bumps the day
/// clock forward, client ships a `vx.sleep` request to the host.
pub(crate) fn try_sleep_bed(vx: &mut crate::Voxel) {
    use crate::DAY_LENGTH_S;
    use crate::net::NetRole;
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
