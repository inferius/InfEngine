//! W197 - faithful MC worldgen, Phase 5: the `surface_rule` DSL.
//!
//! MC paints the top few blocks of every column with a `SurfaceRules`
//! tree (the `surface_rule` in `overworld_noise_settings.json`): a
//! first-match sequence of `condition -> rule` nodes ending in `block`
//! placements. It's what turns the bare stone terrain into grass / dirt
//! / sand+sandstone beaches / gravel / podzol / snow on cold biomes /
//! the bedrock floor. This module parses that embedded tree (NOT
//! hand-transcribed - same data-driven approach as the density DSL) and
//! evaluates it per surface column.
//!
//! Honest scope: conditions that need MC's exact world context
//! (`vertical_gradient` randomness, `steep`, `temperature`, the badlands
//! banding) are approximated from the data we have; and biome conditions
//! match against the engine's 11-biome collapse of MC's 66 (so e.g. a
//! `badlands` rule maps to the nearest engine biome). The COMMON cases -
//! grass/dirt depth, sand+sandstone deserts, gravel/sand beaches by
//! water, snow on frozen biomes, the bedrock floor - port faithfully.

use std::sync::Arc;

use serde_json::Value;

use super::noise::{NoiseParams, NormalNoise, SplitMix64};

/// Engine block ids the surface rules place. `grass_block` is the
/// engine's "grass" (3); `snow_block` is the full opaque cube added in
/// W197 Phase 5c (752), which `powder_snow` also maps to for now (its
/// sink-in physics is a deferred behavior port - not the thin layer 15).
mod ids {
    pub const AIR: u16 = 0;
    pub const STONE: u16 = 1;
    pub const DIRT: u16 = 2;
    pub const GRASS_BLOCK: u16 = 3;
    pub const SAND: u16 = 4;
    pub const SNOW_BLOCK: u16 = 752; // full opaque snow cube (W197 Phase 5c)
    pub const SANDSTONE: u16 = 26;
    pub const BEDROCK: u16 = 45;
    pub const DEEPSLATE: u16 = 96;
    pub const CALCITE: u16 = 97;
    pub const PODZOL: u16 = 394;
    pub const MYCELIUM: u16 = 395;
    pub const COARSE_DIRT: u16 = 396;
    pub const MUD: u16 = 397;
    pub const ICE: u16 = 400;
    pub const PACKED_ICE: u16 = 401;
    pub const TERRACOTTA: u16 = 419;
    pub const WHITE_TERRACOTTA: u16 = 420;
    pub const ORANGE_TERRACOTTA: u16 = 421;
    pub const RED_SANDSTONE: u16 = 358;
    pub const GRAVEL: u16 = 644;
    pub const RED_SAND: u16 = 645;
    pub const WATER: u16 = 36;
}

/// Map an MC block `Name` to an engine id. Unknown -> stone (never panic
/// mid-gen).
fn block_id(name: &str) -> u16 {
    let n = name.strip_prefix("minecraft:").unwrap_or(name);
    match n {
        "air" => ids::AIR,
        "stone" => ids::STONE,
        "dirt" => ids::DIRT,
        "grass_block" => ids::GRASS_BLOCK,
        "coarse_dirt" => ids::COARSE_DIRT,
        "podzol" => ids::PODZOL,
        "mycelium" => ids::MYCELIUM,
        "mud" => ids::MUD,
        "sand" => ids::SAND,
        "sandstone" => ids::SANDSTONE,
        "red_sand" => ids::RED_SAND,
        "red_sandstone" => ids::RED_SANDSTONE,
        "gravel" => ids::GRAVEL,
        "water" => ids::WATER,
        "ice" => ids::ICE,
        "packed_ice" => ids::PACKED_ICE,
        "snow_block" | "powder_snow" => ids::SNOW_BLOCK,
        "calcite" => ids::CALCITE,
        "bedrock" => ids::BEDROCK,
        "deepslate" => ids::DEEPSLATE,
        "terracotta" => ids::TERRACOTTA,
        "white_terracotta" => ids::WHITE_TERRACOTTA,
        "orange_terracotta" => ids::ORANGE_TERRACOTTA,
        // W266: everything else resolves through the registry (the
        // nether families: netherrack / soul sand+soil / basalt /
        // blackstone / nylium / wart blocks / lava); truly unknown
        // names keep the old stone stand-in.
        _ => super::features::block_id_by_name(n).unwrap_or(ids::STONE),
    }
}

