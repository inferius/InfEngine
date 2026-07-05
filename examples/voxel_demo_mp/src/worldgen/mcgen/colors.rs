//! W212 - per-biome water + underwater-fog colours.
//!
//! Water colours come from the biome JSONs' `effects.water_color`
//! (26.x stores hex strings; the extractor normalised them to ints).
//! Underwater fog is 26.x's `EnvironmentAttributes.WATER_FOG_COLOR`:
//! default 0x050533 plus the per-biome `setAttribute` overrides in
//! `OverworldBiomes.java` (lukewarm/warm oceans, dark forest, swamp,
//! mangrove swamp) - both baked into `biome_water_colors.json` by the
//! extraction script, no hand-picked values.
//!
//! Honest divergence: MC blends water colour per quart position with a
//! neighbourhood average (`BiomeColors`); the engine tints per chunk
//! liquid mesh, so a biome border steps over 16 blocks instead of
//! blending over ~4. Upgradable when the mesh vertex format grows a
//! colour attribute.

use std::collections::HashMap;
use std::sync::OnceLock;

use super::biome::McBiome;

const BIOME_WATER_COLORS: &str = include_str!("data/feature/biome_water_colors.json");

struct Row {
    water: [f32; 3],
    fog: [f32; 3],
}

fn rgb(v: u32) -> [f32; 3] {
    [
        ((v >> 16) & 0xFF) as f32 / 255.0,
        ((v >> 8) & 0xFF) as f32 / 255.0,
        (v & 0xFF) as f32 / 255.0,
    ]
}

fn table() -> &'static HashMap<String, Row> {
    static TABLE: OnceLock<HashMap<String, Row>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let v: serde_json::Value =
            serde_json::from_str(BIOME_WATER_COLORS).expect("water colours parse");
        v.as_object()
            .expect("water colours object")
            .iter()
            .map(|(k, row)| {
                let water = rgb(row["water"].as_u64().expect("water int") as u32);
                let fog = rgb(row["fog"].as_u64().expect("fog int") as u32);
                (k.clone(), Row { water, fog })
            })
            .collect()
    })
}

/// The biome's water tint (multiplies the grayscale `water_still`
/// texture, exactly MC's water colormap role).
pub(crate) fn water_color(mc: McBiome) -> [f32; 3] {
    table()[mc.name()].water
}

/// The biome's underwater fog / clear colour.
pub(crate) fn water_fog_color(mc: McBiome) -> [f32; 3] {
    table()[mc.name()].fog
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_biome_has_a_water_colour_row() {
        for mc in McBiome::ALL {
            let _ = water_color(mc);
            let _ = water_fog_color(mc);
        }
    }

    #[test]
    fn known_mc_values_pin() {
        // Default water 0x3F76E4; warm ocean 0x43D5EE; swamp 0x617B64.
        let close = |a: [f32; 3], v: u32| {
            let b = rgb(v);
            (a[0] - b[0]).abs() < 1e-6 && (a[1] - b[1]).abs() < 1e-6 && (a[2] - b[2]).abs() < 1e-6
        };
        assert!(close(water_color(McBiome::Plains), 0x3F76E4));
        assert!(close(water_color(McBiome::WarmOcean), 0x43D5EE));
        assert!(close(water_color(McBiome::Swamp), 0x617B64));
        assert!(close(water_fog_color(McBiome::Plains), 0x050533));
        assert!(close(water_fog_color(McBiome::WarmOcean), 0x041F33));
        assert!(close(water_fog_color(McBiome::MangroveSwamp), 0x4D7A60));
    }
}
