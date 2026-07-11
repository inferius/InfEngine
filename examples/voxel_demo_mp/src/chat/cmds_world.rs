//! World-flavoured slash commands: time/weather, world seed +
//! generator, block editing (set/fill/explode), render distance,
//! diagnostic dumps about the loaded world.
//!
//! Each fn takes `(voxel, arg, lines)` and returns whether the
//! command was handled. Most commands set the bool explicitly
//! because they may early-out on validation failure without
//! pushing lines.

use glam::Vec3;
use infengine_voxel::{BlockId, CHUNK_SIZE, ChunkCoord};

use crate::Voxel;
use crate::block::BLOCK_TYPES;
use crate::net::{EditMsg, NetRole};
use crate::{DAY_LENGTH_S, VERTICAL_CHUNKS, WorldGen};

/// `/day`. Snap day_time to mid-morning.
pub(crate) fn cmd_day(
    voxel: &mut Voxel,
    _arg: &str,
    lines: &mut Vec<String>,
) -> bool {
    // Snap day_time to mid-morning.
    voxel.day_time = DAY_LENGTH_S * 0.25;
    lines.push("Time set to morning.".to_string());
    true
}

/// `/night`. Snap day_time to midnight.
pub(crate) fn cmd_night(
    voxel: &mut Voxel,
    _arg: &str,
    lines: &mut Vec<String>,
) -> bool {
    // Snap day_time to midnight.
    voxel.day_time = DAY_LENGTH_S * 0.75;
    lines.push("Time set to night.".to_string());
    true
}

/// `/time [hh]`. Print the current 24h clock or jump to a value.
pub(crate) fn cmd_time(
    voxel: &mut Voxel,
    arg: &str,
    lines: &mut Vec<String>,
) -> bool {
    // /time <hh>: jump to a specific
    // 24-hour mark. Accepts 0..24, wraps
    // anything else.
    if arg.is_empty() {
        let frac =
            (voxel.day_time / DAY_LENGTH_S).rem_euclid(1.0);
        let hours = frac * 24.0;
        let hh = hours.floor() as i32;
        let mm = ((hours - hh as f32) * 60.0) as i32;
        lines.push(format!("Current time: {hh:02}:{mm:02}"));
    } else if let Ok(hour) = arg.parse::<f32>() {
        let h = hour.rem_euclid(24.0);
        voxel.day_time = DAY_LENGTH_S * (h / 24.0);
        lines.push(format!("Time set to {:.1}h.", h));
    } else {
        lines.push(format!("Cannot parse hour: {arg}"));
    }
    true
}

/// `/sleep`. Skip to morning if it's actually night and we own
/// the clock (host or offline).
pub(crate) fn cmd_sleep(
    voxel: &mut Voxel,
    _arg: &str,
    lines: &mut Vec<String>,
) -> bool {
    // Skip the night: if the sun is below
    // the horizon, fast-forward day_time
    // to morning. Daytime use is a no-op
    // (matches the MC bed rule). Host-only
    // in MP so the night gets skipped for
    // every connected player via the
    // existing day_time broadcast.
    let phase = (voxel.day_time / DAY_LENGTH_S).rem_euclid(1.0);
    let raw_elev = (phase * std::f32::consts::TAU).sin();
    let is_night = raw_elev < -0.05;
    if !is_night {
        lines.push("You can only sleep at night.".to_string());
    } else if matches!(voxel.net, NetRole::Client { .. }) {
        lines.push("Only the host can advance time.".to_string());
    } else {
        voxel.day_time = DAY_LENGTH_S * 0.20;
        voxel.last_day_broadcast = None; // force resend
        lines.push("You slept until morning.".to_string());
    }
    true
}