// W221: biome conditions match the REAL MC biome name carried by the
// column (`SurfaceCtx::mc_name`, from `McBiome::name()`), NOT the lossy
// engine collapse. The pre-W211 collapse mapping made every rule of a
// SPECIAL biome fire on every column sharing its engine bucket - the
// user-visible "gravel plains" (windswept_gravelly_hills hijacking all
// Mountain columns) and "snow on desert beaches" (snowy_beach hijacking
// every Beach). With name matching, rules for biomes the classifier
// never emits (badlands, mushroom_fields, stony_shore...) simply never
// fire, exactly like MC with those regions absent.

/// A vertical anchor (`{above_bottom|absolute|below_top: N}`), resolved
/// against the world's `min_y` / top.
#[derive(Clone, Copy)]
enum VAnchor {
    AboveBottom(i32),
    Absolute(i32),
    BelowTop(i32),
}

impl VAnchor {
    fn parse(v: &Value) -> Self {
        if let Some(n) = v.get("above_bottom").and_then(Value::as_i64) {
            VAnchor::AboveBottom(n as i32)
        } else if let Some(n) = v.get("below_top").and_then(Value::as_i64) {
            VAnchor::BelowTop(n as i32)
        } else if let Some(n) = v.get("absolute").and_then(Value::as_i64) {
            VAnchor::Absolute(n as i32)
        } else {
            VAnchor::Absolute(0)
        }
    }
    fn resolve(self, min_y: i32, max_y: i32) -> i32 {
        match self {
            VAnchor::AboveBottom(n) => min_y + n,
            VAnchor::Absolute(n) => n,
            VAnchor::BelowTop(n) => max_y - n,
        }
    }
}

/// A surface-rule condition (the `if_true` of a `condition` node).
enum Cond {
    /// `biome_is` - MC registry name stems, matched against the
    /// column's real [`SurfaceCtx::mc_name`].
    Biome(Vec<String>),
    NoiseThreshold { noise: Arc<NormalNoise>, min: f64, max: f64 },
    StoneDepth { offset: i32, ceiling: bool, add_surface_depth: bool, secondary: i32 },
    Water { offset: i32, mult: i32, add_stone_depth: bool },
    YAbove { anchor: VAnchor, mult: i32, add_stone_depth: bool },
    Steep,
    Hole,
    Temperature,
    AbovePreliminarySurface,
    VerticalGradient { seed: u64, true_below: VAnchor, false_above: VAnchor },
    Not(Box<Cond>),
    /// Unrecognised - never true (rule skipped).
    Never,
}

/// A surface-rule node.
enum Rule {
    Sequence(Vec<Rule>),
    Condition(Cond, Box<Rule>),
    Block(u16),
    /// Badlands terracotta banding (W227.3: the real 192-layer clay
    /// strata via [`ClayBands`]).
    Bandlands,
    /// Unrecognised - yields nothing.
    Nop,
}

/// Per-column, per-y context the rules evaluate against, mirroring MC's
/// `SurfaceRules.Context` / `SurfaceSystem`.
pub(crate) struct SurfaceCtx {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    /// Solid blocks above this cell within the current run (0 = top).
    pub stone_depth_above: i32,
    /// Solid blocks below this cell (for ceiling rules).
    pub stone_depth_below: i32,
    /// Water surface Y for this column, or `i32::MIN` if dry.
    pub water_height: i32,
    /// The column's surface (heightmap) top - the preliminary surface.
    pub surface: i32,
    /// MC `surfaceDepth`: how thick the soft surface layer is here.
    pub surface_depth: i32,
    /// True where the terrain is steep (used for gravel/stone faces).
    pub steep: bool,
    /// True where this CELL is cold enough to snow (the `temperature`
    /// condition - the real height-adjusted model, W221).
    pub frozen: bool,
    /// The column's MC biome registry name (`McBiome::name()`).
    pub mc_name: &'static str,
    pub min_y: i32,
    pub max_y: i32,
}

/// W227.3: the badlands terracotta banding - `SurfaceSystem`'s
/// 192-layer clay_bands strata (generateBands/makeBands exactly:
/// scattered orange, 6-15 bands each of yellow/brown/red, 9-15 white
/// bands with 50% light-gray fringes) plus the `clay_bands_offset`
/// noise that shifts the strata per column (`getBand`). SplitMix64
/// world-seed streams stand in for MC's `fromHashOf` (rules parity,
/// not seed identity - the whole port's convention).
pub(crate) struct ClayBands {
    bands: [u16; 192],
    offset: NormalNoise,
}

