//! W479 - entities v8 -> v9 migration arrow + the FROZEN v8 shapes.
//!
//! v9 appends per-mob `despawn_delay: u32` (the wandering trader's
//! 48000-tick budget; 0 = no timer on every other kind). Pre-v9
//! saves predate the trader spawner port, so every mob lifts with
//! `despawn_delay: 0` - exactly the no-timer behaviour the old load
//! path reconstructed.
//!
//! Lives in its own module (not `entities.rs`) purely for the file
//! size budget - `entities.rs` already carries seven frozen
//! generations. The frozen v8 shapes are `pub(super)`-consumed by
//! the 7 -> 8 arrow there (the W459/W468 freeze-everything rule:
//! an arrow must never encode the LIVE struct under an old tag).

use std::io;

use serde::{Deserialize, Serialize};

use super::entities::{
    COMP_NONE, COMP_ZSTD, DimensionEntities, DropSave, ENTITIES_MAGIC,
    MobSave, ZSTD_LEVEL, decode_zstd, invalid,
};
use super::entities::CartSave;

/// W479: the v8 on-disk mob shape (pre-`despawn_delay`), frozen so
/// the 7 -> 8 and 8 -> 9 arrows keep decoding/encoding true v8
/// bytes while the live `MobSave` evolves.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct MobSaveV8 {
    pub id: u32,
    pub kind: u8,
    pub pos: [f32; 3],
    pub yaw: f32,
    pub vel: [f32; 3],
    pub health: f32,
    pub age_seconds: f32,
    pub persistent: bool,
    pub baby_left: f32,
    pub tamed: bool,
    pub temper: u8,
    pub trade_xp: u32,
    pub sheared: bool,
    pub size: u8,
}

impl MobSaveV8 {
    /// Lift to the current shape with the v9 default (no despawn
    /// timer - the pre-W479 behaviour for every mob).
    fn lift(self) -> MobSave {
        MobSave {
            id: self.id,
            kind: self.kind,
            pos: self.pos,
            yaw: self.yaw,
            vel: self.vel,
            health: self.health,
            age_seconds: self.age_seconds,
            persistent: self.persistent,
            baby_left: self.baby_left,
            tamed: self.tamed,
            temper: self.temper,
            trade_xp: self.trade_xp,
            sheared: self.sheared,
            size: self.size,
            despawn_delay: 0,
        }
    }
}

/// W479: the v8 on-disk drop shape, frozen even though v9 does not
/// change drops (the freeze-everything rule).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct DropSaveV8 {
    pub id: u32,
    pub stack: crate::item::ItemStack,
    pub pos: [f32; 3],
    pub vel: [f32; 3],
    pub age: f32,
}

impl DropSaveV8 {
    /// Lift to the current shape (field-identical today).
    fn lift(self) -> DropSave {
        DropSave {
            id: self.id,
            stack: self.stack,
            pos: self.pos,
            vel: self.vel,
            age: self.age,
        }
    }
}

/// W479: the v8 on-disk dimension shape.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct DimensionEntitiesV8 {
    pub format_version: u32,
    pub mobs: Vec<MobSaveV8>,
    pub drops: Vec<DropSaveV8>,
    pub next_mob_id: u32,
    pub next_drop_id: u32,
    pub carts: Vec<CartSave>,
    pub next_cart_id: u32,
    pub structure_mob_anchors: Vec<[i32; 3]>,
}

