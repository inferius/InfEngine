//! W79 - unified decoration driver for the modern worldgen profiles
//! (Realistic + Natural). One entry point runs every decoration pass in
//! a fixed order so the per-profile `gen_*` fns don't each have to list
//! them, and so new passes (ground cover, biome features) plug in here
//! once for both profiles.
//!
//! Each pass is a sibling module (`trees`, `flowers`, `cactus`, ...);
//! `decor` only orchestrates. The Classic / Flat / Islands profiles keep
//! their own (legacy / bare) decoration paths - those use a different
//! biome system (`biome_at` vs the climate table) so they are not routed
//! through here.

use infengine_voxel::{Chunk, ChunkCoord};

use super::{WorldgenCtx, biome_features, cactus, flowers, ground_cover, mcgen, trees};

/// Run every decoration pass on a freshly carved chunk. Order matters:
/// trees first (they own the vertical space + cast the canopy), then the
/// surface-level passes (flowers, cactus) on whatever ground is left.
pub(crate) fn place_realistic(chunk: &mut Chunk, coord: ChunkCoord, ctx: &WorldgenCtx) {
    trees::place_realistic_from_coord(chunk, coord, ctx);
    flowers::place_realistic_from_coord(chunk, coord, ctx);
    // W82: biome features (dead bush / bamboo / sugar cane) before
    // ground cover so a thicket isn't blocked by a grass tuft.
    biome_features::place_realistic_from_coord(chunk, coord, ctx);
    // W80: ground cover after flowers so flowers win their cells and
    // grass/ferns fill the rest of the surface.
    ground_cover::place_realistic_from_coord(chunk, coord, ctx);
    cactus::place_realistic_from_coord(chunk, coord, ctx);
    // W201: MC ocean flora (kelp + seagrass) from the real MC
    // placed_feature rules. Internally gated to the Mc profile.
    mcgen::features::place_from_coord(chunk, coord, ctx);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// W310: headless sub-pass bench - run with
    /// `cargo test -p voxel_demo_mp --release --lib deco_subpass_bench -- --ignored --nocapture`.
    /// Times each decoration sub-pass over a spread of surface chunks
    /// (on clones of already-generated chunks, so re-placing is fine).
    #[test]
    #[ignore]
    #[allow(clippy::print_stdout)]
    fn deco_subpass_bench() {
        let ctx = crate::worldgen::WorldgenCtx {
            base_seed: 3592732596,
            dim: "overworld".into(),
            profile: crate::worldgen::WorldGen::Mc,
        };
        // Cold full-column gen timing per vertical band first.
        let mut band = std::collections::BTreeMap::<i32, (f64, u32)>::new();
        let t_all = std::time::Instant::now();
        for cx in 0..4 {
            for cz in 0..4 {
                for cy in -4..20 {
                    let coord = [40 + cx * 3, cy, 40 + cz * 3];
                    let t0 = std::time::Instant::now();
                    let _c = crate::worldgen::generate_chunk_at_ctx(coord, &ctx);
                    let e = band.entry(cy).or_insert((0.0, 0));
                    e.0 += t0.elapsed().as_secs_f64();
                    e.1 += 1;
                }
            }
        }
        println!("cold full-column: {:.1} ms/chunk over {} chunks",
            t_all.elapsed().as_secs_f64() * 1000.0 / (16.0 * 24.0), 16 * 24);
        for (cy, (t, n)) in &band {
            println!("  cy {cy:>3}: {:.2} ms/chunk", t * 1000.0 / *n as f64);
        }
        // Per-callee bisect at a FRESH area (cold caches).
        use crate::worldgen::mcgen as mg;
        let mut area_no = 0i32;
        macro_rules! bench {
            ($name:literal, $f:path, $cs:expr) => {{
                // Each placer gets its OWN virgin area so its layout
                // caches are cold - warm re-runs hid the real cost.
                area_no += 1;
                let base = 500 + area_no * 40;
                let mut n = 0u32;
                let t0 = std::time::Instant::now();
                for cx in 0..3i32 {
                    for cz in 0..3i32 {
                        for cy in [-4i32, -2, 0, 2, 4] {
                            let coord = [base + cx * 3, cy, base + cz * 3];
                            let mut ch = infengine_voxel::Chunk::empty();
                            let origin_v =
                                infengine_voxel::World::chunk_world_origin(coord);
                            let origin = [
                                origin_v.x as i32,
                                origin_v.y as i32,
                                origin_v.z as i32,
                            ];
                            $f(&mut ch, coord, &ctx, origin);
                            n += 1;
                        }
                    }
                }
                let ms = t0.elapsed().as_secs_f64() * 1000.0 / n as f64;
                if ms > 0.2 {
                    println!("  {}: {:.2} ms/chunk", $name, ms);
                }
            }};
        }

        println!("per-structure (45 cold-area chunks, >0.2ms only):");
        bench!("lava_lakes", mg::misc_features::place_lava_lakes, cold);
        bench!("geodes", mg::geode::place_geodes, cold);
        bench!("icebergs", mg::iceberg::place_icebergs, cold);
        bench!("forest_rocks", mg::misc_features::place_forest_rocks, cold);
        bench!("mineshafts", mg::mineshaft::place_mineshafts, cold);
        bench!("buried_treasure", mg::buried_treasure::place_buried_treasure, cold);
        bench!("strongholds", mg::stronghold::place_strongholds, cold);
        bench!("trial_chambers", mg::trial_chambers::place_trial_chambers, cold);
        bench!("monster_rooms", mg::structures::place_monster_rooms, cold);
        bench!("villages", mg::village::place_villages, cold);
        bench!("outposts", mg::outpost::place_outposts, cold);
        bench!("shipwrecks", mg::shipwreck::place_shipwrecks, cold);
        bench!("ocean_ruins", mg::ocean_ruin::place_ocean_ruins, cold);
        bench!("monuments", mg::monument::place_monuments, cold);
        bench!("ruined_portals", mg::ruined_portal::place_ruined_portals, cold);
        bench!("mansions", mg::mansion::place_mansions, cold);
        bench!("pyramids", mg::pyramid::place_desert_pyramids, cold);
        bench!("swamp_huts", mg::swamp_hut::place_swamp_huts, cold);
        bench!("jungle_temples", mg::jungle_temple::place_jungle_temples, cold);
        bench!("igloos", mg::igloo::place_igloos, cold);
        bench!("desert_wells", mg::structures::place_desert_wells, cold);
        bench!("ores", mg::features::place_ores, cold);
        bench!("disks", mg::misc_features::place_disks, cold);
        bench!("ancient_cities", mg::ancient_city::place_ancient_cities, cold);
        bench!("sculk", mg::sculk::place_sculk, cold);
        let mut base = Vec::new();
        for cx in 0..6 {
            for cz in 0..6 {
                let coord = [cx * 3, 4, cz * 3];
                base.push((coord, crate::worldgen::generate_chunk_at_ctx(coord, &ctx)));
            }
        }
        let mut t = [0.0f64; 6];
        let names = ["trees", "flowers", "biome_features", "ground_cover", "cactus", "mc_features"];
        for (coord, ch) in &base {
            let mut c = ch.clone();
            let t0 = std::time::Instant::now();
            trees::place_realistic_from_coord(&mut c, *coord, &ctx);
            t[0] += t0.elapsed().as_secs_f64();
            let t0 = std::time::Instant::now();
            flowers::place_realistic_from_coord(&mut c, *coord, &ctx);
            t[1] += t0.elapsed().as_secs_f64();
            let t0 = std::time::Instant::now();
            biome_features::place_realistic_from_coord(&mut c, *coord, &ctx);
            t[2] += t0.elapsed().as_secs_f64();
            let t0 = std::time::Instant::now();
            ground_cover::place_realistic_from_coord(&mut c, *coord, &ctx);
            t[3] += t0.elapsed().as_secs_f64();
            let t0 = std::time::Instant::now();
            cactus::place_realistic_from_coord(&mut c, *coord, &ctx);
            t[4] += t0.elapsed().as_secs_f64();
            let t0 = std::time::Instant::now();
            mcgen::features::place_from_coord(&mut c, *coord, &ctx);
            t[5] += t0.elapsed().as_secs_f64();
        }
        let n = base.len() as f64;
        for (i, name) in names.iter().enumerate() {
            println!("{name}: {:.2} ms/chunk", t[i] * 1000.0 / n);
        }
    }
}