const CLAY_BANDS_OFFSET_JSON: &str = include_str!("data/noise/clay_bands_offset.json");
const SALT_CLAY_BANDS: u64 = 0xC1A7_BA2D_0000_0000;

impl ClayBands {
    fn new(seed: u32) -> ClayBands {
        let by_name = |n: &str| {
            crate::worldgen::mcgen::features::block_id_by_name(n).unwrap_or(ids::TERRACOTTA)
        };
        let (yellow, brown, red, light_gray) = (
            by_name("yellow_terracotta"),
            by_name("brown_terracotta"),
            by_name("red_terracotta"),
            by_name("light_gray_terracotta"),
        );
        let mut r = SplitMix64::new(seed as u64 ^ SALT_CLAY_BANDS);
        let mut next_int = |n: i32| -> i32 { (r.next_u64() % n.max(1) as u64) as i32 };
        let mut bands = [ids::TERRACOTTA; 192];
        // Scattered single orange layers (the base for loop's i++ folds
        // into the extra += 1).
        let mut i = 0usize;
        loop {
            i += next_int(5) as usize + 1;
            if i >= 192 {
                break;
            }
            bands[i] = ids::ORANGE_TERRACOTTA;
            i += 1;
            if i >= 192 {
                break;
            }
        }
        // makeBands: 6-15 runs of width base+rand(3) at random starts.
        for (base, id) in [(1, yellow), (2, brown), (1, red)] {
            let count = 6 + next_int(10);
            for _ in 0..count {
                let width = base + next_int(3);
                let start = next_int(192) as usize;
                for p in 0..width as usize {
                    if start + p >= 192 {
                        break;
                    }
                    bands[start + p] = id;
                }
            }
        }
        // White bands with 50% light-gray fringes, stepped 4..=19.
        let white_count = 9 + next_int(7);
        let mut placed = 0;
        let mut start = 0usize;
        while placed < white_count && start < 192 {
            bands[start] = ids::WHITE_TERRACOTTA;
            if start >= 2 && next_int(2) == 0 {
                bands[start - 1] = light_gray;
            }
            if start + 1 < 192 && next_int(2) == 0 {
                bands[start + 1] = light_gray;
            }
            placed += 1;
            start += next_int(16) as usize + 4;
        }
        let params: NoiseParams =
            serde_json::from_str(CLAY_BANDS_OFFSET_JSON).expect("clay_bands_offset params");
        ClayBands {
            bands,
            offset: NormalNoise::new(seed as u64 ^ SALT_CLAY_BANDS ^ 1, &params),
        }
    }

    /// `SurfaceSystem.getBand`.
    fn get(&self, x: i32, y: i32, z: i32) -> u16 {
        let off = (self.offset.value(x as f64, 0.0, z as f64) * 4.0).round() as i32;
        self.bands[(y + off).rem_euclid(192) as usize]
    }
}

/// The parsed overworld surface rule + its referenced noises.
pub(crate) struct SurfaceRules {
    root: Rule,
    clay: ClayBands,
}

impl SurfaceRules {
    /// Parse the `surface_rule` value, resolving `noise_threshold`
    /// noises through `noise_res` (name -> seeded NormalNoise); `seed`
    /// builds the badlands clay bands.
    pub(crate) fn parse(
        v: &Value,
        noise_res: &dyn Fn(&str) -> Option<Arc<NormalNoise>>,
        seed: u32,
    ) -> Self {
        SurfaceRules { root: parse_rule(v, noise_res), clay: ClayBands::new(seed) }
    }

    /// First matching block id for the column/cell, or `None` to leave
    /// whatever the terrain stage put there.
    pub(crate) fn eval(&self, ctx: &SurfaceCtx) -> Option<u16> {
        eval_rule(&self.root, ctx, &self.clay)
    }
}

fn parse_rule(v: &Value, nr: &dyn Fn(&str) -> Option<Arc<NormalNoise>>) -> Rule {
    let t = v.get("type").and_then(Value::as_str).unwrap_or("");
    match t.strip_prefix("minecraft:").unwrap_or(t) {
        "sequence" => Rule::Sequence(
            v.get("sequence")
                .and_then(Value::as_array)
                .map(|a| a.iter().map(|r| parse_rule(r, nr)).collect())
                .unwrap_or_default(),
        ),
        "condition" => {
            let cond = v.get("if_true").map(|c| parse_cond(c, nr)).unwrap_or(Cond::Never);
            let then = v
                .get("then_run")
                .map(|r| Box::new(parse_rule(r, nr)))
                .unwrap_or_else(|| Box::new(Rule::Nop));
            Rule::Condition(cond, then)
        }
        "block" => {
            let name = v
                .get("result_state")
                .and_then(|s| s.get("Name"))
                .and_then(Value::as_str)
                .unwrap_or("minecraft:stone");
            Rule::Block(block_id(name))
        }
        "bandlands" => Rule::Bandlands,
        _ => Rule::Nop,
    }
}

