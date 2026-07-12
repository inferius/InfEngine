//! W258 - ruined portal (`RuinedPortalStructure` +
//! `RuinedPortalPiece.java` port) - the overworld half; the nether
//! variant rides the dims epic.
//!
//! Placement (`structure_set/ruined_portals.json`): random_spread
//! spacing 40 / separation 15 / salt 34222645. The set's seven
//! entries carry disjoint biome tags; the overworld six resolve by
//! the start biome (desert / jungle / swamp / mountain / ocean /
//! standard-fallback), each with its JSON setups: vertical placement,
//! air-pocket probability, mossiness, overgrown, vines, can_be_cold.
//!
//! One template piece per start: 5% giant (3 templates) else 1 of 10,
//! random rotation AND a 50% FRONT_BACK mirror around the
//! (sizeX/2, 0, sizeZ/2) pivot - the engine's first mirrored
//! template. Y per placement: surface / partly buried
//! (surface - span + rand[2..8]) / ocean floor / in-mountain
//! (rand[70..surface-span]) / underground (rand[15..surface-span]),
//! then the findSuitableY scan walks down until three of the four
//! bottom corners rest on solid ground (post-carve solidity oracle).
//!
//! Carve processors, pure per world cell: gold blocks vanish 30%;
//! lava turns magma (ocean floor: always; else 20%) or netherrack
//! when cold; netherrack turns magma 7% when warm; the
//! BlockAgeProcessor is APPROXIMATED as the mossiness-weighted
//! stone-family swap (stone bricks -> mossy, cobble -> mossy,
//! stairs/slabs -> mossy variants) - the full age matrix is a
//! follow-up. Air cells write only when the setup rolled an AIR
//! POCKET (BlockIgnoreProcessor::STRUCTURE_BLOCK vs _AND_AIR).
//!
//! Post passes: the netherrack apron spreads by the Java
//! manhattan-distance probability table (surface-following, +-3 y of
//! the box floor), drip columns hang below (50% chain, cap 8), vines
//! crawl onto solid faces and the jungle's overgrown flag sprinkles
//! persistent jungle leaves. Chest loot: RUINED_PORTAL via bb
//! membership on the gen worker.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use infengine_voxel::{CHUNK_SIZE, Chunk, ChunkCoord};

use super::biome::McBiome;
use super::blockmap::{Mapped, map_state, registry_id};
use super::features::stream_seed;
use super::mineshaft::{Bb, ChunkShaftWorld, ShaftWorld};
use super::noise::SplitMix64;
use super::scattered::random_spread_start;
use super::terrain::{column_is_solid, mc_floor_at, mc_full_biome_at};
use crate::worldgen::WorldgenCtx;

/// `structure_set/ruined_portals.json`.
const SPACING: i32 = 40;
const SEPARATION: i32 = 15;
const SALT_PORTAL_SET: u64 = 34_222_645;
const SALT_PORTAL_LAYOUT: u64 = 0xD4E6_00C0;
const SALT_PORTAL_CELL: u64 = 0xD4E6_00C1;
const SALT_PORTAL_APRON: u64 = 0xD4E6_00C2;

const SMALL: &[&str] = &[
    "minecraft:ruined_portal/portal_1",
    "minecraft:ruined_portal/portal_2",
    "minecraft:ruined_portal/portal_3",
    "minecraft:ruined_portal/portal_4",
    "minecraft:ruined_portal/portal_5",
    "minecraft:ruined_portal/portal_6",
    "minecraft:ruined_portal/portal_7",
    "minecraft:ruined_portal/portal_8",
    "minecraft:ruined_portal/portal_9",
    "minecraft:ruined_portal/portal_10",
];
const GIANT: &[&str] = &[
    "minecraft:ruined_portal/giant_portal_1",
    "minecraft:ruined_portal/giant_portal_2",
    "minecraft:ruined_portal/giant_portal_3",
];

