//! W474 - per-kind MC base attributes, GENERATED from the decompiled
//! 26.1.2 source. Do not hand-edit the rows; regenerate instead.
//!
//! Source of truth:
//! - `net/minecraft/world/entity/ai/attributes/DefaultAttributes.java`
//!   (the EntityType -> AttributeSupplier registry) with every
//!   `createAttributes()` chain expanded down to
//!   `LivingEntity.createLivingAttributes()`.
//! - Registry defaults from `Attributes.java`: MAX_HEALTH 20,
//!   MOVEMENT_SPEED 0.7, ATTACK_DAMAGE 2, ARMOR 0.
//!
//! Rows are the REGISTRY BASE values. Kinds whose live stats diverge
//! from the base at spawn keep their special cases outside this
//! table:
//! - Slime / magma cube: `Slime.setSize` overrides MAX_HEALTH to
//!   size^2, MOVEMENT_SPEED to 0.2 + 0.1 * size and ATTACK_DAMAGE to
//!   size (magma cube melee adds +2 via `MagmaCube.getAttackDamage`).
//! - Horse / donkey / mule: `randomizeAttributes` rerolls health via
//!   `generateMaxHealth` (15..30) at spawn - the table's 53 base is
//!   never a live animal's HP, so `initial_health_for` keeps the
//!   spawn-range midpoint.
//! - Villager / wandering trader movement: their stroll goals apply a
//!   speedModifier (0.4 / 0.35) on top of the attribute, which
//!   `kind_walk_speed` folds in.

/// One kind's MC base attribute row.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct MobAttributes {
    /// `Attributes.MAX_HEALTH` base.
    pub(crate) max_health: f32,
    /// `Attributes.MOVEMENT_SPEED` base (blocks/tick modifier scale).
    pub(crate) movement_speed: f32,
    /// `Attributes.ATTACK_DAMAGE` base (melee half-hearts).
    pub(crate) attack_damage: f32,
    /// `Attributes.ARMOR` base (armor points fed to CombatRules).
    pub(crate) armor: f32,
}