fn parse_cond(v: &Value, nr: &dyn Fn(&str) -> Option<Arc<NormalNoise>>) -> Cond {
    let t = v.get("type").and_then(Value::as_str).unwrap_or("");
    let i = |k: &str| v.get(k).and_then(Value::as_i64).unwrap_or(0) as i32;
    let b = |k: &str| v.get(k).and_then(Value::as_bool).unwrap_or(false);
    match t.strip_prefix("minecraft:").unwrap_or(t) {
        "biome" => {
            let biomes = v
                .get("biome_is")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(Value::as_str)
                        .map(|s| s.strip_prefix("minecraft:").unwrap_or(s).to_string())
                        .collect()
                })
                .unwrap_or_default();
            Cond::Biome(biomes)
        }
        "noise_threshold" => {
            let name = v.get("noise").and_then(Value::as_str).unwrap_or("");
            match nr(name) {
                Some(noise) => Cond::NoiseThreshold {
                    noise,
                    min: v.get("min_threshold").and_then(Value::as_f64).unwrap_or(0.0),
                    max: v.get("max_threshold").and_then(Value::as_f64).unwrap_or(0.0),
                },
                None => Cond::Never,
            }
        }
        "stone_depth" => Cond::StoneDepth {
            offset: i("offset"),
            ceiling: v.get("surface_type").and_then(Value::as_str) == Some("ceiling"),
            add_surface_depth: b("add_surface_depth"),
            secondary: i("secondary_depth_range"),
        },
        "water" => Cond::Water {
            offset: i("offset"),
            mult: i("surface_depth_multiplier"),
            add_stone_depth: b("add_stone_depth"),
        },
        "y_above" => Cond::YAbove {
            anchor: v.get("anchor").map(VAnchor::parse).unwrap_or(VAnchor::Absolute(0)),
            mult: i("surface_depth_multiplier"),
            add_stone_depth: b("add_stone_depth"),
        },
        "steep" => Cond::Steep,
        "hole" => Cond::Hole,
        "temperature" => Cond::Temperature,
        "above_preliminary_surface" => Cond::AbovePreliminarySurface,
        "vertical_gradient" => {
            let name = v.get("random_name").and_then(Value::as_str).unwrap_or("");
            Cond::VerticalGradient {
                seed: fnv(name),
                true_below: v.get("true_at_and_below").map(VAnchor::parse).unwrap_or(VAnchor::Absolute(0)),
                false_above: v.get("false_at_and_above").map(VAnchor::parse).unwrap_or(VAnchor::Absolute(0)),
            }
        }
        "not" => Cond::Not(Box::new(
            v.get("invert").map(|c| parse_cond(c, nr)).unwrap_or(Cond::Never),
        )),
        _ => Cond::Never,
    }
}

fn fnv(s: &str) -> u64 {
    let mut h = 0xCBF2_9CE4_8422_2325u64;
    for x in s.bytes() {
        h ^= x as u64;
        h = h.wrapping_mul(0x0100_0000_01B3);
    }
    h
}

fn eval_rule(rule: &Rule, ctx: &SurfaceCtx, clay: &ClayBands) -> Option<u16> {
    match rule {
        Rule::Sequence(rules) => rules.iter().find_map(|r| eval_rule(r, ctx, clay)),
        Rule::Condition(cond, then) => {
            if eval_cond(cond, ctx) {
                eval_rule(then, ctx, clay)
            } else {
                None
            }
        }
        Rule::Block(id) => Some(*id),
        // W227.3: the real terracotta strata.
        Rule::Bandlands => Some(clay.get(ctx.x, ctx.y, ctx.z)),
        Rule::Nop => None,
    }
}