/// `/weather clear|rain|snow`. Local + (host) broadcast.
pub(crate) fn cmd_weather(
    voxel: &mut Voxel,
    arg: &str,
    lines: &mut Vec<String>,
) -> bool {
    // /weather clear | rain. Sets the
    // local state immediately. If we're
    // hosting, also broadcast a vx.weather
    // packet so connected clients flip
    // their visuals to match.
    let kind = match arg.to_lowercase().as_str() {
        "rain" => Some(1u8),
        "snow" => Some(2u8),
        "clear" | "" => Some(0u8),
        _ => None,
    };
    match kind {
        Some(k) => {
            voxel.weather = k;
            if let NetRole::Server { server, .. } = &mut voxel.net
                && let Ok(buf) = bincode::serde::encode_to_vec(
                    k,
                    bincode::config::standard(),
                )
            {
                server.broadcast_bytes("vx.weather", buf);
            }
            lines.push(format!(
                "Weather: {}",
                match k {
                    1 => "rain",
                    2 => "snow",
                    _ => "clear",
                }
            ));
        }
        None => {
            lines.push("Usage: /weather clear|rain".to_string());
        }
    }
    true
}

/// `/seed [n]`. Read or (host) regenerate with a new seed.
pub(crate) fn cmd_seed(
    voxel: &mut Voxel,
    arg: &str,
    lines: &mut Vec<String>,
) -> bool {
    // /seed prints the active value;
    // /seed <n> (host-only) re-rolls the
    // world. Adopting a new seed drops
    // every generated chunk + broadcasts
    // vx.seed so connected clients follow
    // suit on the next net tick.
    if arg.is_empty() {
        // W5.1: report from voxel state, not the deleted atomic.
        let seed = voxel.current_worldgen_ctx().effective_seed();
        lines.push(format!("World seed: {seed}"));
    } else if !matches!(
        voxel.net,
        NetRole::Server { .. } | NetRole::Offline
    ) {
        lines.push("/seed <n> is host-only.".to_string());
    } else if let Ok(new_seed) = arg.parse::<u32>() {
        // W5.1: write the per-world base_seed; worldgen ctx is
        // re-derived each call from this field + the active dim.
        voxel.base_seed = new_seed;
        let to_unload: Vec<ChunkCoord> =
            voxel.world.iter_chunks().map(|(c, _)| *c).collect();
        if !to_unload.is_empty() {
            let renderer =
                voxel.renderer.as_mut().expect("renderer ready");
            for (_cc, subs) in voxel.chunk_meshes.drain() {
                for (_block, handle) in subs {
                    renderer.remove_mesh(handle);
                }
            }
            for cc in &to_unload {
                voxel.world.remove_chunk(*cc);
            }
        }
        voxel.pending_chunks.clear();
        voxel.pending_set.clear();
        voxel.in_flight_chunks.clear();
        if let NetRole::Server { server, .. } = &mut voxel.net
            && let Ok(buf) = bincode::serde::encode_to_vec(
                new_seed,
                bincode::config::standard(),
            )
        {
            server.broadcast_bytes("vx.seed", buf);
        }
        lines.push(format!(
            "World seed set to {new_seed}; chunks regenerating."
        ));
    } else {
        lines.push(format!("Cannot parse seed: {arg}"));
    }
    true
}

/// `/setblock <x> <y> <z> <block>`. Place a single block via
/// the edit pipeline.
pub(crate) fn cmd_setblock(
    voxel: &mut Voxel,
    arg: &str,
    lines: &mut Vec<String>,
) -> bool {
    // /setblock x y z block: place a
    // block at exact world coords. Routes
    // through the normal edit path so the
    // change persists in the edit history
    // + replicates to peers. "air"
    // clears the cell.
    let parts: Vec<&str> = arg.split_whitespace().collect();
    if parts.len() != 4 {
        lines.push(
            "Usage: /setblock <x> <y> <z> <block>".to_string(),
        );
        return true;
    }
    let x = parts[0].parse::<i32>();
    let y = parts[1].parse::<i32>();
    let z = parts[2].parse::<i32>();
    let block_id: Option<u16> =
        if parts[3].eq_ignore_ascii_case("air") {
            Some(0)
        } else {
            BLOCK_TYPES
                .iter()
                .find(|bt| {
                    bt.name.eq_ignore_ascii_case(parts[3])
                })
                .map(|bt| bt.id)
        };
    match (x, y, z, block_id) {
        (Ok(x), Ok(y), Ok(z), Some(bid)) => {
            let coord = [x, y, z];
            let block = BlockId(bid);
            let edit = EditMsg { x, y, z, block: bid };
            voxel.world_set_tracked(coord, block);
            let coords = Voxel::affected_chunks(coord);
            voxel.rebuild_chunks(&coords);
            let cfg = bincode::config::standard();
            match &mut voxel.net {
                NetRole::Offline => {}
                NetRole::Server { server, .. } => {
                    voxel.edit_history.push(edit);
                    if let Ok(buf) =
                        bincode::serde::encode_to_vec(edit, cfg)
                    {
                        server.broadcast_bytes("vx.edit", buf);
                    }
                }
                NetRole::Client { pending_edits, .. } => {
                    pending_edits.push(edit);
                }
            }
            lines.push(format!(
                "Set ({x},{y},{z}) = {}",
                parts[3]
            ));
        }
        _ => {
            lines.push(
                "Bad args. Coords must be ints + block must exist."
                    .to_string(),
            );
        }
    }
    true
}

