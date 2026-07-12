//! W181 - faithful Minecraft (1.18+) overworld worldgen port.
//!
//! The legacy `Natural` / `Realistic` profiles are a custom fBm-height
//! plus 3-channel-climate approximation: they get the rough shape but
//! not MC's algorithm, so the terrain reads as "off" next to a real MC
//! world (the user's repeated, fair complaint). This module ports the
//! actual MC pipeline, data-driven from the JSON the game ships under
//! `data/minecraft/worldgen/` (extracted to `tools/mc_worldgen/` for
//! reference). It is NOT seed-identical to MC (that needs MC's exact
//! Xoroshiro seeding); it reproduces the LOOK via the same algorithm
//! and the same data.
//!
//! Build phases, each its own committed + green wave.
//! Phase 1 (this wave) NOISE: ImprovedNoise / PerlinNoise / NormalNoise.
//! Phase 2 DENSITY FUNCTIONS: the density_function DSL evaluator that
//! drives `final_density`.
//! Phase 3 TERRAIN: sample final_density + aquifers into stone/water as
//! a new `WorldGen` profile.
//! Phase 4 BIOMES: the 6-channel multi-noise climate + the overworld
//! parameter-list lookup (66 biomes).
//! Phase 5 SURFACE: the `surface_rule` DSL (grass / sand / gravel).
//! Phase 6 FEATURES: placed_features (trees, plants, ores, and
//! kelp/seagrass so oceans aren't dead).
//!
//! `allow(dead_code)`: this module is built across several waves; the
//! noise primitives land first (wave 1) and are consumed by the
//! density-function evaluator (wave 2). The allow is removed once the
//! pipeline wires everything up.
#![allow(dead_code)]

pub(crate) mod aquatic_features;
pub(crate) mod aquifer;
pub(crate) mod bamboo;
pub(crate) mod biome;
pub(crate) mod blockmap;
pub(crate) mod buried_treasure;
pub(crate) mod carver;
pub(crate) mod cave_features;
pub(crate) mod colors;
pub(crate) mod coral;
pub(crate) mod data;
pub(crate) mod density;
pub(crate) mod features;
pub(crate) mod geode;
pub(crate) mod iceberg;
pub(crate) mod igloo;
pub(crate) mod jigsaw;
pub(crate) mod jungle_temple;
pub(crate) mod mineshaft;
pub(crate) mod misc_features;
pub(crate) mod nbt;
pub(crate) mod noise;
pub(crate) mod ocean_ruin;
pub(crate) mod ore_vein;
pub(crate) mod outpost;
pub(crate) mod overworld;
pub(crate) mod pyramid;
pub(crate) mod scattered;
pub(crate) mod shipwreck;
pub(crate) mod stronghold;
pub(crate) mod structures;
pub(crate) mod swamp_hut;
pub(crate) mod surface_rule;
pub(crate) mod template;
pub(crate) mod terrain;
pub(crate) mod trees;
pub(crate) mod vegetation;
pub(crate) mod village;
pub(crate) mod village_data;
pub(crate) mod vines;
