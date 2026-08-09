//! W479 - players v6 -> v7 migration arrow + the FROZEN v6 shape.
//!
//! v7 appends `ticks_since_rest: u32` (the MC `TIME_SINCE_REST`
//! stat that drives the phantom insomnia roll). Pre-v7 saves
//! predate the stat, so the lift default 0 loads every player well
//! rested - exactly the pre-W479 behaviour (no phantoms until an
//! hour of play).
//!
//! Per the W474/W476.1 append-arrow precedent this bump SHIPS its
//! arrow (the user's un-verified v6 world must stay loadable); the
//! R39 dev-policy dead-end at v5 is unchanged for anything older.
//! Lives in its own module purely for the `players.rs` size budget.

use std::io;

use serde::{Deserialize, Serialize};

use super::players::{
    COMP_NONE, COMP_ZSTD, PLAYER_MAGIC, PlayerSave, ZSTD_LEVEL, decode_zstd,
    invalid,
};

/// W479: FROZEN v6 payload shape - `ItemStack` inventory + craft
/// grid + armor, WITHOUT `ticks_since_rest`. The 6 -> 7 arrow
/// decodes THIS (decoding the live `PlayerSave` would demand a v7
/// field a v6 envelope cannot contain - the W387/W459 lesson).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct PlayerSaveV6 {
    pub format_version: u32,
    pub pos: [f32; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub inventory: Vec<Option<crate::item::ItemStack>>,
    pub craft_grid: Vec<Option<crate::item::ItemStack>>,
    pub selected_slot: u8,
    pub air: f32,
    pub health: f32,
    pub home_pos: Option<[f32; 3]>,
    pub spawn_pos: Option<[f32; 3]>,
    pub permission_level: u8,
    pub play_time_seconds: u64,
    pub last_dimension: String,
    pub highlight_mode: u8,
    pub game_mode: u8,
    pub food: f32,
    pub saturation: f32,
    pub xp: u32,
    pub armor: Vec<Option<crate::item::ItemStack>>,
}