/// `/fill x1 y1 z1 x2 y2 z2 block`. Box-fill (host-only, capped
/// at 64^3 voxels).
pub(crate) fn cmd_fill(
    voxel: &mut Voxel,
    arg: &str,
    lines: &mut Vec<String>,
) -> bool {
    // Host-only: fill a box defined by
    // (x1 y1 z1 x2 y2 z2 block) with the
    // named block. "air" clears. Routes
    // every cell through the edit
    // pipeline so the change persists +
    // replicates. Hard cap at 64^3
    // voxels per call.
    if !matches!(
        voxel.net,
        NetRole::Server { .. } | NetRole::Offline
    ) {
        lines.push("/fill is host-only.".to_string());
        return true;
    }
    let parts: Vec<&str> = arg.split_whitespace().collect();
    if parts.len() != 7 {
        lines.push(
            "Usage: /fill x1 y1 z1 x2 y2 z2 block".to_string(),
        );
        return true;
    }
    if let (Ok(x1), Ok(y1), Ok(z1), Ok(x2), Ok(y2), Ok(z2)) = (
        parts[0].parse::<i32>(),
        parts[1].parse::<i32>(),
        parts[2].parse::<i32>(),
        parts[3].parse::<i32>(),
        parts[4].parse::<i32>(),
        parts[5].parse::<i32>(),
    ) {
        let block_id: Option<u16> =
            if parts[6].eq_ignore_ascii_case("air") {
                Some(0)
            } else {
                BLOCK_TYPES
                    .iter()
                    .find(|bt| {
                        bt.name.eq_ignore_ascii_case(parts[6])
                    })
                    .map(|bt| bt.id)
            };
        let mn = [x1.min(x2), y1.min(y2), z1.min(z2)];
        let mx = [x1.max(x2), y1.max(y2), z1.max(z2)];
        let span = [
            (mx[0] - mn[0]).unsigned_abs() as u64 + 1,
            (mx[1] - mn[1]).unsigned_abs() as u64 + 1,
            (mx[2] - mn[2]).unsigned_abs() as u64 + 1,
        ];
        let total = span[0] * span[1] * span[2];
        let bid_opt = block_id;
        if bid_opt.is_none() {
            lines.push(format!(
                "Unknown block: {}",
                parts[6]
            ));
        } else if total > 64 * 64 * 64 {
            lines.push(
                "Too large: cap is 64^3 voxels.".to_string(),
            );
        } else if let Some(bid) = bid_opt {
            let block = BlockId(bid);
            let cfg = bincode::config::standard();
            let mut count = 0u64;
            for y in mn[1]..=mx[1] {
                for z in mn[2]..=mx[2] {
                    for x in mn[0]..=mx[0] {
                        let v = [x, y, z];
                        voxel.world_set_tracked(v, block);
                        count += 1;
                        let edit = EditMsg {
                            x,
                            y,
                            z,
                            block: bid,
                        };
                        if let NetRole::Server { server, .. } =
                            &mut voxel.net
                        {
                            voxel.edit_history.push(edit);
                            if let Ok(buf) =
                                bincode::serde::encode_to_vec(
                                    edit, cfg,
                                )
                            {
                                server.broadcast_bytes(
                                    "vx.edit", buf,
                                );
                            }
                        }
                    }
                }
            }
            // Remesh all touched
            // chunks at once.
            let cs = CHUNK_SIZE as i32;
            let min_cc = [
                mn[0].div_euclid(cs),
                mn[1].div_euclid(cs),
                mn[2].div_euclid(cs),
            ];
            let max_cc = [
                mx[0].div_euclid(cs),
                mx[1].div_euclid(cs),
                mx[2].div_euclid(cs),
            ];
            let mut coords: Vec<ChunkCoord> = Vec::new();
            for cz in min_cc[2]..=max_cc[2] {
                for cy in min_cc[1]..=max_cc[1] {
                    for cx in min_cc[0]..=max_cc[0] {
                        coords.push([cx, cy, cz]);
                    }
                }
            }
            voxel.rebuild_chunks(&coords);
            lines.push(format!("Filled {count} blocks."));
        }
    } else {
        lines.push("Coords must be ints.".to_string());
    }
    true
}

