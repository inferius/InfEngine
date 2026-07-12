//! W183 - embedded MC overworld worldgen data (the final_density
//! dependency closure: 19 density functions + 42 noise params + the
//! noise_settings entry). Transcribed from the client jar's
//! data/minecraft/worldgen/, same faithful-port basis as the model
//! data. GENERATED - see tools/mc_worldgen extraction.

/// Raw JSON of the overworld noise_settings (noise cfg + noise_router
/// + surface_rule).
pub(crate) const OVERWORLD_SETTINGS: &str = include_str!("data/overworld_noise_settings.json");

/// W264: raw JSON of the End noise_settings (the whole dimension is
/// 97 lines - end_islands + base_3d_noise + slides).
pub(crate) const END_SETTINGS: &str = include_str!("data/end_noise_settings.json");

/// W266: raw JSON of the Nether noise_settings (router + the
/// netherrack/soul-sand/basalt/nylium surface rules, lava default
/// fluid at sea 32, bedrock floor + roof gradients).
pub(crate) const NETHER_SETTINGS: &str = include_str!("data/nether_noise_settings.json");

/// Density-function JSON by name (path under density_function/).
pub(crate) fn df_json(name: &str) -> Option<&'static str> {
    match name {
        "end/base_3d_noise" => Some(include_str!("data/density_function/end/base_3d_noise.json")),
        "end/sloped_cheese" => Some(include_str!("data/density_function/end/sloped_cheese.json")),
        "nether/base_3d_noise" => Some(include_str!("data/density_function/nether/base_3d_noise.json")),
        "overworld/base_3d_noise" => Some(include_str!("data/density_function/overworld/base_3d_noise.json")),
        "overworld/caves/entrances" => Some(include_str!("data/density_function/overworld/caves/entrances.json")),
        "overworld/caves/noodle" => Some(include_str!("data/density_function/overworld/caves/noodle.json")),
        "overworld/caves/pillars" => Some(include_str!("data/density_function/overworld/caves/pillars.json")),
        "overworld/caves/spaghetti_2d" => Some(include_str!("data/density_function/overworld/caves/spaghetti_2d.json")),
        "overworld/caves/spaghetti_2d_thickness_modulator" => Some(include_str!("data/density_function/overworld/caves/spaghetti_2d_thickness_modulator.json")),
        "overworld/caves/spaghetti_roughness_function" => Some(include_str!("data/density_function/overworld/caves/spaghetti_roughness_function.json")),
        "overworld/continents" => Some(include_str!("data/density_function/overworld/continents.json")),
        "overworld/depth" => Some(include_str!("data/density_function/overworld/depth.json")),
        "overworld/erosion" => Some(include_str!("data/density_function/overworld/erosion.json")),
        "overworld/factor" => Some(include_str!("data/density_function/overworld/factor.json")),
        "overworld/jaggedness" => Some(include_str!("data/density_function/overworld/jaggedness.json")),
        "overworld/offset" => Some(include_str!("data/density_function/overworld/offset.json")),
        "overworld/ridges" => Some(include_str!("data/density_function/overworld/ridges.json")),
        "overworld/ridges_folded" => Some(include_str!("data/density_function/overworld/ridges_folded.json")),
        "overworld/sloped_cheese" => Some(include_str!("data/density_function/overworld/sloped_cheese.json")),
        "shift_x" => Some(include_str!("data/density_function/shift_x.json")),
        "shift_z" => Some(include_str!("data/density_function/shift_z.json")),
        "y" => Some(include_str!("data/density_function/y.json")),
        _ => None,
    }
}