fn next_int(r: &mut SplitMix64, n: i32) -> i32 {
    (r.next_u64() % n.max(1) as u64) as i32
}

fn next_f32(r: &mut SplitMix64) -> f32 {
    (r.next_u64() >> 40) as f32 / (1u64 << 24) as f32
}

fn range_inc(r: &mut SplitMix64, a: i32, b: i32) -> i32 {
    a + next_int(r, (b - a + 1).max(1))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Place {
    OnLandSurface,
    PartlyBuried,
    OnOceanFloor,
    InMountain,
    Underground,
}

#[derive(Debug, Clone, Copy)]
struct Setup {
    place: Place,
    air_pocket_p: f32,
    mossiness: f32,
    overgrown: bool,
    vines: bool,
    can_be_cold: bool,
    weight: f32,
}

/// The six overworld variants' setups, verbatim from the JSONs.
fn setups_for(biome: McBiome) -> &'static [Setup] {
    const STANDARD: &[Setup] = &[
        Setup {
            place: Place::Underground,
            air_pocket_p: 1.0,
            mossiness: 0.2,
            overgrown: false,
            vines: false,
            can_be_cold: true,
            weight: 0.5,
        },
        Setup {
            place: Place::OnLandSurface,
            air_pocket_p: 0.5,
            mossiness: 0.2,
            overgrown: false,
            vines: false,
            can_be_cold: true,
            weight: 0.5,
        },
    ];
    const DESERT: &[Setup] = &[Setup {
        place: Place::PartlyBuried,
        air_pocket_p: 0.0,
        mossiness: 0.0,
        overgrown: false,
        vines: false,
        can_be_cold: false,
        weight: 1.0,
    }];
    const JUNGLE: &[Setup] = &[Setup {
        place: Place::OnLandSurface,
        air_pocket_p: 0.5,
        mossiness: 0.8,
        overgrown: true,
        vines: true,
        can_be_cold: false,
        weight: 1.0,
    }];
    const SWAMP: &[Setup] = &[Setup {
        place: Place::OnOceanFloor,
        air_pocket_p: 0.0,
        mossiness: 0.5,
        overgrown: false,
        vines: true,
        can_be_cold: false,
        weight: 1.0,
    }];
    const MOUNTAIN: &[Setup] = &[
        Setup {
            place: Place::InMountain,
            air_pocket_p: 1.0,
            mossiness: 0.2,
            overgrown: false,
            vines: false,
            can_be_cold: true,
            weight: 0.5,
        },
        Setup {
            place: Place::OnLandSurface,
            air_pocket_p: 0.5,
            mossiness: 0.2,
            overgrown: false,
            vines: false,
            can_be_cold: true,
            weight: 0.5,
        },
    ];
    const OCEAN: &[Setup] = &[Setup {
        place: Place::OnOceanFloor,
        air_pocket_p: 0.0,
        mossiness: 0.8,
        overgrown: false,
        vines: false,
        can_be_cold: true,
        weight: 1.0,
    }];
    match biome {
        McBiome::Desert => DESERT,
        McBiome::Jungle | McBiome::SparseJungle | McBiome::BambooJungle => JUNGLE,
        McBiome::Swamp | McBiome::MangroveSwamp => SWAMP,
        McBiome::Badlands
        | McBiome::WoodedBadlands
        | McBiome::ErodedBadlands
        | McBiome::WindsweptHills
        | McBiome::WindsweptGravellyHills
        | McBiome::WindsweptForest
        | McBiome::WindsweptSavanna
        | McBiome::JaggedPeaks
        | McBiome::FrozenPeaks
        | McBiome::StonyPeaks
        | McBiome::SnowySlopes
        | McBiome::Meadow
        | McBiome::Grove
        | McBiome::CherryGrove
        | McBiome::SavannaPlateau => MOUNTAIN,
        McBiome::DeepFrozenOcean
        | McBiome::DeepColdOcean
        | McBiome::DeepOcean
        | McBiome::DeepLukewarmOcean
        | McBiome::WarmOcean
        | McBiome::FrozenOcean
        | McBiome::ColdOcean
        | McBiome::Ocean
        | McBiome::LukewarmOcean => OCEAN,
        _ => STANDARD,
    }
}