/// W479: one-step v8 -> v9 migration arrow - lifts every mob with
/// `despawn_delay: 0` and copies the frozen v8 drops across
/// unchanged. Same envelope handling as the earlier arrows.
pub(crate) fn entities_v8_to_v9(bytes: &[u8]) -> io::Result<Vec<u8>> {
    if bytes.len() < 17 {
        return Err(invalid("entities v8 envelope truncated"));
    }
    if &bytes[..4] != ENTITIES_MAGIC {
        return Err(invalid("entities v8 envelope magic mismatch"));
    }
    let on_disk_v = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    if on_disk_v != 8 {
        return Err(invalid(format!(
            "entities_v8_to_v9 expected v8, got v{on_disk_v}"
        )));
    }
    let kind = bytes[8];
    let uncompressed_len = u64::from_le_bytes([
        bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14],
        bytes[15], bytes[16],
    ]) as usize;
    let body = &bytes[17..];
    let raw_v8: Vec<u8> = match kind {
        COMP_NONE => {
            if body.len() != uncompressed_len {
                return Err(invalid(
                    "entities v8 body length mismatch during migration",
                ));
            }
            body.to_vec()
        }
        COMP_ZSTD => decode_zstd(body, uncompressed_len)?,
        other => {
            return Err(invalid(format!(
                "unknown entities v8 compression kind {other} during migration"
            )));
        }
    };
    let cfg = bincode::config::standard();
    let (v8, _read) =
        bincode::serde::decode_from_slice::<DimensionEntitiesV8, _>(&raw_v8, cfg)
            .map_err(|err| {
                invalid(format!("entities v8 payload decode failed: {err}"))
            })?;
    let v9 = DimensionEntities {
        format_version: 9,
        mobs: v8.mobs.into_iter().map(MobSaveV8::lift).collect(),
        drops: v8.drops.into_iter().map(DropSaveV8::lift).collect(),
        next_mob_id: v8.next_mob_id,
        next_drop_id: v8.next_drop_id,
        carts: v8.carts,
        next_cart_id: v8.next_cart_id,
        structure_mob_anchors: v8.structure_mob_anchors,
    };
    let raw_v9 = bincode::serde::encode_to_vec(&v9, cfg)
        .map_err(|err| invalid(format!("entities v9 payload encode failed: {err}")))?;
    let uncompressed_len_v9 = raw_v9.len() as u64;
    let (kind_v9, body_v9): (u8, Vec<u8>) =
        match zstd::encode_all(&raw_v9[..], ZSTD_LEVEL) {
            Ok(compressed) if compressed.len() < raw_v9.len() => {
                (COMP_ZSTD, compressed)
            }
            _ => (COMP_NONE, raw_v9),
        };
    let mut out: Vec<u8> = Vec::with_capacity(body_v9.len() + 17);
    out.extend_from_slice(ENTITIES_MAGIC);
    out.extend_from_slice(&9u32.to_le_bytes());
    out.push(kind_v9);
    out.extend_from_slice(&uncompressed_len_v9.to_le_bytes());
    out.extend_from_slice(&body_v9);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn envelope(version: u32, raw: &[u8]) -> Vec<u8> {
        let mut bytes: Vec<u8> = Vec::new();
        bytes.extend_from_slice(ENTITIES_MAGIC);
        bytes.extend_from_slice(&version.to_le_bytes());
        bytes.push(COMP_NONE);
        bytes.extend_from_slice(&(raw.len() as u64).to_le_bytes());
        bytes.extend_from_slice(raw);
        bytes
    }

    /// A hand-built FROZEN v8 envelope lifts through the 8 -> 9
    /// arrow - every mob gains `despawn_delay: 0`, the other fields
    /// and the drops carry through untouched.
    #[test]
    fn entities_v8_to_v9_lifts_despawn_delay() {
        let cfg = bincode::config::standard();
        let mut worn = crate::item::ItemStack::new(48, 5);
        worn.components.damage = 20;
        let v8 = DimensionEntitiesV8 {
            format_version: 8,
            mobs: vec![MobSaveV8 {
                id: 3,
                kind: 43, // wandering trader - lifts with NO timer
                pos: [1.0, 64.0, 1.0],
                yaw: 0.25,
                vel: [0.0; 3],
                health: 20.0,
                age_seconds: 0.0,
                persistent: true,
                baby_left: 0.0,
                tamed: false,
                temper: 0,
                trade_xp: 12,
                sheared: false,
                size: 4,
            }],
            drops: vec![DropSaveV8 {
                id: 9,
                stack: worn.clone(),
                pos: [2.5, 65.0, 2.5],
                vel: [0.0, 0.1, 0.0],
                age: 4.0,
            }],
            next_mob_id: 4,
            next_drop_id: 10,
            carts: Vec::new(),
            next_cart_id: 1,
            structure_mob_anchors: vec![[4, 5, 6]],
        };
        let raw = bincode::serde::encode_to_vec(&v8, cfg).unwrap();
        let bytes = envelope(8, &raw);
        let out = entities_v8_to_v9(&bytes).expect("arrow lifts");
        assert_eq!(u32::from_le_bytes([out[4], out[5], out[6], out[7]]), 9);
        let kind = out[8];
        let len = u64::from_le_bytes(out[9..17].try_into().unwrap()) as usize;
        let body: Vec<u8> = match kind {
            COMP_NONE => out[17..].to_vec(),
            _ => decode_zstd(&out[17..], len).unwrap(),
        };
        let (v9, _) =
            bincode::serde::decode_from_slice::<DimensionEntities, _>(&body, cfg)
                .unwrap();
        assert_eq!(v9.format_version, 9);
        assert_eq!(v9.mobs.len(), 1);
        assert_eq!(v9.mobs[0].despawn_delay, 0, "v8 mobs lift with no timer");
        assert_eq!(v9.mobs[0].size, 4, "v8 size carries through");
        assert_eq!(v9.mobs[0].trade_xp, 12, "v8 mob fields carry through");
        assert_eq!(v9.drops.len(), 1);
        assert_eq!(v9.drops[0].stack, worn, "v8 drop stacks carry through");
        assert_eq!(v9.structure_mob_anchors, vec![[4, 5, 6]]);
        assert_eq!(v9.next_drop_id, 10);
    }

    /// The 8 -> 9 arrow refuses input tagged with any other
    /// envelope version.
    #[test]
    fn entities_v8_to_v9_rejects_wrong_version() {
        let cfg = bincode::config::standard();
        let v8 = DimensionEntitiesV8 {
            format_version: 8,
            mobs: Vec::new(),
            drops: Vec::new(),
            next_mob_id: 0,
            next_drop_id: 0,
            carts: Vec::new(),
            next_cart_id: 0,
            structure_mob_anchors: Vec::new(),
        };
        let raw = bincode::serde::encode_to_vec(&v8, cfg).unwrap();
        let bytes = envelope(7, &raw);
        let err = entities_v8_to_v9(&bytes).expect_err("must refuse v7 input");
        assert!(
            err.to_string().contains("expected v8"),
            "error names the version mismatch, got: {err}"
        );
    }
}