/// `/explode [radius]`. Blast a sphere of voxels around the
/// player to air (host-only).
pub(crate) fn cmd_explode(
    voxel: &mut Voxel,
    arg: &str,
    lines: &mut Vec<String>,
) -> bool {
    // Host-only: blast a sphere of voxels
    // to air around the player. Default
    // radius 4. Routes every cleared cell
    // through the normal edit pipeline so
    // the change replicates + saves.
    if !matches!(
        voxel.net,
        NetRole::Server { .. } | NetRole::Offline
    ) {
        lines.push("/explode is host-only.".to_string());
        return true;
    }
    let radius: i32 =
        arg.parse().ok().unwrap_or(4i32).clamp(1, 16);
    let center = [
        voxel.player_pos.x.floor() as i32,
        voxel.player_pos.y.floor() as i32,
        voxel.player_pos.z.floor() as i32,
    ];
    let mut cleared = 0u32;
    let r2 = radius * radius;
    let cfg = bincode::config::standard();
    for dy in -radius..=radius {
        for dz in -radius..=radius {
            for dx in -radius..=radius {
                if dx * dx + dy * dy + dz * dz > r2 {
                    continue;
                }
                let v = [
                    center[0] + dx,
                    center[1] + dy,
                    center[2] + dz,
                ];
                if v[1] < 1 {
                    // Skip the bedrock
                    // floor so the
                    // crater doesn't
                    // open into the
                    // void.
                    continue;
                }
                if voxel.world.get(v).0 == 0 {
                    continue;
                }
                voxel.world_set_tracked(v, BlockId::AIR);
                cleared += 1;
                let edit = EditMsg {
                    x: v[0],
                    y: v[1],
                    z: v[2],
                    block: 0,
                };
                if let NetRole::Server { server, .. } =
                    &mut voxel.net
                {
                    voxel.edit_history.push(edit);
                    if let Ok(buf) =
                        bincode::serde::encode_to_vec(edit, cfg)
                    {
                        server.broadcast_bytes("vx.edit", buf);
                    }
                }
            }
        }
    }
    // Remesh every chunk the
    // explosion bounding box touches
    // (plus its 1-cell halo).
    let cs = CHUNK_SIZE as i32;
    let min_cc = [
        (center[0] - radius - 1).div_euclid(cs),
        (center[1] - radius - 1).div_euclid(cs),
        (center[2] - radius - 1).div_euclid(cs),
    ];
    let max_cc = [
        (center[0] + radius + 1).div_euclid(cs),
        (center[1] + radius + 1).div_euclid(cs),
        (center[2] + radius + 1).div_euclid(cs),
    ];
    let mut coords: Vec<ChunkCoord> = Vec::new();
    for cz in min_cc[2]..=max_cc[2] {
        for cy in min_cc[1]..=max_cc[1] {
            for cx in min_cc[0]..=max_cc[0] {
                coords.push([cx, cy, cz]);
            }
        }
    }
    voxel.rebuild_chunks(&coords);
    lines.push(format!(
        "Exploded {cleared} blocks (r={radius})."
    ));
    true
}