/// W479: one-step v6 -> v7 migration arrow - appends
/// `ticks_since_rest: 0` (well rested). Same envelope handling as
/// the earlier player arrows.
pub(crate) fn player_v6_to_v7(bytes: &[u8]) -> io::Result<Vec<u8>> {
    if bytes.len() < 17 {
        return Err(invalid("player v6 envelope truncated"));
    }
    if &bytes[..4] != PLAYER_MAGIC {
        return Err(invalid("player v6 envelope magic mismatch"));
    }
    let on_disk_v = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    if on_disk_v != 6 {
        return Err(invalid(format!(
            "player_v6_to_v7 expected v6, got v{on_disk_v}"
        )));
    }
    let kind = bytes[8];
    let uncompressed_len = u64::from_le_bytes([
        bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14],
        bytes[15], bytes[16],
    ]) as usize;
    let body = &bytes[17..];
    let raw_v6: Vec<u8> = match kind {
        COMP_NONE => {
            if body.len() != uncompressed_len {
                return Err(invalid("v6 body length mismatch during migration"));
            }
            body.to_vec()
        }
        COMP_ZSTD => decode_zstd(body, uncompressed_len)?,
        other => {
            return Err(invalid(format!(
                "unknown v6 compression kind {other} during migration"
            )));
        }
    };
    let cfg = bincode::config::standard();
    let (v6, _read) =
        bincode::serde::decode_from_slice::<PlayerSaveV6, _>(&raw_v6, cfg)
            .map_err(|err| invalid(format!("v6 payload decode failed: {err}")))?;
    let v7 = PlayerSave {
        format_version: 7,
        pos: v6.pos,
        yaw: v6.yaw,
        pitch: v6.pitch,
        inventory: v6.inventory,
        craft_grid: v6.craft_grid,
        selected_slot: v6.selected_slot,
        air: v6.air,
        health: v6.health,
        home_pos: v6.home_pos,
        spawn_pos: v6.spawn_pos,
        permission_level: v6.permission_level,
        play_time_seconds: v6.play_time_seconds,
        last_dimension: v6.last_dimension,
        highlight_mode: v6.highlight_mode,
        game_mode: v6.game_mode,
        food: v6.food,
        saturation: v6.saturation,
        xp: v6.xp,
        armor: v6.armor,
        ticks_since_rest: 0,
    };
    let raw_v7 = bincode::serde::encode_to_vec(&v7, cfg)
        .map_err(|err| invalid(format!("v7 payload encode failed: {err}")))?;
    let uncompressed_len_v7 = raw_v7.len() as u64;
    let (kind_v7, body_v7): (u8, Vec<u8>) =
        match zstd::encode_all(&raw_v7[..], ZSTD_LEVEL) {
            Ok(compressed) if compressed.len() < raw_v7.len() => {
                (COMP_ZSTD, compressed)
            }
            _ => (COMP_NONE, raw_v7),
        };
    let mut out: Vec<u8> = Vec::with_capacity(body_v7.len() + 17);
    out.extend_from_slice(PLAYER_MAGIC);
    out.extend_from_slice(&7u32.to_le_bytes());
    out.push(kind_v7);
    out.extend_from_slice(&uncompressed_len_v7.to_le_bytes());
    out.extend_from_slice(&body_v7);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::super::players::{
        CRAFT_GRID_SLOTS_DEFAULT, INVENTORY_SLOTS_DEFAULT,
        PLAYER_FORMAT_VERSION, player_path_in_dir, read_player_in_dir,
    };
    use super::*;

    fn sample_v6() -> PlayerSaveV6 {
        let mut inventory: Vec<Option<crate::item::ItemStack>> =
            vec![None; INVENTORY_SLOTS_DEFAULT];
        let mut worn = crate::item::ItemStack::new(48, 1);
        worn.components.damage = 33;
        inventory[2] = Some(worn);
        PlayerSaveV6 {
            format_version: 6,
            pos: [4.5, 70.0, -3.5],
            yaw: 0.5,
            pitch: -0.2,
            inventory,
            craft_grid: vec![None; CRAFT_GRID_SLOTS_DEFAULT],
            selected_slot: 2,
            air: 12.0,
            health: 18.0,
            home_pos: None,
            spawn_pos: Some([0.0, 64.0, 0.0]),
            permission_level: 4,
            play_time_seconds: 99,
            last_dimension: "overworld".to_string(),
            highlight_mode: 1,
            game_mode: 1,
            food: 14.0,
            saturation: 2.5,
            xp: 77,
            armor: vec![None; 4],
        }
    }

    fn encode_v6_envelope(v6: &PlayerSaveV6) -> Vec<u8> {
        let cfg = bincode::config::standard();
        let raw = bincode::serde::encode_to_vec(v6, cfg).unwrap();
        let mut bytes: Vec<u8> = Vec::with_capacity(raw.len() + 17);
        bytes.extend_from_slice(PLAYER_MAGIC);
        bytes.extend_from_slice(&6u32.to_le_bytes());
        bytes.push(COMP_NONE);
        bytes.extend_from_slice(&(raw.len() as u64).to_le_bytes());
        bytes.extend_from_slice(&raw);
        bytes
    }

    /// The 6 -> 7 arrow appends `ticks_since_rest: 0`; every v6
    /// field (stack components included) carries through.
    #[test]
    fn player_v6_to_v7_appends_rested_clock() {
        let v6 = sample_v6();
        let out = player_v6_to_v7(&encode_v6_envelope(&v6)).expect("arrow lifts");
        assert_eq!(u32::from_le_bytes([out[4], out[5], out[6], out[7]]), 7);
        let kind = out[8];
        let len = u64::from_le_bytes(out[9..17].try_into().unwrap()) as usize;
        let body: Vec<u8> = match kind {
            COMP_NONE => out[17..].to_vec(),
            _ => decode_zstd(&out[17..], len).unwrap(),
        };
        let cfg = bincode::config::standard();
        let (v7, _) =
            bincode::serde::decode_from_slice::<PlayerSave, _>(&body, cfg)
                .unwrap();
        assert_eq!(v7.format_version, 7);
        assert_eq!(v7.ticks_since_rest, 0, "migrated players wake rested");
        assert_eq!(v7.health, 18.0);
        assert_eq!(v7.xp, 77);
        assert_eq!(v7.game_mode, 1);
        let stack = v7.inventory[2].as_ref().expect("slot 2 survives");
        assert_eq!(stack.components.damage, 33, "components carry through");
    }

    /// The arrow refuses input tagged with any other version.
    #[test]
    fn player_v6_to_v7_rejects_wrong_version() {
        let mut bytes = vec![0u8; 17];
        bytes[..4].copy_from_slice(PLAYER_MAGIC);
        bytes[4..8].copy_from_slice(&7u32.to_le_bytes());
        let err = player_v6_to_v7(&bytes).expect_err("must reject v7");
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    /// Chain through the PRODUCTION reader: a v6 file on disk loads
    /// at the current version via the registered arrow (the whole
    /// point of the append-arrow policy - the user's un-verified v6
    /// world keeps loading), while pre-6 still refuses.
    #[test]
    fn v6_file_loads_through_production_registry() {
        let v6 = sample_v6();
        let bytes = encode_v6_envelope(&v6);
        let dir = tempfile::tempdir().expect("tempdir");
        let path = player_path_in_dir(dir.path(), "w479", "sleeper");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &bytes).unwrap();
        let loaded = read_player_in_dir(dir.path(), "w479", "sleeper")
            .expect("v6 must migrate cleanly")
            .expect("file present");
        assert_eq!(loaded.format_version, PLAYER_FORMAT_VERSION);
        assert_eq!(loaded.ticks_since_rest, 0);
        assert_eq!(loaded.xp, 77, "v6 fields survive the chain");
        // The migrated bytes were written back at v7.
        let rewritten = std::fs::read(&path).unwrap();
        assert_eq!(
            u32::from_le_bytes([
                rewritten[4],
                rewritten[5],
                rewritten[6],
                rewritten[7]
            ]),
            PLAYER_FORMAT_VERSION
        );
    }
}