/// `Mirror.FRONT_BACK` flips local z, then the rotation turns around
/// the (sx/2, 0, sz/2) pivot (`StructureTemplate.transform`).
fn transform(p: [i32; 3], size: [i32; 3], rot: u8, mirror: bool) -> [i32; 3] {
    let p = if mirror { [p[0], p[1], size[2] - 1 - p[2]] } else { p };
    let (px, pz) = (size[0] / 2, size[2] / 2);
    match rot % 4 {
        0 => p,
        1 => [px + pz - p[2], p[1], pz - px + p[0]],
        2 => [px + px - p[0], p[1], pz + pz - p[2]],
        _ => [px - pz + p[2], p[1], px + pz - p[0]],
    }
}

#[derive(Debug, Clone)]
pub(crate) struct RuinedPortal {
    pub(crate) template: &'static str,
    pub(crate) position: [i32; 3],
    pub(crate) rotation: u8,
    pub(crate) mirror: bool,
    pub(crate) hull: Bb,
    place: Place,
    air_pocket: bool,
    mossiness: f32,
    overgrown: bool,
    vines: bool,
    cold: bool,
    /// `spreadNetherrack`'s layout-time distance adjustment.
    apron_adjust: i32,
}

pub(crate) fn layout_for_start(
    ctx: &WorldgenCtx,
    scx: i32,
    scz: i32,
) -> Option<Rc<RuinedPortal>> {
    type Cache = HashMap<(u32, i32, i32), Option<Rc<RuinedPortal>>>;
    thread_local! {
        static CACHE: RefCell<Cache> = RefCell::new(HashMap::new());
    }
    let seed = ctx.effective_seed();
    if let Some(hit) = CACHE.with(|c| c.borrow().get(&(seed, scx, scz)).cloned()) {
        return hit;
    }
    let built = build_layout(ctx, scx, scz).map(Rc::new);
    CACHE.with(|c| {
        let mut map = c.borrow_mut();
        if map.len() > 256 {
            map.clear();
        }
        map.insert((seed, scx, scz), built.clone());
    });
    built
}

