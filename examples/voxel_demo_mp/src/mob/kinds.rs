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
    /// W158 - the first of the biome-animal expansion.
    Wolf = 7,
    /// W161 - polar bear (snowy biomes).
    PolarBear = 8,
    /// W162 - rabbit (first NESTED bone hierarchy; plains/desert/taiga).
    Rabbit = 9,
    /// W300 - zombified piglin (the nether's native ambler;
    /// `NeutralMob` - only fights back after being hit).
    ZombifiedPiglin = 10,
    /// W302 - villager (structure-spawned; villages are alive).
    Villager = 11,
    /// W315 - cat (village template entities; strolls, no taming yet).
    Cat = 12,
    /// W322 - iron golem (village guardian; strolls, defense later).
    IronGolem = 13,
    /// W322 - horse (village stables; taming/riding later).
    Horse = 14,
    /// W327 - ghast (nether floater; fireballs are a follow-up).
    Ghast = 15,
    /// W327 - magma cube (nether hopper, contact damage via melee).
    MagmaCube = 16,
    /// W330 - zombie villager (zombie-village template entities;
    /// curing is a follow-up).
    ZombieVillager = 17,
    /// W343 - creeper (swell + the W337 explosion, radius 3).
    Creeper = 18,
    /// W344 - donkey (equine + panniers; village stables).
    Donkey = 19,
    /// W375 - bee (tree bee nests; hovers, stings when hit).
    Bee = 20,
    /// W376 - witch (swamp-hut resident; villager mesh, own sheet).
    Witch = 21,
    /// W376 - slime (swamp night surface spawns; magma-cube mesh).
    Slime = 22,
    /// W377 - drowned (zombie mesh, underwater-green sheet).
    Drowned = 23,
    /// W377 - husk (zombie mesh, desert-sand sheet).
    Husk = 24,
    /// W377 - stray (skeleton mesh, frost sheet).
    Stray = 25,
    /// W378 - illager family (one IllagerModel, three sheets).
    Pillager = 26,
    Vindicator = 27,
    Evoker = 28,
    /// W379 - blaze (fortress throne spawner, floats).
    Blaze = 29,
    /// W379 - wither skeleton (skeleton mesh, sooty sheet; spawn
    /// waits on the fortress-interior probe).
    WitherSkeleton = 30,
    /// W380 - guardian + elder (monument structure override).
    Guardian = 31,
    ElderGuardian = 32,
    /// W381 - enderman (night surface, 3 blocks tall).
    Enderman = 33,
    /// W381 - cave spider (mineshaft spawner; spider mesh 0.7x).
    CaveSpider = 34,
    /// W381 - silverfish (stronghold spawner).
    Silverfish = 35,
    /// W382 - phantom (night flyer), bat (cave ambient),
    /// mooshroom (cow mesh, mushroom-fields resident).
    Phantom = 36,
    Bat = 37,
    Mooshroom = 38,
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
            7 => MobKind::Wolf,
            8 => MobKind::PolarBear,
            9 => MobKind::Rabbit,
            10 => MobKind::ZombifiedPiglin,
            11 => MobKind::Villager,
            12 => MobKind::Cat,
            13 => MobKind::IronGolem,
            14 => MobKind::Horse,
            15 => MobKind::Ghast,
            16 => MobKind::MagmaCube,
            17 => MobKind::ZombieVillager,
            18 => MobKind::Creeper,
            19 => MobKind::Donkey,
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
                // W158: the wild wolf wanders like an animal (the
                // MC neutral anger state is a follow-up).
                | MobKind::Wolf
                // W161: polar bear wanders (MC neutral - hostile when
                // a cub is near is a follow-up).
                | MobKind::PolarBear
                // W162: rabbit hops + flees (the MC hop gait is a
                // follow-up; for now it wanders like other passives).
                | MobKind::Rabbit
                // W302: villager - strolls + panics + looks at players
                // (trades/schedules are follow-ups). Structure-spawned
                // only; the natural-spawn pools never pick it.
                | MobKind::Villager
                // W315: cat strolls like an animal (taming/sitting
                // are follow-ups; village cats just live there).
                | MobKind::Cat
                // W344: donkey grazes like the horse (riding/taming
                // are follow-ups).
                | MobKind::Donkey
                // W322: golem is neutral (mob defense is a follow-up)
                // and the horse is a plain animal until riding lands.
                | MobKind::IronGolem
                | MobKind::Horse
                // W327: the ghast floats passively until fireballs
                // land (a melee chase would look absurd on it); the
                // magma cube is NOT here - it hops at the player.
                | MobKind::Ghast
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
            MobKind::Wolf => "wolf",
            MobKind::PolarBear => "polar_bear",
            MobKind::Rabbit => "rabbit",
            MobKind::ZombifiedPiglin => "zombified_piglin",
            MobKind::Villager => "villager",
            MobKind::Cat => "cat",
            MobKind::IronGolem => "iron_golem",
            MobKind::Horse => "horse",
            MobKind::Ghast => "ghast",
            MobKind::MagmaCube => "magma_cube",
            MobKind::ZombieVillager => "zombie_villager",
            MobKind::Creeper => "creeper",
            MobKind::Donkey => "donkey",
            MobKind::Bee => "bee",
            MobKind::Witch => "witch",
            MobKind::Slime => "slime",
            MobKind::Drowned => "drowned",
            MobKind::Husk => "husk",
            MobKind::Stray => "stray",
            MobKind::Pillager => "pillager",
            MobKind::Vindicator => "vindicator",
            MobKind::Evoker => "evoker",
            MobKind::Blaze => "blaze",
            MobKind::WitherSkeleton => "wither_skeleton",
            MobKind::Guardian => "guardian",
            MobKind::ElderGuardian => "elder_guardian",
            MobKind::Enderman => "enderman",
            MobKind::CaveSpider => "cave_spider",
            MobKind::Silverfish => "silverfish",
            MobKind::Phantom => "phantom",
            MobKind::Bat => "bat",
            MobKind::Mooshroom => "mooshroom",
        }
    }
}

