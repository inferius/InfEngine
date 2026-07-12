//! W264 - the End terrain, ported data-driven from
//! `noise_settings/end.json` (26.1.2) exactly like the overworld
//! (W181+): final_density = squeeze(0.64 * interpolated(
//! blend_density(-0.234375 + top_slide * (0.234375 + (-23.4375 +
//! bottom_slide * (23.4375 + sloped_cheese)))))) where sloped_cheese
//! = `end_islands` (the verbatim `EndIslandDensityFunction` port:
//! main island 100-radius falloff + hash-sized outer islands past
//! chunk distance 64 where the island simplex dips under -0.9) +
//! the legacy `old_blended_noise`. Solid where density > 0; the
//! surface rule is a single block: end_stone. No aquifers, no sea.
//!
//! First slice notes: `interpolated` evaluates at full resolution
//! (MC smooths on 8x4x8 cells - ours reads slightly smoother);
//! biomes (small/end midlands/highlands/barrens), chorus plants,
//! obsidian pillars, the dragon fight and end cities are follow-up
//! waves. The obsidian spawn platform belongs to the arrival path,
//! not the generator.

use std::cell::RefCell;
use std::rc::Rc;

use infengine_voxel::{BlockId, CHUNK_SIZE, Chunk, ChunkCoord, World};
use serde_json::Value;
use std::sync::Arc;

use super::data;
use super::density::{Df, parse};
use super::noise::{NoiseParams, NormalNoise};
use crate::worldgen::WorldgenCtx;

/// `end_stone` (registry id 148).
const END_STONE: u16 = 148;

pub(crate) struct EndDim {
    final_density: Df,
    pub(crate) min_y: i32,
    pub(crate) height: i32,
}

/// Per-noise seed fold (the overworld convention).
fn noise_seed(seed: u32, name: &str) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325 ^ (seed as u64);
    for b in name.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0100_0000_01B3);
    }
    h
}

/// Parse the End `final_density` for `seed`.
pub(crate) fn build_end(seed: u32) -> EndDim {
    let settings: Value =
        serde_json::from_str(data::END_SETTINGS).expect("end settings JSON");
    let df_res = |name: &str| -> Option<Value> {
        let n = name.strip_prefix("minecraft:").unwrap_or(name);
        data::df_json(n).and_then(|j| serde_json::from_str(j).ok())
    };
    let noise_res = move |name: &str| -> Option<Arc<NormalNoise>> {
        let n = name.strip_prefix("minecraft:").unwrap_or(name);
        let params: NoiseParams = serde_json::from_str(data::noise_json(n)?).ok()?;
        Some(Arc::new(NormalNoise::new(noise_seed(seed, n), &params)))
    };
    let nr = &settings["noise_router"];
    let blended_seed = noise_seed(seed, "end_blended_noise");
    let final_density = parse(
        &nr["final_density"],
        &df_res,
        &noise_res as &super::density::NoiseResolver<'_>,
        blended_seed,
    );
    let noise = &settings["noise"];
    EndDim {
        final_density,
        min_y: noise["min_y"].as_i64().unwrap_or(0) as i32,
        height: noise["height"].as_i64().unwrap_or(128) as i32,
    }
}

thread_local! {
    static END_CACHE: RefCell<Option<(u32, Rc<EndDim>)>> = const { RefCell::new(None) };
}

fn end_for(seed: u32) -> Rc<EndDim> {
    END_CACHE.with(|c| {
        let mut slot = c.borrow_mut();
        match slot.as_ref() {
            Some((s, e)) if *s == seed => return e.clone(),
            _ => {}
        }
        let e = Rc::new(build_end(seed));
        *slot = Some((seed, e.clone()));
        e
    })
}

/// The End chunk generator: end_stone where the density is positive
/// inside the 0..height noise band, air (the void) everywhere else.
pub(crate) fn gen_end(coord: ChunkCoord, ctx: &WorldgenCtx) -> Chunk {
    let seed = ctx.effective_seed();
    let end = end_for(seed);
    let origin = World::chunk_world_origin(coord);
    let mut chunk = Chunk::empty();
    let (oy, ox, oz) = (origin.y as i32, origin.x as i32, origin.z as i32);
    if oy + (CHUNK_SIZE as i32) <= end.min_y || oy >= end.min_y + end.height {
        return chunk;
    }
    chunk.fill_with(|x, y, z| {
        let wy = oy + y as i32;
        if wy < end.min_y || wy >= end.min_y + end.height {
            return BlockId::AIR;
        }
        let (wx, wz) = (ox + x as i32, oz + z as i32);
        if end
            .final_density
            .eval(wx as f64, wy as f64, wz as f64)
            > 0.0
        {
            BlockId(END_STONE)
        } else {
            BlockId::AIR
        }
    });
    chunk
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worldgen::WorldGen;

    fn ctx() -> WorldgenCtx {
        WorldgenCtx {
            base_seed: 0,
            dim: "end".into(),
            profile: WorldGen::End,
        }
    }

    /// The main island exists: chunks around the origin carry
    /// end_stone in the 40..80 band, and the far void (inside the
    /// 64-chunk dead ring, outside the main island) is empty.
    #[test]
    fn main_island_generates_and_the_void_is_empty() {
        let c = ctx();
        let mut stone = 0u32;
        for cy in 2..5 {
            let chunk = gen_end([0, cy, 0], &c);
            for x in 0..CHUNK_SIZE {
                for y in 0..CHUNK_SIZE {
                    for z in 0..CHUNK_SIZE {
                        if chunk.get(x, y, z).0 == END_STONE {
                            stone += 1;
                        }
                    }
                }
            }
        }
        assert!(stone > 500, "main island end_stone cells: {stone}");
        // Chunk 40 (~640 blocks out) sits in the between-islands ring
        // (past the main island falloff, before the outer islands at
        // chunk distance 64).
        let far = gen_end([40, 3, 0], &c);
        let mut far_stone = 0u32;
        for x in 0..CHUNK_SIZE {
            for y in 0..CHUNK_SIZE {
                for z in 0..CHUNK_SIZE {
                    if far.get(x, y, z).0 == END_STONE {
                        far_stone += 1;
                    }
                }
            }
        }
        assert_eq!(far_stone, 0, "the void ring must be empty");
    }

    /// Outer islands exist somewhere past the 64-chunk ring. The
    /// simplex-under--0.9 gate makes them SPARSE - probe the island
    /// height field itself (cheap) over a wide chunk area.
    #[test]
    fn outer_islands_exist() {
        use crate::worldgen::mcgen::density::end_island_height;
        use crate::worldgen::mcgen::noise::SimplexNoise2;
        let noise = SimplexNoise2::new(12345);
        let mut gate_hits = 0u32;
        let mut island_sections = 0u32;
        for cx in -400i64..400 {
            for cz in -400i64..400 {
                if cx * cx + cz * cz <= 4096 {
                    continue;
                }
                if noise.get_value(cx as f64, cz as f64) < -0.9 {
                    gate_hits += 1;
                    let h =
                        end_island_height(&noise, (cx * 2) as i32, (cz * 2) as i32);
                    if h > 0.0 {
                        island_sections += 1;
                    }
                }
            }
        }
        assert!(gate_hits > 0, "the -0.9 simplex gate never fired in 800x800 chunks");
        assert!(
            island_sections > 0,
            "island centres never lift the height field: {gate_hits} gate hits"
        );
    }
}