fn build_layout(ctx: &WorldgenCtx, scx: i32, scz: i32) -> Option<RuinedPortal> {
    let cs = CHUNK_SIZE as i32;
    let west = scx * cs;
    let north = scz * cs;
    let biome = mc_full_biome_at(ctx, west + 8, north + 8);
    let setups = setups_for(biome);
    let seed = ctx.effective_seed();
    let mut rng =
        SplitMix64::new(stream_seed(seed, scx, scz, SALT_PORTAL_LAYOUT));
    // Setup pick (weighted when the variant carries two).
    let setup = if setups.len() > 1 {
        let total: f32 = setups.iter().map(|s| s.weight).sum();
        let mut pick = next_f32(&mut rng);
        let mut chosen = setups[0];
        for s in setups {
            pick -= s.weight / total;
            if pick < 0.0 {
                chosen = *s;
                break;
            }
        }
        chosen
    } else {
        setups[0]
    };
    let air_pocket = match setup.air_pocket_p {
        p if p <= 0.0 => false,
        p if p >= 1.0 => true,
        p => next_f32(&mut rng) < p,
    };
    let template = if next_f32(&mut rng) < 0.05 {
        GIANT[next_int(&mut rng, GIANT.len() as i32) as usize]
    } else {
        SMALL[next_int(&mut rng, SMALL.len() as i32) as usize]
    };
    let t = super::village_data::template(template)?;
    let rotation = next_int(&mut rng, 4) as u8;
    let mirror = next_f32(&mut rng) < 0.5;
    // Rotated/mirrored corners around the pivot -> footprint.
    let a = transform([0, 0, 0], t.size, rotation, mirror);
    let b = transform([t.size[0] - 1, t.size[1] - 1, t.size[2] - 1], t.size, rotation, mirror);
    let (bx0, bx1) = (a[0].min(b[0]), a[0].max(b[0]));
    let (bz0, bz1) = (a[2].min(b[2]), a[2].max(b[2]));
    let centre = [west + (bx0 + bx1) / 2, north + (bz0 + bz1) / 2];
    let surface_y = mc_floor_at(ctx, centre[0], centre[1]);
    let span = t.size[1];
    let new_y = match setup.place {
        Place::OnLandSurface | Place::OnOceanFloor => surface_y,
        Place::PartlyBuried => surface_y - span + range_inc(&mut rng, 2, 8),
        Place::InMountain => {
            let max = surface_y - span;
            if 70 < max { range_inc(&mut rng, 70, max) } else { max }
        }
        Place::Underground => {
            let max = surface_y - span;
            if 15 < max { range_inc(&mut rng, 15, max) } else { max }
        }
    };
    // findSuitableY: walk down until 3 of 4 corners rest on solid.
    let corners = [
        [west + bx0, north + bz0],
        [west + bx1, north + bz0],
        [west + bx0, north + bz1],
        [west + bx1, north + bz1],
    ];
    let mut y = new_y.clamp(16, 250);
    while y > 15 {
        let solid = corners
            .iter()
            .filter(|c| column_is_solid(ctx, c[0], y, c[1]))
            .count();
        if solid >= 3 {
            break;
        }
        y -= 1;
    }
    let position = [west, y, north];
    let hull = Bb {
        x0: west + bx0,
        y0: y,
        z0: north + bz0,
        x1: west + bx1,
        y1: y + span - 1,
        z1: north + bz1,
    };
    let avg_width = ((hull.x1 - hull.x0 + 1) + (hull.z1 - hull.z0 + 1)) / 2;
    let apron_adjust = next_int(&mut rng, (8 - avg_width / 2).max(1));
    let cold = setup.can_be_cold
        && super::vegetation::cold_enough_to_snow_at(biome, centre[0], y, centre[1]);
    Some(RuinedPortal {
        template,
        position,
        rotation,
        mirror,
        hull,
        place: setup.place,
        air_pocket,
        mossiness: setup.mossiness,
        overgrown: setup.overgrown,
        vines: setup.vines,
        cold,
        apron_adjust,
    })
}

