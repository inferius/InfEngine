//! W264 - the pluggable dimension registry (the DIMS-epic
//! foundation, and the extension point modded worlds plug into
//! later: Twilight Forest / Mystcraft ages / mining dims add rows
//! here + their own worldgen profile).
//!
//! Each dimension is one static record: slug (the save-dir + parking
//! key the engine already uses), the default worldgen profile, the
//! vertical chunk span (MC `dimension_type` min_y/height), the
//! portal coordinate scale, and the sky/ambient facts the renderer
//! consumes. The overworld / nether / end rows mirror MC's
//! `dimension_type` JSONs (26.1.2).
//!
//! What reads it: `current_worldgen_ctx` (profile fallback),
//! `vertical_chunk_range` (per-dim spans), the /dim command list,
//! and (incrementally) the sky/fog path. The legacy
//! `dim_worldgens` map stays as the per-world OVERRIDE - custom
//! `/dim add` worlds keep working; the registry only answers when
//! the map has no entry.

use super::WorldGen;

/// One dimension's static definition.
// The sky/ambient fields wire into the renderer incrementally
// (`allow(dead_code)`: the registry record is complete from day one,
// consumers land per wave - the mod-support doctrine).
#[derive(Debug)]
#[allow(dead_code)]
pub struct DimensionDef {
    /// Save-dir + parking-map key ("overworld", "nether", "end").
    pub slug: &'static str,
    /// Human name for the /dim list.
    pub display: &'static str,
    /// Default worldgen profile when the world's `dim_worldgens`
    /// table has no explicit entry. `None` = the world's own choice
    /// (the overworld row: worlds pick Classic/Mc/... at creation).
    pub default_profile: Option<WorldGen>,
    /// First vertical chunk (MC `dimension_type.min_y / 16`).
    pub min_chunk_y: i32,
    /// Vertical chunk count (`height / 16`).
    pub vertical_chunks: i32,
    /// `coordinate_scale` (nether 8: one nether block = 8 overworld).
    pub coordinate_scale: i32,
    /// `has_skylight` - drives the sky/celestial pass.
    pub has_skylight: bool,
    /// `has_ceiling` - a roofed dim (nether bedrock roof).
    pub has_ceiling: bool,
    /// `ambient_light` (nether 0.1 - dark corners stay readable).
    pub ambient_light: f32,
    /// `fixed_time` - `Some(ticks)` freezes the day cycle (nether
    /// 18000, end 6000); `None` = the cycle runs.
    pub fixed_time: Option<u32>,
    /// `bed_works` - false = beds explode in MC (respawn rules).
    pub bed_works: bool,
}

/// The built-in dimensions. Mod dims append rows through a future
/// registration API; every consumer iterates this slice (never
/// hardcode a slug match outside this module).
pub const DIMENSIONS: &[DimensionDef] = &[
    DimensionDef {
        slug: "overworld",
        display: "Overworld",
        default_profile: None,
        min_chunk_y: -4,
        vertical_chunks: 24,
        coordinate_scale: 1,
        has_skylight: true,
        has_ceiling: false,
        ambient_light: 0.0,
        fixed_time: None,
        bed_works: true,
    },
    DimensionDef {
        slug: "nether",
        display: "The Nether",
        default_profile: Some(WorldGen::Nether),
        min_chunk_y: 0,
        vertical_chunks: 16,
        coordinate_scale: 8,
        has_skylight: false,
        has_ceiling: true,
        ambient_light: 0.1,
        fixed_time: Some(18_000),
        bed_works: false,
    },
    DimensionDef {
        slug: "end",
        display: "The End",
        default_profile: Some(WorldGen::End),
        min_chunk_y: 0,
        vertical_chunks: 16,
        coordinate_scale: 1,
        has_skylight: false,
        has_ceiling: false,
        ambient_light: 0.0,
        fixed_time: Some(6_000),
        bed_works: false,
    },
];

/// Look a dimension up by slug.
pub fn dim_def(slug: &str) -> Option<&'static DimensionDef> {
    DIMENSIONS.iter().find(|d| d.slug == slug)
}

/// Slug lookup with the overworld as the fallback (custom /dim
/// worlds inherit overworld physics until they register a row).
pub fn dim_def_or_overworld(slug: &str) -> &'static DimensionDef {
    dim_def(slug).unwrap_or(&DIMENSIONS[0])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_rows_resolve() {
        assert_eq!(dim_def("overworld").unwrap().coordinate_scale, 1);
        assert_eq!(dim_def("nether").unwrap().coordinate_scale, 8);
        assert_eq!(
            dim_def("nether").unwrap().default_profile,
            Some(WorldGen::Nether)
        );
        assert_eq!(dim_def("end").unwrap().default_profile, Some(WorldGen::End));
        assert!(dim_def("twilight_forest").is_none());
        assert_eq!(dim_def_or_overworld("twilight_forest").slug, "overworld");
    }

    #[test]
    fn vertical_spans_match_mc_dimension_types() {
        let ow = dim_def("overworld").unwrap();
        assert_eq!((ow.min_chunk_y, ow.vertical_chunks), (-4, 24));
        let nt = dim_def("nether").unwrap();
        assert_eq!((nt.min_chunk_y, nt.vertical_chunks), (0, 16));
        let end = dim_def("end").unwrap();
        assert_eq!((end.min_chunk_y, end.vertical_chunks), (0, 16));
    }
}