fn eval_cond(cond: &Cond, ctx: &SurfaceCtx) -> bool {
    match cond {
        Cond::Biome(list) => list.iter().any(|n| n == ctx.mc_name),
        Cond::NoiseThreshold { noise, min, max } => {
            // MC samples surface noises at (x, 0, z) (xz-only).
            let n = noise.value(ctx.x as f64, 0.0, ctx.z as f64);
            n >= *min && n <= *max
        }
        Cond::StoneDepth { offset, ceiling, add_surface_depth, secondary } => {
            let depth = if *ceiling { ctx.stone_depth_below } else { ctx.stone_depth_above };
            let add = if *add_surface_depth { ctx.surface_depth } else { 0 };
            // MC also widens by a secondary range near the surface; we
            // fold it straight into the limit.
            depth <= offset + add + secondary
        }
        Cond::Water { offset, mult, add_stone_depth } => {
            if ctx.water_height == i32::MIN {
                return true; // no water column -> always "above water"
            }
            let add = if *add_stone_depth { ctx.stone_depth_above } else { 0 };
            ctx.y + add >= ctx.water_height + offset + ctx.surface_depth * mult
        }
        Cond::YAbove { anchor, mult, add_stone_depth } => {
            let add = if *add_stone_depth { ctx.stone_depth_above } else { 0 };
            ctx.y + add >= anchor.resolve(ctx.min_y, ctx.max_y) + ctx.surface_depth * mult
        }
        Cond::Steep => ctx.steep,
        Cond::Hole => ctx.surface_depth <= 0,
        Cond::Temperature => ctx.frozen,
        Cond::AbovePreliminarySurface => ctx.y >= ctx.surface,
        Cond::VerticalGradient { seed, true_below, false_above } => {
            let lo = true_below.resolve(ctx.min_y, ctx.max_y);
            let hi = false_above.resolve(ctx.min_y, ctx.max_y);
            gradient_roll(*seed, ctx.x, ctx.y, ctx.z, lo, hi)
        }
        Cond::Not(inner) => !eval_cond(inner, ctx),
        Cond::Never => false,
    }
}

/// One `vertical_gradient` decision (`SurfaceRules
/// .VerticalGradientConditionSource`): always true at/below `lo`,
/// never at/above `hi`, in between the probability of "true" ramps
/// from 1 at `lo` to 0 at `hi`. MC uses a per-cell random from a
/// named positional factory; a position hash keeps it stable +
/// deterministic. Shared by the rule-chain eval AND the deep-fill
/// fast paths ([`is_bedrock_floor`] / [`is_deepslate`]) so a cell
/// rolls identically wherever it's asked.
fn gradient_roll(seed: u64, x: i32, y: i32, z: i32, lo: i32, hi: i32) -> bool {
    if y <= lo {
        return true;
    }
    if y >= hi {
        return false;
    }
    let frac = (y - lo) as f64 / (hi - lo).max(1) as f64;
    let h = pos_hash(seed, x, y, z);
    (h as f64 / u64::MAX as f64) > frac
}

/// W260: the overworld `bedrock_floor` rule (`vertical_gradient`
/// above_bottom 0..5) as a standalone check. The chunk fill only
/// evaluates the rule chain inside the surface band, so the deep
/// bulk cells ask this directly - same stream name + roll as the
/// JSON rule.
pub(crate) fn is_bedrock_floor(x: i32, y: i32, z: i32, min_y: i32) -> bool {
    gradient_roll(fnv("minecraft:bedrock_floor"), x, y, z, min_y, min_y + 5)
}

/// W260: the overworld trailing `deepslate` rule (`vertical_gradient`
/// absolute 0..8) as a standalone check for the deep bulk cells.
pub(crate) fn is_deepslate(x: i32, y: i32, z: i32) -> bool {
    gradient_roll(fnv("minecraft:deepslate"), x, y, z, 0, 8)
}