/// Pure per-cell roll in [0, 1).
fn cell_roll(seed: u32, pos: [i32; 3], salt: u64) -> f32 {
    let s = stream_seed(seed, pos[0], pos[2], salt)
        ^ (pos[1] as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    (SplitMix64::new(s).next_u64() >> 40) as f32 / (1u64 << 24) as f32
}

/// The apron probability table (`netherrackProbabilityByDistance`).
const APRON_P: [f32; 14] =
    [1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 0.9, 0.9, 0.8, 0.7, 0.6, 0.4, 0.2];

struct Ids {
    gold: u16,
    netherrack: u16,
    magma: u16,
    lava: u16,
    vine: u16,
    leaves: u16,
    obsidian: u16,
    stone_bricks: u16,
    mossy_bricks: u16,
    cobble: u16,
    mossy_cobble: u16,
    brick_stairs: u16,
    mossy_brick_stairs: u16,
    brick_slab: u16,
    mossy_brick_slab: u16,
}

impl Ids {
    fn get() -> Ids {
        Ids {
            gold: registry_id("gold_block").unwrap_or(0),
            netherrack: registry_id("netherrack").unwrap_or(1),
            magma: registry_id("magma_block").unwrap_or(1),
            lava: crate::block::LAVA_ID,
            vine: registry_id("vine").unwrap_or(0),
            leaves: registry_id("jungle_leaves")
                .or_else(|| registry_id("leaves"))
                .unwrap_or(0),
            obsidian: registry_id("obsidian").unwrap_or(1),
            stone_bricks: registry_id("stone_bricks").unwrap_or(1),
            mossy_bricks: registry_id("mossy_stone_bricks").unwrap_or(1),
            cobble: crate::block::COBBLE_ID,
            mossy_cobble: registry_id("mossy_cobblestone").unwrap_or(1),
            brick_stairs: registry_id("stone_brick_stairs").unwrap_or(1),
            mossy_brick_stairs: registry_id("mossy_stone_brick_stairs").unwrap_or(1),
            brick_slab: registry_id("stone_brick_slab").unwrap_or(1),
            mossy_brick_slab: registry_id("mossy_stone_brick_slab").unwrap_or(1),
        }
    }
}

/// Grid reach: giant template ~13 + the 13-block apron.
const REACH: i32 = 3;

/// The per-chunk ruined-portal pass (surface_structures).
pub(crate) fn place_ruined_portals(
    chunk: &mut Chunk,
    coord: ChunkCoord,
    ctx: &WorldgenCtx,
    origin: [i32; 3],
) {
    let seed = ctx.effective_seed();
    let cs = CHUNK_SIZE as i32;
    for gx in (coord[0] - REACH).div_euclid(SPACING)..=(coord[0] + REACH).div_euclid(SPACING)
    {
        for gz in
            (coord[2] - REACH).div_euclid(SPACING)..=(coord[2] + REACH).div_euclid(SPACING)
        {
            let [scx, scz] =
                random_spread_start(seed, gx, gz, SPACING, SEPARATION, SALT_PORTAL_SET);
            if (scx - coord[0]).abs() > REACH || (scz - coord[2]).abs() > REACH {
                continue;
            }
            let Some(p) = layout_for_start(ctx, scx, scz) else {
                continue;
            };
            let slack = Bb {
                x0: p.hull.x0 - 16,
                y0: 0,
                z0: p.hull.z0 - 16,
                x1: p.hull.x1 + 16,
                y1: 255,
                z1: p.hull.z1 + 16,
            };
            let cube = Bb {
                x0: origin[0],
                y0: origin[1],
                z0: origin[2],
                x1: origin[0] + cs - 1,
                y1: origin[1] + cs - 1,
                z1: origin[2] + cs - 1,
            };
            if !slack.intersects(&cube) {
                continue;
            }
            let mut world = ChunkShaftWorld { chunk, origin, ctx };
            carve(&mut world, ctx, seed, &p);
        }
    }
}

fn carve(w: &mut dyn ShaftWorld, ctx: &WorldgenCtx, seed: u32, p: &RuinedPortal) {
    let Some(t) = super::village_data::template(p.template) else {
        return;
    };
    let ids = Ids::get();
    let (cx0, cz0, cx1, cz1) = w.column();
    let in_cube = |pos: [i32; 3]| {
        pos[0] >= cx0
            && pos[0] <= cx1
            && pos[2] >= cz0
            && pos[2] <= cz1
            && (-64..320).contains(&pos[1])
    };
    // Template cells with the processor chain.
    for cell in &t.cells {
        let lp = transform(cell.pos, t.size, p.rotation, p.mirror);
        let wp = [
            p.position[0] + lp[0],
            p.position[1] + lp[1],
            p.position[2] + lp[2],
        ];
        if !in_cube(wp) {
            continue;
        }
        let state = &t.palette[cell.state];
        let name = state.name.strip_prefix("minecraft:").unwrap_or(&state.name);
        if name == "air" {
            // STRUCTURE_AND_AIR skips air unless the air pocket
            // survived the roll.
            if p.air_pocket {
                w.place(wp, 0);
            }
            continue;
        }
        let roll = cell_roll(seed, wp, SALT_PORTAL_CELL);
        let mapped = match name {
            "gold_block" if roll < 0.3 => Some(0),
            "lava" => Some(match p.place {
                Place::OnOceanFloor => ids.magma,
                _ if p.cold => ids.netherrack,
                _ if roll < 0.2 => ids.magma,
                _ => ids.lava,
            }),
            "netherrack" if !p.cold && roll < 0.07 => Some(ids.magma),
            // The BlockAgeProcessor approximation: the stone family
            // mosses over at the setup's mossiness.
            "stone_bricks" if roll < p.mossiness => Some(ids.mossy_bricks),
            "cobblestone" if roll < p.mossiness => Some(ids.mossy_cobble),
            "stone_brick_stairs" if roll < p.mossiness => Some(ids.mossy_brick_stairs),
            "stone_brick_slab" if roll < p.mossiness => Some(ids.mossy_brick_slab),
            _ => None,
        };
        let id = match mapped {
            Some(id) => id,
            None => match map_state(state) {
                Mapped::Id(id) => id,
                Mapped::Keep => continue,
            },
        };
        w.place(wp, id);
    }
    // spreadNetherrack: the surface apron.
    let centre = [
        (p.hull.x0 + p.hull.x1) / 2,
        (p.hull.y0 + p.hull.y1) / 2,
        (p.hull.z0 + p.hull.z1) / 2,
    ];
    let follow_surface =
        matches!(p.place, Place::OnLandSurface | Place::OnOceanFloor);
    let max_d = APRON_P.len() as i32;
    for x in (centre[0] - max_d)..=(centre[0] + max_d) {
        for z in (centre[2] - max_d)..=(centre[2] + max_d) {
            if x < cx0 || x > cx1 || z < cz0 || z > cz1 {
                continue;
            }
            let d = (x - centre[0]).abs() + (z - centre[2]).abs();
            let ad = (d + p.apron_adjust).max(0);
            if ad >= max_d {
                continue;
            }
            let pr = APRON_P[ad as usize];
            if cell_roll(seed, [x, 0, z], SALT_PORTAL_APRON) >= pr {
                continue;
            }
            let surface = mc_floor_at(ctx, x, z);
            let y = if follow_surface { surface } else { p.hull.y0.min(surface) };
            if (y - p.hull.y0).abs() > 3 || !(1..255).contains(&y) {
                continue;
            }
            let pos = [x, y, z];
            let cur = w.read(pos);
            if cur == 0 || cur == ids.obsidian || cur == ids.lava {
                continue;
            }
            place_netherrack(w, seed, &ids, p, pos);
            if p.overgrown
                && cell_roll(seed, [x, y + 1, z], SALT_PORTAL_APRON) < 0.5
                && w.read([x, y + 1, z]) == 0
            {
                w.place([x, y + 1, z], ids.leaves);
            }
            drip_column(w, seed, &ids, p, [x, y - 1, z]);
        }
    }
    // Drip columns straight below the template floor's netherrack.
    for x in (p.hull.x0 + 1)..p.hull.x1 {
        for z in (p.hull.z0 + 1)..p.hull.z1 {
            if x < cx0 || x > cx1 || z < cz0 || z > cz1 {
                continue;
            }
            if w.read([x, p.hull.y0, z]) == ids.netherrack {
                drip_column(w, seed, &ids, p, [x, p.hull.y0 - 1, z]);
            }
        }
    }
    // Vines + overgrown leaves over the box.
    if p.vines || p.overgrown {
        for x in p.hull.x0..=p.hull.x1 {
            for z in p.hull.z0..=p.hull.z1 {
                if x < cx0 || x > cx1 || z < cz0 || z > cz1 {
                    continue;
                }
                for y in p.hull.y0..=p.hull.y1 {
                    let pos = [x, y, z];
                    let cur = w.read(pos);
                    if p.vines && cur != 0 && cur != ids.vine {
                        // One random horizontal face tries a vine.
                        let r = cell_roll(seed, pos, SALT_PORTAL_CELL ^ 0x51);
                        let dirs = [[1, 0], [-1, 0], [0, 1], [0, -1]];
                        let d = dirs[(r * 4.0) as usize % 4];
                        let n = [x + d[0], y, z + d[1]];
                        if n[0] >= cx0
                            && n[0] <= cx1
                            && n[2] >= cz0
                            && n[2] <= cz1
                            && w.read(n) == 0
                        {
                            w.place(n, ids.vine);
                        }
                    }
                    if p.overgrown
                        && cur == ids.netherrack
                        && y < 319
                        && w.read([x, y + 1, z]) == 0
                        && cell_roll(seed, [x, y + 1, z], SALT_PORTAL_CELL ^ 0x52) < 0.5
                    {
                        w.place([x, y + 1, z], ids.leaves);
                    }
                }
            }
        }
    }
}

fn place_netherrack(
    w: &mut dyn ShaftWorld,
    seed: u32,
    ids: &Ids,
    p: &RuinedPortal,
    pos: [i32; 3],
) {
    let id = if !p.cold && cell_roll(seed, pos, SALT_PORTAL_APRON ^ 0x7) < 0.07 {
        ids.magma
    } else {
        ids.netherrack
    };
    w.place(pos, id);
}

/// `addNetherrackDripColumn`: 50% chain, cap 8, pure per cell.
fn drip_column(
    w: &mut dyn ShaftWorld,
    seed: u32,
    ids: &Ids,
    p: &RuinedPortal,
    top: [i32; 3],
) {
    let mut pos = top;
    if pos[1] < 1 {
        return;
    }
    place_netherrack(w, seed, ids, p, pos);
    let mut cap = 8;
    while cap > 0
        && pos[1] > 1
        && cell_roll(seed, pos, SALT_PORTAL_APRON ^ 0x9) < 0.5
    {
        pos[1] -= 1;
        cap -= 1;
        place_netherrack(w, seed, ids, p, pos);
    }
}

/// Chest membership for the RUINED_PORTAL loot pick (worker-side).
pub(crate) fn pos_in_ruined_portal(ctx: &WorldgenCtx, pos: [i32; 3]) -> bool {
    if !matches!(ctx.profile, crate::worldgen::WorldGen::Mc) {
        return false;
    }
    let cs = CHUNK_SIZE as i32;
    let (ccx, ccz) = (pos[0].div_euclid(cs), pos[2].div_euclid(cs));
    for gx in (ccx - REACH).div_euclid(SPACING)..=(ccx + REACH).div_euclid(SPACING) {
        for gz in (ccz - REACH).div_euclid(SPACING)..=(ccz + REACH).div_euclid(SPACING) {
            let [scx, scz] = random_spread_start(
                ctx.effective_seed(),
                gx,
                gz,
                SPACING,
                SEPARATION,
                SALT_PORTAL_SET,
            );
            if let Some(p) = layout_for_start(ctx, scx, scz)
                && p.hull.contains(pos)
            {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transform_mirrors_and_rotates_around_the_pivot() {
        let size = [7, 5, 9];
        // No-op transform keeps the cell.
        assert_eq!(transform([2, 1, 3], size, 0, false), [2, 1, 3]);
        // Mirror flips z.
        assert_eq!(transform([2, 1, 3], size, 0, true), [2, 1, 5]);
        // The pivot cell is a fixed point of every rotation.
        for r in 0..4 {
            assert_eq!(
                transform([3, 0, 4], size, r, false),
                [3, 0, 4],
                "pivot fixed under rot {r}"
            );
        }
        // Corners stay within a normalized footprint.
        for r in 0..4 {
            for m in [false, true] {
                let a = transform([0, 0, 0], size, r, m);
                let b = transform([6, 4, 8], size, r, m);
                let w = (a[0] - b[0]).abs() + 1;
                let d = (a[2] - b[2]).abs() + 1;
                assert!(
                    (w == 7 && d == 9) || (w == 9 && d == 7),
                    "footprint {w}x{d} under rot {r} mirror {m}"
                );
            }
        }
    }

    #[test]
    fn every_biome_resolves_a_setup_list() {
        for b in McBiome::ALL {
            assert!(!setups_for(b).is_empty());
        }
    }

    /// Probe: ruined portals near the test region + full-pipeline
    /// verify (obsidian frame + netherrack apron + the chest).
    /// `INFENGINE_DUMP_SEED=<seed> INFENGINE_DUMP_CX/CZ=<chunk>
    /// INFENGINE_DUMP_RADIUS=<r> cargo test -p voxel_demo_mp
    /// --release --lib dump_ruined_portals -- --ignored --nocapture`
    #[test]
    #[ignore]
    #[allow(clippy::print_stdout)]
    fn dump_ruined_portals() {
        let env_i32 = |name: &str, default: i32| -> i32 {
            std::env::var(name)
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(default)
        };
        let seed: u32 = std::env::var("INFENGINE_DUMP_SEED")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(3592732596);
        let radius = env_i32("INFENGINE_DUMP_RADIUS", 96);
        let (ccx, ccz) =
            (env_i32("INFENGINE_DUMP_CX", 296), env_i32("INFENGINE_DUMP_CZ", 295));
        let ctx = crate::worldgen::WorldgenCtx {
            base_seed: seed,
            dim: std::env::var("INFENGINE_DUMP_DIM")
                .unwrap_or_else(|_| "overworld".into()),
            profile: crate::worldgen::WorldGen::Mc,
        };
        let cs = CHUNK_SIZE as i32;
        let mut found: Vec<Rc<RuinedPortal>> = Vec::new();
        for gx in (ccx - radius).div_euclid(SPACING)..=(ccx + radius).div_euclid(SPACING)
        {
            for gz in
                (ccz - radius).div_euclid(SPACING)..=(ccz + radius).div_euclid(SPACING)
            {
                let [sx, sz] = random_spread_start(
                    ctx.effective_seed(),
                    gx,
                    gz,
                    SPACING,
                    SEPARATION,
                    SALT_PORTAL_SET,
                );
                if let Some(p) = layout_for_start(&ctx, sx, sz) {
                    println!(
                        "ruined portal at ({sx}, {sz}): {} rot{} mirror {} {:?} hull {:?}",
                        p.template, p.rotation, p.mirror, p.place, p.hull
                    );
                    found.push(p);
                }
            }
        }
        println!("total: {} ruined portals in +-{radius} chunks", found.len());
        if let Some(p) = found.first() {
            let cx = (p.hull.x0 + p.hull.x1) / 2;
            let cy = (p.hull.y0 + p.hull.y1) / 2;
            let cz = (p.hull.z0 + p.hull.z1) / 2;
            let coord = [cx.div_euclid(cs), cy.div_euclid(cs), cz.div_euclid(cs)];
            let chunk = crate::worldgen::generate_chunk_at_ctx(coord, &ctx);
            let obsidian = registry_id("obsidian").unwrap();
            let crying = registry_id("crying_obsidian").unwrap();
            let nrack = registry_id("netherrack").unwrap();
            let (mut n_obs, mut n_rack) = (0usize, 0usize);
            for lx in 0..CHUNK_SIZE {
                for ly in 0..CHUNK_SIZE {
                    for lz in 0..CHUNK_SIZE {
                        let id = chunk.get(lx, ly, lz).0;
                        if id == obsidian || id == crying {
                            n_obs += 1;
                        } else if id == nrack {
                            n_rack += 1;
                        }
                    }
                }
            }
            println!(
                "full-pipeline portal cube {coord:?}: {n_obs} obsidian, {n_rack} netherrack"
            );
            assert!(n_obs > 0, "the obsidian frame must materialise");
            assert!(
                pos_in_ruined_portal(&ctx, [cx, cy, cz]),
                "membership re-derives"
            );
        }
    }
}