/// Noise-param JSON by name.
pub(crate) fn noise_json(name: &str) -> Option<&'static str> {
    match name {
        "aquifer_barrier" => Some(include_str!("data/noise/aquifer_barrier.json")),
        "aquifer_fluid_level_floodedness" => Some(include_str!("data/noise/aquifer_fluid_level_floodedness.json")),
        "aquifer_fluid_level_spread" => Some(include_str!("data/noise/aquifer_fluid_level_spread.json")),
        "aquifer_lava" => Some(include_str!("data/noise/aquifer_lava.json")),
        "calcite" => Some(include_str!("data/noise/calcite.json")),
        "cave_entrance" => Some(include_str!("data/noise/cave_entrance.json")),
        "cave_cheese" => Some(include_str!("data/noise/cave_cheese.json")),
        "cave_layer" => Some(include_str!("data/noise/cave_layer.json")),
        "continentalness" => Some(include_str!("data/noise/continentalness.json")),
        "erosion" => Some(include_str!("data/noise/erosion.json")),
        "gravel" => Some(include_str!("data/noise/gravel.json")),
        "gravel_layer" => Some(include_str!("data/noise/gravel_layer.json")),
        "ice" => Some(include_str!("data/noise/ice.json")),
        "nether_state_selector" => Some(include_str!("data/noise/nether_state_selector.json")),
        "nether_wart" => Some(include_str!("data/noise/nether_wart.json")),
        "nether/temperature" => Some(include_str!("data/noise/nether/temperature.json")),
        "nether/vegetation" => Some(include_str!("data/noise/nether/vegetation.json")),
        "netherrack" => Some(include_str!("data/noise/netherrack.json")),
        "patch" => Some(include_str!("data/noise/patch.json")),
        "soul_sand_layer" => Some(include_str!("data/noise/soul_sand_layer.json")),
        "jagged" => Some(include_str!("data/noise/jagged.json")),
        "noodle" => Some(include_str!("data/noise/noodle.json")),
        "noodle_ridge_a" => Some(include_str!("data/noise/noodle_ridge_a.json")),
        "noodle_ridge_b" => Some(include_str!("data/noise/noodle_ridge_b.json")),
        "noodle_thickness" => Some(include_str!("data/noise/noodle_thickness.json")),
        "offset" => Some(include_str!("data/noise/offset.json")),
        "ore_gap" => Some(include_str!("data/noise/ore_gap.json")),
        "ore_vein_a" => Some(include_str!("data/noise/ore_vein_a.json")),
        "ore_vein_b" => Some(include_str!("data/noise/ore_vein_b.json")),
        "ore_veininess" => Some(include_str!("data/noise/ore_veininess.json")),
        "packed_ice" => Some(include_str!("data/noise/packed_ice.json")),
        "pillar" => Some(include_str!("data/noise/pillar.json")),
        "pillar_rareness" => Some(include_str!("data/noise/pillar_rareness.json")),
        "pillar_thickness" => Some(include_str!("data/noise/pillar_thickness.json")),
        "powder_snow" => Some(include_str!("data/noise/powder_snow.json")),
        "ridge" => Some(include_str!("data/noise/ridge.json")),
        "spaghetti_2d" => Some(include_str!("data/noise/spaghetti_2d.json")),
        "spaghetti_2d_elevation" => Some(include_str!("data/noise/spaghetti_2d_elevation.json")),
        "spaghetti_2d_modulator" => Some(include_str!("data/noise/spaghetti_2d_modulator.json")),
        "spaghetti_2d_thickness" => Some(include_str!("data/noise/spaghetti_2d_thickness.json")),
        "spaghetti_3d_1" => Some(include_str!("data/noise/spaghetti_3d_1.json")),
        "spaghetti_3d_2" => Some(include_str!("data/noise/spaghetti_3d_2.json")),
        "spaghetti_3d_rarity" => Some(include_str!("data/noise/spaghetti_3d_rarity.json")),
        "spaghetti_3d_thickness" => Some(include_str!("data/noise/spaghetti_3d_thickness.json")),
        "spaghetti_roughness" => Some(include_str!("data/noise/spaghetti_roughness.json")),
        "spaghetti_roughness_modulator" => Some(include_str!("data/noise/spaghetti_roughness_modulator.json")),
        "surface" => Some(include_str!("data/noise/surface.json")),
        "surface_swamp" => Some(include_str!("data/noise/surface_swamp.json")),
        "temperature" => Some(include_str!("data/noise/temperature.json")),
        "vegetation" => Some(include_str!("data/noise/vegetation.json")),
        _ => None,
    }
}