fn pos_hash(seed: u64, x: i32, y: i32, z: i32) -> u64 {
    let mut h = seed ^ 0x9E37_79B9_7F4A_7C15;
    for v in [x as u64, y as u64, z as u64] {
        h ^= v.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
        h = h.rotate_left(31).wrapping_mul(0xC4CE_B9FE_1A85_EC53);
    }
    h ^= h >> 33;
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules_json() -> serde_json::Value {
        let s: serde_json::Value =
            serde_json::from_str(super::super::data::OVERWORLD_SETTINGS).unwrap();
        s["surface_rule"].clone()
    }

    fn ctx(y: i32, mc_name: &'static str) -> SurfaceCtx {
        SurfaceCtx {
            x: 0,
            y,
            z: 0,
            stone_depth_above: 0,
            stone_depth_below: i32::MAX / 2, // the fill's no-ceiling default
            water_height: 63,
            surface: 70,
            surface_depth: 3,
            steep: false,
            frozen: false,
            mc_name,
            min_y: -64,
            max_y: 320,
        }
    }

    #[test]
    fn parses_overworld_surface_rule() {
        // No noise resolver in this test (noise_threshold -> Never), but
        // the tree must parse without panicking and be evaluable.
        let nr = |_: &str| None;
        let sr = SurfaceRules::parse(&rules_json(), &nr, 0);
        // A grassy land surface (top solid, above water, above
        // preliminary surface) should resolve to grass_block.
        let mut c = ctx(70, "plains");
        c.surface = 70;
        let got = sr.eval(&c);
        assert_eq!(got, Some(ids::GRASS_BLOCK), "plains surface should be grass, got {got:?}");
    }

    #[test]
    fn snowy_mountain_surface_is_snow() {
        // Snowy slopes must paint a snow_block surface, not green grass
        // - the user's "cold biomes are green" complaint. With no noise
        // resolver the powder_snow branch is skipped, so the snow_block
        // default wins. The frozen flag models coldEnoughToSnow there.
        let nr = |_: &str| None;
        let sr = SurfaceRules::parse(&rules_json(), &nr, 0);
        let mut c = ctx(80, "snowy_slopes");
        c.frozen = true;
        assert_eq!(
            sr.eval(&c),
            Some(ids::SNOW_BLOCK),
            "snowy slopes surface should be a snow block"
        );
    }

    /// W221: the anti-bleed guarantee that replaced the collapse map. A
    /// plain Beach column must NOT take the snowy_beach snow surface,
    /// and a windswept_hills column must NOT take the gravelly variant
    /// (the user's gravel plains / snowy desert coast screenshots).
    #[test]
    fn special_biome_rules_do_not_bleed() {
        let nr = |_: &str| None;
        let sr = SurfaceRules::parse(&rules_json(), &nr, 0);
        let mut beach = ctx(64, "beach");
        beach.water_height = i32::MIN;
        beach.surface = 64;
        let got = sr.eval(&beach);
        assert_ne!(got, Some(ids::SNOW_BLOCK), "warm beach must not take snowy_beach snow");
        assert_eq!(got, Some(ids::SAND), "beach surface is sand, got {got:?}");
        let mut hills = ctx(90, "windswept_hills");
        hills.water_height = i32::MIN;
        hills.surface = 90;
        // With no noise resolver the windswept noise branches skip; the
        // point is the GRAVELLY variant's rule can't fire by name.
        assert_ne!(sr.eval(&hills), Some(ids::GRAVEL));
    }

    #[test]
    fn bedrock_at_world_floor() {
        let nr = |_: &str| None;
        let sr = SurfaceRules::parse(&rules_json(), &nr, 0);
        // The first rule is the bedrock floor (vertical_gradient near
        // min_y). At the very bottom it should always be bedrock.
        let c = ctx(-64, "plains");
        assert_eq!(sr.eval(&c), Some(ids::BEDROCK));
    }

    #[test]
    fn block_map_resolves() {
        assert_eq!(block_id("minecraft:grass_block"), ids::GRASS_BLOCK);
        assert_eq!(block_id("minecraft:gravel"), ids::GRAVEL);
        assert_eq!(block_id("minecraft:red_sandstone"), ids::RED_SANDSTONE);
    }

    /// W227.3: the badlands clay bands carry real strata - several
    /// terracotta colours, deterministic per seed, column-shifted by
    /// the offset noise.
    #[test]
    fn clay_bands_have_strata() {
        let a = ClayBands::new(42);
        let b = ClayBands::new(42);
        let mut colours = std::collections::HashSet::new();
        for y in 0..192 {
            assert_eq!(a.get(10, y, -7), b.get(10, y, -7), "bands must be deterministic");
            colours.insert(a.get(10, y, -7));
        }
        assert!(
            colours.len() >= 4,
            "expected several terracotta colours in the strata, got {colours:?}"
        );
        // The offset noise must actually shift bands across the world:
        // sample many far-apart columns; at least two distinct integer
        // offsets must appear (a single column pair can tie by chance).
        let mut offsets = std::collections::HashSet::new();
        for i in 0..32 {
            let (x, z) = (i * 1731 - 20000, i * -2917 + 9000);
            offsets.insert((a.offset.value(x as f64, 0.0, z as f64) * 4.0).round() as i32);
        }
        assert!(
            offsets.len() >= 2,
            "clay_bands_offset must shift the strata per column, got {offsets:?}"
        );
    }
}