/// Indexed by `MobKind` discriminant 0..=50 (kinds.rs order).
pub(crate) const MOB_ATTRIBUTES: [MobAttributes; 51] = [
    //  0 Sheep: Sheep.createAttributes()
    MobAttributes { max_health: 8.0, movement_speed: 0.23, attack_damage: 2.0, armor: 0.0 },
    //  1 Zombie: Zombie.createAttributes()
    MobAttributes { max_health: 20.0, movement_speed: 0.23, attack_damage: 3.0, armor: 2.0 },
    //  2 Chicken: Chicken.createAttributes()
    MobAttributes { max_health: 4.0, movement_speed: 0.25, attack_damage: 2.0, armor: 0.0 },
    //  3 Pig: Pig.createAttributes()
    MobAttributes { max_health: 10.0, movement_speed: 0.25, attack_damage: 2.0, armor: 0.0 },
    //  4 Cow: Cow.createAttributes()
    MobAttributes { max_health: 10.0, movement_speed: 0.2, attack_damage: 2.0, armor: 0.0 },
    //  5 Skeleton: AbstractSkeleton.createAttributes()
    MobAttributes { max_health: 20.0, movement_speed: 0.25, attack_damage: 2.0, armor: 0.0 },
    //  6 Spider: Spider.createAttributes()
    MobAttributes { max_health: 16.0, movement_speed: 0.3, attack_damage: 2.0, armor: 0.0 },
    //  7 Wolf: Wolf.createAttributes()
    MobAttributes { max_health: 8.0, movement_speed: 0.3, attack_damage: 4.0, armor: 0.0 },
    //  8 PolarBear: PolarBear.createAttributes()
    MobAttributes { max_health: 30.0, movement_speed: 0.25, attack_damage: 6.0, armor: 0.0 },
    //  9 Rabbit: Rabbit.createAttributes()
    MobAttributes { max_health: 3.0, movement_speed: 0.3, attack_damage: 3.0, armor: 0.0 },
    // 10 ZombifiedPiglin: ZombifiedPiglin.createAttributes()
    MobAttributes { max_health: 20.0, movement_speed: 0.23, attack_damage: 5.0, armor: 2.0 },
    // 11 Villager: Villager.createAttributes()
    MobAttributes { max_health: 20.0, movement_speed: 0.5, attack_damage: 2.0, armor: 0.0 },
    // 12 Cat: Cat.createAttributes()
    MobAttributes { max_health: 10.0, movement_speed: 0.3, attack_damage: 3.0, armor: 0.0 },
    // 13 IronGolem: IronGolem.createAttributes()
    MobAttributes { max_health: 100.0, movement_speed: 0.25, attack_damage: 15.0, armor: 0.0 },
    // 14 Horse: AbstractHorse.createBaseHorseAttributes()
    MobAttributes { max_health: 53.0, movement_speed: 0.225, attack_damage: 2.0, armor: 0.0 },
    // 15 Ghast: Ghast.createAttributes()
    MobAttributes { max_health: 10.0, movement_speed: 0.7, attack_damage: 2.0, armor: 0.0 },
    // 16 MagmaCube: MagmaCube.createAttributes()
    MobAttributes { max_health: 20.0, movement_speed: 0.2, attack_damage: 2.0, armor: 0.0 },
    // 17 ZombieVillager: Zombie.createAttributes()
    MobAttributes { max_health: 20.0, movement_speed: 0.23, attack_damage: 3.0, armor: 2.0 },
    // 18 Creeper: Creeper.createAttributes()
    MobAttributes { max_health: 20.0, movement_speed: 0.25, attack_damage: 2.0, armor: 0.0 },
    // 19 Donkey: AbstractChestedHorse.createBaseChestedHorseAttributes()
    MobAttributes { max_health: 53.0, movement_speed: 0.175, attack_damage: 2.0, armor: 0.0 },
    // 20 Bee: Bee.createAttributes()
    MobAttributes { max_health: 10.0, movement_speed: 0.3, attack_damage: 2.0, armor: 0.0 },
    // 21 Witch: Witch.createAttributes()
    MobAttributes { max_health: 26.0, movement_speed: 0.25, attack_damage: 2.0, armor: 0.0 },
    // 22 Slime: Monster.createMonsterAttributes()
    MobAttributes { max_health: 20.0, movement_speed: 0.7, attack_damage: 2.0, armor: 0.0 },
    // 23 Drowned: Drowned.createAttributes()
    MobAttributes { max_health: 20.0, movement_speed: 0.23, attack_damage: 3.0, armor: 2.0 },
    // 24 Husk: Zombie.createAttributes()
    MobAttributes { max_health: 20.0, movement_speed: 0.23, attack_damage: 3.0, armor: 2.0 },
    // 25 Stray: AbstractSkeleton.createAttributes()
    MobAttributes { max_health: 20.0, movement_speed: 0.25, attack_damage: 2.0, armor: 0.0 },
    // 26 Pillager: Pillager.createAttributes()
    MobAttributes { max_health: 24.0, movement_speed: 0.35, attack_damage: 5.0, armor: 0.0 },
    // 27 Vindicator: Vindicator.createAttributes()
    MobAttributes { max_health: 24.0, movement_speed: 0.35, attack_damage: 5.0, armor: 0.0 },
    // 28 Evoker: Evoker.createAttributes()
    MobAttributes { max_health: 24.0, movement_speed: 0.5, attack_damage: 2.0, armor: 0.0 },
    // 29 Blaze: Blaze.createAttributes()
    MobAttributes { max_health: 20.0, movement_speed: 0.23, attack_damage: 6.0, armor: 0.0 },
    // 30 WitherSkeleton: AbstractSkeleton.createAttributes()
    MobAttributes { max_health: 20.0, movement_speed: 0.25, attack_damage: 2.0, armor: 0.0 },
    // 31 Guardian: Guardian.createAttributes()
    MobAttributes { max_health: 30.0, movement_speed: 0.5, attack_damage: 6.0, armor: 0.0 },
    // 32 ElderGuardian: ElderGuardian.createAttributes()
    MobAttributes { max_health: 80.0, movement_speed: 0.3, attack_damage: 8.0, armor: 0.0 },
    // 33 Enderman: EnderMan.createAttributes()
    MobAttributes { max_health: 40.0, movement_speed: 0.3, attack_damage: 7.0, armor: 0.0 },
    // 34 CaveSpider: CaveSpider.createCaveSpider()
    MobAttributes { max_health: 12.0, movement_speed: 0.3, attack_damage: 2.0, armor: 0.0 },
    // 35 Silverfish: Silverfish.createAttributes()
    MobAttributes { max_health: 8.0, movement_speed: 0.25, attack_damage: 1.0, armor: 0.0 },
    // 36 Phantom: Monster.createMonsterAttributes()
    MobAttributes { max_health: 20.0, movement_speed: 0.7, attack_damage: 2.0, armor: 0.0 },
    // 37 Bat: Bat.createAttributes()
    MobAttributes { max_health: 6.0, movement_speed: 0.7, attack_damage: 2.0, armor: 0.0 },
    // 38 Mooshroom: Cow.createAttributes()
    MobAttributes { max_health: 10.0, movement_speed: 0.2, attack_damage: 2.0, armor: 0.0 },
    // 39 Squid: Squid.createAttributes()
    MobAttributes { max_health: 10.0, movement_speed: 0.7, attack_damage: 2.0, armor: 0.0 },
    // 40 GlowSquid: GlowSquid.createAttributes()
    MobAttributes { max_health: 10.0, movement_speed: 0.7, attack_damage: 2.0, armor: 0.0 },
    // 41 SkeletonHorse: SkeletonHorse.createAttributes()
    MobAttributes { max_health: 15.0, movement_speed: 0.2, attack_damage: 2.0, armor: 0.0 },
    // 42 Mule: AbstractChestedHorse.createBaseChestedHorseAttributes()
    MobAttributes { max_health: 53.0, movement_speed: 0.175, attack_damage: 2.0, armor: 0.0 },
    // 43 WanderingTrader: Mob.createMobAttributes()
    MobAttributes { max_health: 20.0, movement_speed: 0.7, attack_damage: 2.0, armor: 0.0 },
    // 44 Fox: Fox.createAttributes()
    MobAttributes { max_health: 10.0, movement_speed: 0.3, attack_damage: 2.0, armor: 0.0 },
    // 45 Ocelot: Ocelot.createAttributes()
    MobAttributes { max_health: 10.0, movement_speed: 0.3, attack_damage: 3.0, armor: 0.0 },
    // 46 Parrot: Parrot.createAttributes()
    MobAttributes { max_health: 6.0, movement_speed: 0.2, attack_damage: 3.0, armor: 0.0 },
    // 47 Goat: Goat.createAttributes()
    MobAttributes { max_health: 10.0, movement_speed: 0.2, attack_damage: 2.0, armor: 0.0 },
    // 48 Turtle: Turtle.createAttributes()
    MobAttributes { max_health: 30.0, movement_speed: 0.25, attack_damage: 2.0, armor: 0.0 },
    // 49 Panda: Panda.createAttributes()
    MobAttributes { max_health: 20.0, movement_speed: 0.15, attack_damage: 6.0, armor: 0.0 },
    // 50 Camel: Camel.createAttributes()
    MobAttributes { max_health: 32.0, movement_speed: 0.09, attack_damage: 2.0, armor: 0.0 },
];

/// Attribute row for a kind byte. Unknown bytes collapse to Sheep,
/// mirroring `MobKind::from_u8`.
pub(crate) fn mob_attributes(kind: u8) -> &'static MobAttributes {
    let idx = if (kind as usize) < MOB_ATTRIBUTES.len() {
        kind as usize
    } else {
        0
    };
    &MOB_ATTRIBUTES[idx]
}
