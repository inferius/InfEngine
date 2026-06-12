//! Mob kind enum + per-kind balance config.
//!
//! Mirrors MC's `net.minecraft.world.entity.EntityType` registry
//! pattern: one variant per concrete mob type plus a small lookup
//! table of stats (max HP, walk speed, hitbox half-extents). The
//! enum is `#[repr(u8)]` so the wire shape (`MobMsg.kind: u8`) and
//! the persistence shape (`MobSave.kind: u8`) stay byte-compatible
//! with the pre-split `KIND_*` constants. Renumbering a variant
//! would break old saves and over-the-wire frames the same way
//! renumbering the `u8` constants would, so the discriminants are
//! pinned numerically here and must not be reordered.
//!
//! Cited MC source files in commit messages:
//! - `net/minecraft/world/entity/EntityType.java` - registry that
//!   maps a numeric id to the runtime entity factory.
//! - `net/minecraft/world/entity/monster/Monster.java` - hostile
//!   base type behaviour.
//! - `net/minecraft/world/entity/animal/Animal.java` - passive
//!   base type behaviour.

/// Numeric mob discriminator. Identical wire-shape to the legacy
/// `u8` `KIND_*` constants - the variant discriminant is the same
/// number, so old `MobMsg` / `MobSave` payloads keep deserialising
/// unchanged.
///
/// Adding a new kind: append a new variant with the next free
/// discriminant + register it in `KIND_INFO` below. Removing a
/// variant or renumbering one breaks save / wire compatibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
#[allow(dead_code)] // Cow / Skeleton / Spider wired in H.2 + H.3.
pub(crate) enum MobKind {
    Sheep = 0,
    Zombie = 1,
    Chicken = 2,
    Pig = 3,
    Cow = 4,
    Skeleton = 5,
    Spider = 6,
}

#[allow(dead_code)] // Helpers are exercised by tests + by H.2 + H.3.
impl MobKind {
    /// Round-trip from the raw wire / save byte. Unknown values
    /// collapse to `Sheep` so a corrupt or forward-compat byte
    /// doesn't crash the runtime.
    pub(crate) fn from_u8(b: u8) -> Self {
        match b {
            0 => MobKind::Sheep,
            1 => MobKind::Zombie,
            2 => MobKind::Chicken,
            3 => MobKind::Pig,
            4 => MobKind::Cow,
            5 => MobKind::Skeleton,
            6 => MobKind::Spider,
            _ => MobKind::Sheep,
        }
    }

    /// Raw byte form for wire / save serialisation. Matches the
    /// `#[repr(u8)]` discriminant.
    #[inline]
    pub(crate) fn as_u8(self) -> u8 {
        self as u8
    }

    /// True for hostile mobs that chase players + deal contact
    /// damage. Drives the dark-only spawn gate in `spawn.rs`.
    pub(crate) fn is_hostile(self) -> bool {
        matches!(
            self,
            MobKind::Zombie | MobKind::Skeleton | MobKind::Spider
        )
    }

    /// True for passive mobs that spawn on well-lit grass + flee
    /// from the player when hit. Mirrors MC's `Animal` subclass.
    pub(crate) fn is_passive(self) -> bool {
        matches!(
            self,
            MobKind::Sheep
                | MobKind::Chicken
                | MobKind::Pig
                | MobKind::Cow
        )
    }

    /// Short lowercase debug name. Used by chat commands + HUD.
    pub(crate) fn name(self) -> &'static str {
        match self {
            MobKind::Sheep => "sheep",
            MobKind::Zombie => "zombie",
            MobKind::Chicken => "chicken",
            MobKind::Pig => "pig",
            MobKind::Cow => "cow",
            MobKind::Skeleton => "skeleton",
            MobKind::Spider => "spider",
        }
    }
}

// ----------------------------------------------------------------
// Legacy u8 aliases.
//
// External call sites (chat commands, save layer, dim_switch tests)
// still reference the constants by their old names. Keep them as
// `pub(crate) const`-style aliases so the H.1 split is a pure
// refactor.
// ----------------------------------------------------------------

pub(crate) const KIND_SHEEP: u8 = MobKind::Sheep as u8;
pub(crate) const KIND_ZOMBIE: u8 = MobKind::Zombie as u8;
pub(crate) const KIND_CHICKEN: u8 = MobKind::Chicken as u8;
pub(crate) const KIND_PIG: u8 = MobKind::Pig as u8;
pub(crate) const KIND_COW: u8 = MobKind::Cow as u8;
#[allow(dead_code)] // Spawns from H.3 biome pools.
pub(crate) const KIND_SKELETON: u8 = MobKind::Skeleton as u8;
#[allow(dead_code)] // Spawns from H.3 biome pools.
pub(crate) const KIND_SPIDER: u8 = MobKind::Spider as u8;