/// `/rd` (alias `/renderdistance`). Read or set view distance
/// (2..=12).
pub(crate) fn cmd_rd(
    voxel: &mut Voxel,
    arg: &str,
    lines: &mut Vec<String>,
) -> bool {
    // Without args: report current view
    // distance. With one arg (2..=VIEW_DISTANCE_MAX):
    // set it. Routes through the same
    // settings field the Settings panel
    // slider drives, so the change takes
    // effect on the next stream pass. W198:
    // cap raised from 12 to the engine max
    // (32) - it already matched the GUI
    // slider; the chat cap was the only thing
    // pinning it to 12.
    if arg.is_empty() {
        lines.push(format!(
            "Render distance: {}",
            voxel.settings.view_distance
        ));
    } else if let Ok(v) = arg.parse::<u8>() {
        voxel.settings.view_distance = v.clamp(2, crate::VIEW_DISTANCE_MAX as u8);
        voxel.save_config();
        lines.push(format!(
            "Render distance set to {}.",
            voxel.settings.view_distance
        ));
    } else {
        lines.push(format!("Cannot parse distance: {arg}"));
    }
    true
}

/// `/worldgen` (alias `/gen`). Print or switch the worldgen
/// mode (host-only).
pub(crate) fn cmd_worldgen(
    voxel: &mut Voxel,
    arg: &str,
    lines: &mut Vec<String>,
) -> bool {
    // /worldgen with no arg prints the
    // current dim's mode; with classic /
    // flat / islands / amplified it switches
    // the active dim's profile +
    // regenerates every cached chunk in it.
    // Host-only since the workers run
    // server-side.
    if !matches!(
        voxel.net,
        NetRole::Server { .. } | NetRole::Offline
    ) {
        lines.push("/worldgen is host-only.".to_string());
    } else if arg.is_empty() {
        // W5.1: report the active dim's profile from voxel state.
        let profile = voxel.current_worldgen_ctx().profile;
        lines.push(format!("Worldgen: {:?}", profile));
    } else if let Some(g) = WorldGen::from_str(arg) {
        // W5.1: write the per-dim worldgen variant; the ctx
        // resolves to this map on next chunk-gen call.
        let dim = voxel.current_dim.clone();
        voxel.dim_worldgens.insert(dim, g);
        let to_unload: Vec<ChunkCoord> =
            voxel.world.iter_chunks().map(|(c, _)| *c).collect();
        if !to_unload.is_empty() {
            let renderer =
                voxel.renderer.as_mut().expect("renderer ready");
            for (_cc, subs) in voxel.chunk_meshes.drain() {
                for (_block, handle) in subs {
                    renderer.remove_mesh(handle);
                }
            }
            for cc in &to_unload {
                voxel.world.remove_chunk(*cc);
            }
        }
        voxel.pending_chunks.clear();
        voxel.pending_set.clear();
        voxel.in_flight_chunks.clear();
        lines.push(format!(
            "Worldgen set to {g:?}; chunks regenerating."
        ));
    } else {
        lines.push(
            "Usage: /worldgen classic|flat|islands|amplified|realistic"
                .to_string(),
        );
    }
    true
}

/// `/clearedits` (alias `/wipe`). Drop edit history + force a
/// re-stream from worldgen (host-only).
pub(crate) fn cmd_clearedits(
    voxel: &mut Voxel,
    _arg: &str,
    lines: &mut Vec<String>,
) -> bool {
    // Host-only: drop the entire edit
    // history + every cached chunk. Next
    // streaming pass regenerates from
    // scratch under the current seed.
    if !matches!(
        voxel.net,
        NetRole::Server { .. } | NetRole::Offline
    ) {
        lines.push("/clearedits is host-only.".to_string());
    } else {
        let n_edits = voxel.edit_history.len();
        voxel.edit_history.clear();
        voxel.torch_positions.clear();
        let to_unload: Vec<ChunkCoord> =
            voxel.world.iter_chunks().map(|(c, _)| *c).collect();
        if !to_unload.is_empty() {
            let renderer =
                voxel.renderer.as_mut().expect("renderer ready");
            for (_cc, subs) in voxel.chunk_meshes.drain() {
                for (_block, handle) in subs {
                    renderer.remove_mesh(handle);
                }
            }
            for cc in &to_unload {
                voxel.world.remove_chunk(*cc);
            }
        }
        voxel.pending_chunks.clear();
        voxel.pending_set.clear();
        voxel.in_flight_chunks.clear();
        lines.push(format!(
            "Wiped {n_edits} edits + regenerating chunks."
        ));
    }
    true
}