/// W242.2: per-kind hitbox as `(hx, hy, hz, cy_off)` - half-extents
/// plus the box-centre Y offset above the feet position. Single
/// source of truth shared by the mob-targeting raycast and the
/// monster spawner's spawn-volume / crowding checks (it started as
/// the inline table in `raycast.rs`).
pub(crate) fn hitbox_for(kind: u8) -> (f32, f32, f32, f32) {
    match kind {
        // W302: MC villager 0.6 x 1.95.
        KIND_VILLAGER => (0.3, 0.975, 0.3, 0.975),
        // W315: MC cat 0.6 x 0.7.
        KIND_CAT => (0.3, 0.35, 0.3, 0.35),
        // W322: MC iron golem 1.4 x 2.7.
        KIND_IRON_GOLEM => (0.7, 1.35, 0.7, 1.35),
        // W322: MC horse 1.3965 x 1.6.
        KIND_HORSE => (0.7, 0.8, 0.7, 0.8),
        // W327: MC ghast 4 x 4.
        KIND_GHAST => (2.0, 2.0, 2.0, 2.0),
        // W327: MC magma cube size 2 = 1.02 x 1.02.
        KIND_MAGMA_CUBE => (0.51, 0.51, 0.51, 0.51),
        // W330: zombie villager = villager frame (0.6 x 1.95).
        KIND_ZOMBIE_VILLAGER => (0.3, 0.975, 0.3, 0.975),
        // W343: MC creeper 0.6 x 1.7.
        KIND_CREEPER => (0.3, 0.85, 0.3, 0.85),
        // W344: MC donkey 1.3965 x 1.5.
        KIND_DONKEY => (0.7, 0.75, 0.7, 0.75),
        KIND_ZOMBIE | KIND_SKELETON | KIND_ZOMBIFIED_PIGLIN => (0.45, 1.0, 0.45, 1.0),
        KIND_CHICKEN => (0.28, 0.45, 0.28, 0.35),
        KIND_PIG => (0.6, 0.55, 0.45, 0.45),
        KIND_SPIDER => (0.7, 0.45, 0.7, 0.45),
        KIND_COW => (0.7, 0.85, 0.5, 0.55),
        _ => (0.7, 0.75, 0.5, 0.55),
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
pub(crate) const KIND_WOLF: u8 = MobKind::Wolf as u8;
pub(crate) const KIND_POLAR_BEAR: u8 = MobKind::PolarBear as u8;
pub(crate) const KIND_RABBIT: u8 = MobKind::Rabbit as u8;
pub(crate) const KIND_ZOMBIFIED_PIGLIN: u8 = MobKind::ZombifiedPiglin as u8;
pub(crate) const KIND_VILLAGER: u8 = MobKind::Villager as u8;
pub(crate) const KIND_CAT: u8 = MobKind::Cat as u8;
pub(crate) const KIND_IRON_GOLEM: u8 = MobKind::IronGolem as u8;
pub(crate) const KIND_HORSE: u8 = MobKind::Horse as u8;
pub(crate) const KIND_GHAST: u8 = MobKind::Ghast as u8;
pub(crate) const KIND_MAGMA_CUBE: u8 = MobKind::MagmaCube as u8;
pub(crate) const KIND_ZOMBIE_VILLAGER: u8 = MobKind::ZombieVillager as u8;
pub(crate) const KIND_CREEPER: u8 = MobKind::Creeper as u8;
pub(crate) const KIND_DONKEY: u8 = MobKind::Donkey as u8;
pub(crate) const KIND_BEE: u8 = MobKind::Bee as u8;
pub(crate) const KIND_WITCH: u8 = MobKind::Witch as u8;
pub(crate) const KIND_SLIME: u8 = MobKind::Slime as u8;
pub(crate) const KIND_DROWNED: u8 = MobKind::Drowned as u8;
pub(crate) const KIND_HUSK: u8 = MobKind::Husk as u8;
pub(crate) const KIND_STRAY: u8 = MobKind::Stray as u8;
pub(crate) const KIND_PILLAGER: u8 = MobKind::Pillager as u8;
pub(crate) const KIND_VINDICATOR: u8 = MobKind::Vindicator as u8;
pub(crate) const KIND_EVOKER: u8 = MobKind::Evoker as u8;
pub(crate) const KIND_BLAZE: u8 = MobKind::Blaze as u8;
pub(crate) const KIND_WITHER_SKELETON: u8 = MobKind::WitherSkeleton as u8;
pub(crate) const KIND_GUARDIAN: u8 = MobKind::Guardian as u8;
pub(crate) const KIND_ELDER_GUARDIAN: u8 = MobKind::ElderGuardian as u8;
pub(crate) const KIND_ENDERMAN: u8 = MobKind::Enderman as u8;
pub(crate) const KIND_CAVE_SPIDER: u8 = MobKind::CaveSpider as u8;
pub(crate) const KIND_SILVERFISH: u8 = MobKind::Silverfish as u8;
pub(crate) const KIND_PHANTOM: u8 = MobKind::Phantom as u8;
pub(crate) const KIND_BAT: u8 = MobKind::Bat as u8;
pub(crate) const KIND_MOOSHROOM: u8 = MobKind::Mooshroom as u8;
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
        assert_eq!(spawn_egg_kind(717), Some(KIND_WOLF));
        assert_eq!(spawn_egg_kind(720), Some(KIND_POLAR_BEAR));
        assert_eq!(spawn_egg_kind(721), Some(KIND_RABBIT));
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
        // fallback - no panic, no out-of-bounds. (W158: 7 = Wolf,
        // W161: 8 = PolarBear, W162: 9 = Rabbit.)
        assert_eq!(MobKind::from_u8(7), MobKind::Wolf);
        assert_eq!(MobKind::from_u8(8), MobKind::PolarBear);
        assert_eq!(MobKind::from_u8(9), MobKind::Rabbit);
        // W300: 10 = ZombifiedPiglin. W302: 11 = Villager. W315: 12 = Cat.
        assert_eq!(MobKind::from_u8(10), MobKind::ZombifiedPiglin);
        assert_eq!(MobKind::from_u8(11), MobKind::Villager);
        assert_eq!(MobKind::from_u8(12), MobKind::Cat);
        // W322: 13 = IronGolem, 14 = Horse.
        assert_eq!(MobKind::from_u8(13), MobKind::IronGolem);
        assert_eq!(MobKind::from_u8(14), MobKind::Horse);
        // W327: 15 = Ghast, 16 = MagmaCube.
        assert_eq!(MobKind::from_u8(15), MobKind::Ghast);
        assert_eq!(MobKind::from_u8(16), MobKind::MagmaCube);
        // W343: 18 = Creeper. W344: 19 = Donkey.
        assert_eq!(MobKind::from_u8(18), MobKind::Creeper);
        assert_eq!(MobKind::from_u8(19), MobKind::Donkey);
        // W330: 17 = ZombieVillager.
        assert_eq!(MobKind::from_u8(17), MobKind::ZombieVillager);
        for raw in [20u8, 100, 255] {
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