#[cfg(test)]
mod tests {
    use super::*;

    /// W145: the spawn-egg item table hardcodes kind bytes (it also
    /// builds in the lib target where this module doesn't exist) -
    /// lock them against the real discriminants.
    #[test]
    fn spawn_egg_kinds_match_mob_kinds() {
        use crate::item::spawn_egg_kind;
        assert_eq!(spawn_egg_kind(677), Some(KIND_CHICKEN));
        assert_eq!(spawn_egg_kind(678), Some(KIND_COW));
        assert_eq!(spawn_egg_kind(679), Some(KIND_PIG));
        assert_eq!(spawn_egg_kind(680), Some(KIND_SHEEP));
        assert_eq!(spawn_egg_kind(681), Some(KIND_ZOMBIE));
        assert_eq!(spawn_egg_kind(682), Some(KIND_SKELETON));
        assert_eq!(spawn_egg_kind(683), Some(KIND_SPIDER));
        assert_eq!(spawn_egg_kind(657), None, "minecart is not an egg");
    }

    #[test]
    fn kind_discriminants_match_legacy_consts() {
        // Reordering a variant would break save + wire compat.
        // Pin every discriminant here so the regression is loud.
        assert_eq!(MobKind::Sheep.as_u8(), 0);
        assert_eq!(MobKind::Zombie.as_u8(), 1);
        assert_eq!(MobKind::Chicken.as_u8(), 2);
        assert_eq!(MobKind::Pig.as_u8(), 3);
        assert_eq!(MobKind::Cow.as_u8(), 4);
        assert_eq!(MobKind::Skeleton.as_u8(), 5);
        assert_eq!(MobKind::Spider.as_u8(), 6);
    }

    #[test]
    fn from_u8_round_trips_known() {
        for k in [
            MobKind::Sheep,
            MobKind::Zombie,
            MobKind::Chicken,
            MobKind::Pig,
            MobKind::Cow,
            MobKind::Skeleton,
            MobKind::Spider,
        ] {
            assert_eq!(MobKind::from_u8(k.as_u8()), k);
        }
    }

    #[test]
    fn from_u8_unknown_collapses_to_sheep() {
        // Any future / corrupted byte should land on the passive
        // fallback - no panic, no out-of-bounds.
        for raw in [7u8, 8, 100, 255] {
            assert_eq!(MobKind::from_u8(raw), MobKind::Sheep);
        }
    }

    #[test]
    fn hostile_passive_partition_is_disjoint() {
        // Every kind is exactly one of {hostile, passive}. If a
        // future variant lands in both buckets the goal selector
        // would pick conflicting goals.
        for k in [
            MobKind::Sheep,
            MobKind::Zombie,
            MobKind::Chicken,
            MobKind::Pig,
            MobKind::Cow,
            MobKind::Skeleton,
            MobKind::Spider,
        ] {
            assert_ne!(k.is_hostile(), k.is_passive(), "{:?}", k);
        }
    }

    #[test]
    fn hostiles_are_zombie_skeleton_spider() {
        assert!(MobKind::Zombie.is_hostile());
        assert!(MobKind::Skeleton.is_hostile());
        assert!(MobKind::Spider.is_hostile());
    }

    #[test]
    fn passives_are_farm_animals() {
        assert!(MobKind::Sheep.is_passive());
        assert!(MobKind::Chicken.is_passive());
        assert!(MobKind::Pig.is_passive());
        assert!(MobKind::Cow.is_passive());
    }

    #[test]
    fn name_lookup_is_lowercase_ascii() {
        for k in [
            MobKind::Sheep,
            MobKind::Zombie,
            MobKind::Chicken,
            MobKind::Pig,
            MobKind::Cow,
            MobKind::Skeleton,
            MobKind::Spider,
        ] {
            let n = k.name();
            assert!(n.is_ascii(), "{:?} name not ascii: {}", k, n);
            assert_eq!(n, n.to_lowercase(), "{:?}: {}", k, n);
        }
    }

    #[test]
    fn skeleton_spider_extend_hostile_pool() {
        // Wave 6 / H.3 adds two new hostile kinds without altering
        // existing wire shapes.
        assert_eq!(KIND_SKELETON, 5);
        assert_eq!(KIND_SPIDER, 6);
    }
}