/// `/chunkinfo`. Diagnostic dump: chunk count, pending queue,
/// edit history, total voxels.
pub(crate) fn cmd_chunkinfo(
    voxel: &mut Voxel,
    _arg: &str,
    lines: &mut Vec<String>,
) -> bool {
    // Diagnostic: chunk count + size +
    // pending queue + edit history length.
    let cs = CHUNK_SIZE as i32;
    let voxels_per_chunk = (cs as u64).pow(3);
    let total =
        voxel.chunk_meshes.len() as u64 * voxels_per_chunk;
    lines.push(format!(
        "Chunks: {}  pending: {}  edits: {}  voxels: {total}",
        voxel.chunk_meshes.len(),
        voxel.pending_chunks.len(),
        voxel.edit_history.len(),
    ));
    true
}

/// `/tree`. Drop a small oak tree a few blocks in front of the
/// player (host-only).
pub(crate) fn cmd_tree(
    voxel: &mut Voxel,
    _arg: &str,
    lines: &mut Vec<String>,
) -> bool {
    // Host-only: drop a small oak tree
    // a few blocks in front of the
    // player. Routes every voxel through
    // the edit pipeline so the change
    // persists + replicates.
    if !matches!(
        voxel.net,
        NetRole::Server { .. } | NetRole::Offline
    ) {
        lines.push("/tree is host-only.".to_string());
        return true;
    }
    let fwd = Vec3::new(
        voxel.player_yaw.cos(),
        0.0,
        -voxel.player_yaw.sin(),
    )
    .normalize_or_zero();
    let cx = (voxel.player_pos.x + fwd.x * 4.0).floor() as i32;
    let cz = (voxel.player_pos.z + fwd.z * 4.0).floor() as i32;
    let ceiling = (CHUNK_SIZE as i32) * VERTICAL_CHUNKS - 1;
    let sy = voxel
        .surface_y_below(cx, cz, ceiling)
        .unwrap_or(voxel.player_pos.y.floor() as i32);
    let cfg = bincode::config::standard();
    let mut place = |x: i32, y: i32, z: i32, bid: u16| {
        let v = [x, y, z];
        voxel.world_set_tracked(v, BlockId(bid));
        let edit = EditMsg { x, y, z, block: bid };
        if let NetRole::Server { server, .. } = &mut voxel.net {
            voxel.edit_history.push(edit);
            if let Ok(buf) =
                bincode::serde::encode_to_vec(edit, cfg)
            {
                server.broadcast_bytes("vx.edit", buf);
            }
        }
    };
    // 4-block trunk.
    for ty in 1..=4 {
        place(cx, sy + ty, cz, 5);
    }
    // 2 canopy layers + cap + top.
    for layer in 0i32..2 {
        let wy = sy + 3 + layer;
        for dx in -2i32..=2 {
            for dz in -2i32..=2 {
                if dx.abs() == 2 && dz.abs() == 2 {
                    continue;
                }
                if dx == 0 && dz == 0 && layer == 0 {
                    continue;
                }
                place(cx + dx, wy, cz + dz, 6);
            }
        }
    }
    for dx in -1i32..=1 {
        for dz in -1i32..=1 {
            place(cx + dx, sy + 5, cz + dz, 6);
        }
    }
    place(cx, sy + 6, cz, 6);
    let coords = Voxel::affected_chunks([cx, sy + 3, cz]);
    voxel.rebuild_chunks(&coords);
    lines.push("Planted a tree.".to_string());
    true
}

