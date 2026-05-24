//! Hand-built `BlockModel` descriptors for blocks whose MC source
//! file ships per-cuboid geometry instead of a plain 6-face cube.
//!
//! The Wave 9.x.1 JSON parser can read MC's `block/<name>.json`
//! files when they're present on disk, but the engine ALSO needs to
//! render these shapes when the user doesn't have MC installed
//! (placeholder pack) AND while we're still building out the
//! chunk-mesher integration. This module returns a fully-resolved
//! `BlockModel` directly from in-code constants, keyed by the same
//! `block/<name>` strings the JSON loader uses.
//!
//! Each shape is cited against the upstream MC source so future
//! contributors can verify the geometry matches the vanilla look.
//! Citations refer to decompiled MC `26.1.2-sources` JSON files
//! under `assets/minecraft/models/block/`.
//!
//! Shapes shipped in W9.x.3:
//! - `torch` - thin stick `[7, 0, 7] -> [9, 10, 9]`
//!   (cite: `models/block/template_torch.json`)
//! - `wall_torch` - same stick tilted `-22.5deg` around Z
//!   (cite: `models/block/template_torch_wall.json`)
//! - `slab` (bottom) - `[0, 0, 0] -> [16, 8, 16]`
//!   (cite: `models/block/slab.json`)
//! - `slab_top` - `[0, 8, 0] -> [16, 16, 16]`
//!   (cite: `models/block/slab_top.json`)
//! - `fence_post` - centre post `[6, 0, 6] -> [10, 16, 10]`
//!   (cite: `models/block/fence_post.json` - connections deferred)
//! - `door_lower` - thin slab `[0, 0, 0] -> [3, 16, 16]`
//!   (cite: `models/block/door_bottom_left.json` - thickness 3/16)
//! - `bed_foot` - `[0, 0, 0] -> [16, 9, 16]` with side trim
//!   (cite: `models/block/bed_foot.json` - simplified to one cuboid)
//!
//! Deferred for later waves:
//! - Bed head (two-voxel bed split)
//! - Chest entity model (full LayerEntity render path)
//! - Fence side / wall side connection arms (multipart blockstate)
//! - Stairs (2-element shape - works with the existing parser
//!   once enabled in the mesher)

use std::collections::HashMap;

use super::models::{
    BlockModel, CubeFace, ElementFaces, ElementRotation, ModelElement,
    ModelFace, RotationAxis,
};
use super::{blockstates, models, name_map};

// ---------------------------------------------------------------------------
// W41.1: cached resolved block state
// ---------------------------------------------------------------------------

/// Pre-resolved per-block model data. Built once at pack load by
/// `build_block_model_cache` and stored in `ResourcePack::block_models`.
/// The chunk mesher reads from here instead of hitting the disk per-voxel.
#[derive(Debug, Clone)]
pub(crate) enum ResolvedBlockState {
    /// Single model for all states (variant-form blocks: stone, oak_log, ...).
    Single(BlockModel),
    /// Multipart block (fences, walls, panes). Each entry is an
    /// (optional condition, model) pair. `None` condition = unconditional
    /// (always applied). The mesher merges matching parts in-memory.
    Multipart(Vec<(Option<blockstates::MultipartCondition>, BlockModel)>),
    /// No MC blockstate / model data found. Fall back to the hand-coded
    /// special_models table or the default cube render path.
    None,
}

/// Populate `pack.block_models` for every entry in `BLOCK_NAME_MAP`.
/// Called once at end of `ResourcePack::load`. For the placeholder pack
/// (no JSON on disk) most ids land in `ResolvedBlockState::None` and the
/// mesher falls back to the hand-coded table - same behaviour as before W41.
pub(crate) fn build_block_model_cache(pack: &mut super::ResourcePack) {
    use super::name_map::BLOCK_NAME_MAP;
    let mut cache = HashMap::with_capacity(BLOCK_NAME_MAP.len());
    let mut single_count = 0usize;
    let mut multipart_count = 0usize;
    let mut none_count = 0usize;
    for entry in BLOCK_NAME_MAP {
        let id = entry.infengine_id;
        let resolved = resolve_for_cache(pack, id);
        // W46.11: emit INFO-level breadcrumbs for known-tricky blocks so
        // we can verify the JSON pipeline actually fired without enabling
        // tracing-debug. Keeps the log volume tiny (a single line per id).
        match &resolved {
            ResolvedBlockState::Single(m) => {
                single_count += 1;
                if matches!(id, 10 | 50 | 51 | 52 | 53) {
                    tracing::info!(
                        block_id = id,
                        block_name = entry.block,
                        elements = m.elements.len(),
                        textures = m.textures.len(),
                        "W46.11 cache: torch-family resolved to Single(JSON)",
                    );
                }
            }
            ResolvedBlockState::Multipart(parts) => {
                multipart_count += 1;
                if matches!(id, 10 | 50 | 51 | 52 | 53) {
                    tracing::info!(
                        block_id = id,
                        block_name = entry.block,
                        parts = parts.len(),
                        "W46.11 cache: torch-family resolved to Multipart(JSON)",
                    );
                }
            }
            ResolvedBlockState::None => {
                none_count += 1;
                if matches!(id, 10 | 50 | 51 | 52 | 53) {
                    tracing::warn!(
                        block_id = id,
                        block_name = entry.block,
                        "W46.11 cache: torch-family fell back to None (hand-built table)",
                    );
                }
            }
        }
        cache.insert(id, resolved);
    }
    tracing::info!(
        single = single_count,
        multipart = multipart_count,
        none = none_count,
        total = BLOCK_NAME_MAP.len(),
        "W46.11 cache breakdown",
    );
    pack.block_models = cache;
}

/// Resolve one block id to a `ResolvedBlockState` for the cache.
/// Reads disk ONCE per id (at cache-build time); during meshing the
/// cache is consulted instead.
///
/// W46.3: logs the resolution path so we can verify the JSON pipeline
/// is actually taking effect (e.g. for the torch root-cause hunt).
/// Each id is resolved once at pack load, so the log volume is bounded
/// by `BLOCK_NAME_MAP.len()` (a few dozen lines, info level).
fn resolve_for_cache(pack: &super::ResourcePack, id: u16) -> ResolvedBlockState {
    // id -> MC block name.
    let Some(map_entry) = name_map::lookup_block(id) else {
        tracing::debug!(block_id = id, "resolve_for_cache: no name_map entry, falling back to special_table");
        return resolve_from_special_table(id);
    };
    let block_name = map_entry.block;

    // Load blockstate JSON (may miss on placeholder pack -> fall through).
    let Some(state) = blockstates::load_blockstate(pack, block_name) else {
        // No blockstate on disk. Try hand-coded special_models table
        // so the placeholder pack still works.
        tracing::debug!(
            block_id = id,
            block_name = block_name,
            "resolve_for_cache: blockstate JSON missing, falling back to special_table",
        );
        return resolve_from_special_table(id);
    };

    if state.is_multipart() {
        // Pre-load every part's model.
        let mut parts: Vec<(Option<blockstates::MultipartCondition>, BlockModel)> =
            Vec::with_capacity(state.parts.len());
        for part in &state.parts {
            if let Some(m) = models::load_model(pack, &part.apply.model) {
                parts.push((part.when.clone(), m));
            }
        }
        if parts.is_empty() {
            tracing::debug!(
                block_id = id,
                block_name = block_name,
                "resolve_for_cache: multipart blockstate but no parts loaded, falling back",
            );
            return resolve_from_special_table(id);
        }
        tracing::debug!(
            block_id = id,
            block_name = block_name,
            parts = parts.len(),
            "resolve_for_cache: multipart via JSON pipeline",
        );
        ResolvedBlockState::Multipart(parts)
    } else {
        // Variant-form: pick first matching model (full state-matching deferred).
        let Some(model_ref) = state.first_variant_model() else {
            tracing::debug!(
                block_id = id,
                block_name = block_name,
                "resolve_for_cache: no variant model, falling back",
            );
            return resolve_from_special_table(id);
        };
        match models::load_model(pack, &model_ref.model) {
            Some(m) if !m.elements.is_empty() => {
                tracing::debug!(
                    block_id = id,
                    block_name = block_name,
                    model = model_ref.model.as_str(),
                    elements = m.elements.len(),
                    "resolve_for_cache: single variant via JSON pipeline",
                );
                ResolvedBlockState::Single(m)
            }
            Some(_) => {
                // W57.1: MC ships some blocks (chest, ender_chest,
                // shulker_box, banner, sign, ...) as element-less
                // model JSONs because the renderer is a
                // BlockEntityRenderer in vanilla. JSON pipeline
                // "succeeds" but yields nothing to draw. Fall
                // through to the hand-coded table so our static
                // mesh fallback gets a chance.
                tracing::debug!(
                    block_id = id,
                    block_name = block_name,
                    model = model_ref.model.as_str(),
                    "resolve_for_cache: model has zero elements (entity-rendered in MC), falling back",
                );
                resolve_from_special_table(id)
            }
            None => {
                tracing::debug!(
                    block_id = id,
                    block_name = block_name,
                    model = model_ref.model.as_str(),
                    "resolve_for_cache: model JSON load failed, falling back",
                );
                resolve_from_special_table(id)
            }
        }
    }
}

/// Try the hand-coded special_models table as a final fallback.
fn resolve_from_special_table(id: u16) -> ResolvedBlockState {
    if let Some(name) = name_for_block(id)
        && let Some(m) = lookup(name)
    {
        return ResolvedBlockState::Single(m);
    }
    ResolvedBlockState::None
}

/// Resolve a model name to a hand-built `BlockModel`. Returns
/// `None` for names that should fall through to the JSON loader
/// (or the default cube_all fallback). The W9.x.1 parser's lookup
/// path is the source of truth when the MC pack is on disk; this
/// table is the fallback when the pack is absent OR when the
/// W9.x scope deliberately overrides the JSON (e.g. simplified
/// slab without inheriting `block/block`'s display transforms).
pub(crate) fn lookup(name: &str) -> Option<BlockModel> {
    match name {
        "block/torch" | "torch" => Some(build_torch()),
        // W45.2: redstone + soul torch fallback paths (placeholder pack with
        // no MC JSON on disk). Hand-build them as torch-stick geometry with
        // their respective texture names so the placeholder pack still
        // renders something sensible.
        "block/redstone_torch" | "redstone_torch" => Some(build_redstone_torch()),
        "block/soul_torch" | "soul_torch" => Some(build_soul_torch()),
        "block/wall_torch" | "wall_torch" => Some(build_wall_torch()),
        "block/slab" | "slab" => Some(build_slab_bottom()),
        "block/slab_top" | "slab_top" => Some(build_slab_top()),
        "block/fence_post" | "fence_post" => Some(build_fence_post()),
        "block/door_lower" | "door_lower" => Some(build_door_lower()),
        "block/door_upper" | "door_upper" => Some(build_door_upper()),
        "block/trapdoor" | "trapdoor" => Some(build_trapdoor()),
        "block/chest" | "chest" => Some(build_chest()),
        "block/bed_foot" | "bed_foot" => Some(build_bed_foot()),
        // W12.2 - stairs / pane / bars / wall.
        "block/stairs" | "stairs" => Some(build_stairs()),
        "block/glass_pane" | "glass_pane" => Some(build_thin_pane()),
        "block/iron_bars" | "iron_bars" => Some(build_thin_pane()),
        "block/wall_post" | "wall_post" => Some(build_wall_post()),
        // W16.1 - cross-pattern for plant blocks. The texture var is
        // stored as the bare file stem; the mesher resolves it through
        // `ResourcePack::block_texture` the same way cube faces work.
        // The caller provides the texture name per-block so we accept
        // the generic "cross/<tex>" key shape.
        n if n.starts_with("block/cross/") => {
            let tex = n.trim_start_matches("block/cross/");
            Some(build_cross(tex))
        }
        _ => None,
    }
}

/// Map an infengine block id to its special-model name, if any.
/// Used by the chunk mesher to override the default 6-face cube
/// emission for known specials.
///
/// Cross-pattern plant blocks use the `"block/cross/<tex>"` key
/// shape so `lookup()` can extract the per-block texture name from
/// the key string without a second map lookup.
pub(crate) fn name_for_block(id: u16) -> Option<&'static str> {
    use crate::block::{
        COBBLESTONE_SLAB_ID, COBBLESTONE_STAIRS_ID, COBBLESTONE_WALL_ID,
        FLOWER_BLUE_ID, FLOWER_RED_ID, FLOWER_YELLOW_ID, GLASS_PANE_ID,
        IRON_BARS_ID, IRON_DOOR_ID, NETHER_BRICK_FENCE_ID, OAK_DOOR_ID,
        OAK_DOOR_UPPER_ID, OAK_FENCE_ID, OAK_SLAB_ID, OAK_SLAB_TOP_ID,
        OAK_STAIRS_ID, STONE_SLAB_ID, STONE_STAIRS_ID, TORCH_ID,
    };
    match id {
        x if x == TORCH_ID => Some("block/torch"),
        // W45.2: redstone + soul torch variants now route through the real
        // MC JSON blockstate (block/redstone_torch.json, block/soul_torch.json)
        // exactly like the regular torch. The old "block/cross/<tex>" hack
        // rendered them as flat X-cross quads which looked wrong; the JSON
        // model is the proper torch-stick geometry with the correct texture.
        50 => Some("block/redstone_torch"),
        51 => Some("block/redstone_torch"),
        52 => Some("block/soul_torch"),
        53 => Some("block/soul_torch"),
        // W16 - cross-pattern plant blocks. Key encodes the texture
        // stem so `lookup` can build the model without a second table.
        x if x == FLOWER_RED_ID => Some("block/cross/poppy"),
        x if x == FLOWER_YELLOW_ID => Some("block/cross/dandelion"),
        x if x == FLOWER_BLUE_ID => Some("block/cross/blue_orchid"),
        // W12.2 - slab / fence / door / stairs / pane / bars / wall.
        x if x == OAK_SLAB_ID
            || x == STONE_SLAB_ID
            || x == COBBLESTONE_SLAB_ID =>
        {
            Some("block/slab")
        }
        x if x == OAK_SLAB_TOP_ID => Some("block/slab_top"),
        x if x == OAK_STAIRS_ID
            || x == STONE_STAIRS_ID
            || x == COBBLESTONE_STAIRS_ID =>
        {
            Some("block/stairs")
        }
        x if x == OAK_FENCE_ID || x == NETHER_BRICK_FENCE_ID => {
            Some("block/fence_post")
        }
        x if x == OAK_DOOR_ID || x == IRON_DOOR_ID => Some("block/door_lower"),
        x if x == OAK_DOOR_UPPER_ID => Some("block/door_upper"),
        // W55.1: W48 door + trapdoor variants. JSON pipeline takes
        // precedence (loads real MC blockstate); these fallbacks
        // only fire when the JSON path returns None.
        548..=552 => Some("block/trapdoor"),
        568..=575 => Some("block/door_lower"),
        // W55.2: chest renders as an inset 14x14x14 box so it
        // visually stands apart from a plain plank block.
        x if x == crate::block::CHEST_ID => Some("block/chest"),
        x if x == GLASS_PANE_ID || x == IRON_BARS_ID => Some("block/glass_pane"),
        x if x == COBBLESTONE_WALL_ID => Some("block/wall_post"),
        _ => None,
    }
}

/// Resolve a block id to its MC block model via the full
/// blockstate -> variant -> model chain. Returns `None` when ANY
/// step fails (caller falls back to the special_models hand-coded
/// table or the default cube path).
///
/// W39.1: this is the primary resolution path for all blocks.
/// Resolution steps:
///   1. `name_map::lookup_block(id)` -> MC block name.
///   2. `blockstates::load_blockstate(pack, name)` -> `BlockState`.
///   3. `BlockState::first_variant_model()` -> `ModelRef` (first
///      variant; full state-matching deferred).
///   4. `models::load_model(pack, model_ref.model)` -> `BlockModel`
///      (walks the parent chain, resolves `#var` texture refs).
///
/// Multipart blockstates (fences, walls, redstone) still return
/// `None` here so the special_models hand-coded table gets a chance.
pub(crate) fn resolve_block_model(
    pack: &super::ResourcePack,
    id: u16,
) -> Option<BlockModel> {
    // Step 1: id -> MC block name.
    let map_entry = name_map::lookup_block(id)?;
    let block_name = map_entry.block;
    // Step 2: load blockstate JSON.
    let state = blockstates::load_blockstate(pack, block_name)?;
    // Step 3: pick first variant (full state-matching deferred).
    let model_ref = state.first_variant_model()?;
    // Step 4: load the model JSON (handles parent chain).
    // model_ref.model is already namespace-stripped by the blockstates
    // parser (strip_namespace is called in parse_model_ref). It may be
    // "block/stone", "block/oak_log", etc.
    models::load_model(pack, &model_ref.model)
}

/// Convenience: resolve a block model (no neighbor awareness).
///
/// W39.2: was the primary path. W41.1: delegates to the cache
/// so no disk I/O occurs during meshing.
/// W45.5: W42 gate removed. All blocks now route through the JSON
/// pipeline. Safety nets in place:
///   - W43.1: emit_block_elements returns Err when zero faces emit,
///     so the chunk mesher's cube path takes over for any model that
///     resolves but produces no visible faces.
///   - W44.5: air-as-opaque bug fixed, so cullface decisions are correct.
///   - W45.1: name_map entries corrected (slab/stair/fence/door IDs now
///     carry the real MC blockstate name, not "oak_planks"/"stone").
///
/// Resolution order:
///   1. `pack.block_models` cache (blockstate JSON -> model JSON).
///      Single or Multipart (unconditional parts merged).
///   2. Hand-coded `lookup` table (placeholder pack / cross plants).
///   3. None -> chunk mesher uses the greedy cube path.
pub(crate) fn model_for_block(id: u16, pack: &super::ResourcePack) -> Option<BlockModel> {
    if let Some(entry) = pack.block_models.get(&id) {
        match entry {
            ResolvedBlockState::Single(m) => return Some(m.clone()),
            ResolvedBlockState::Multipart(parts) => {
                let mut elements = Vec::new();
                let mut textures = std::collections::HashMap::new();
                let mut ambient_occlusion = true;
                for (cond, model) in parts {
                    if cond.is_none() {
                        elements.extend(model.elements.clone());
                        for (k, v) in &model.textures {
                            textures.insert(k.clone(), v.clone());
                        }
                        ambient_occlusion = model.ambient_occlusion;
                    }
                }
                if !elements.is_empty() {
                    return Some(BlockModel { elements, textures, ambient_occlusion });
                }
            }
            ResolvedBlockState::None => {}
        }
    }
    // Fall through to hand-coded table (placeholder pack / cross plants).
    name_for_block(id).and_then(lookup)
}

/// Resolve a block model taking neighbour connection flags into account.
///
/// W40.1: extends `resolve_block_model` to handle multipart blockstates.
/// W41.1: reads from the pre-built `pack.block_models` cache instead of
/// hitting disk. No file I/O during meshing.
pub(crate) fn resolve_block_model_with_neighbors(
    pack: &super::ResourcePack,
    id: u16,
    neighbors: &blockstates::NeighborStates,
) -> Option<BlockModel> {
    match pack.block_models.get(&id)? {
        ResolvedBlockState::None => None,
        ResolvedBlockState::Single(m) => Some(m.clone()),
        ResolvedBlockState::Multipart(parts) => {
            // Merge all matching parts in-memory - no disk I/O.
            let mut elements = Vec::new();
            let mut textures = std::collections::HashMap::new();
            let mut ambient_occlusion = true;
            for (cond, model) in parts {
                let matches = match cond {
                    None => true,
                    Some(c) => c.matches_neighbors(neighbors),
                };
                if matches {
                    elements.extend(model.elements.clone());
                    // Merge textures; later parts override earlier ones.
                    for (k, v) in &model.textures {
                        textures.insert(k.clone(), v.clone());
                    }
                    ambient_occlusion = model.ambient_occlusion;
                }
            }
            if elements.is_empty() {
                None
            } else {
                Some(BlockModel { elements, textures, ambient_occlusion })
            }
        }
    }
}

/// Neighbour-aware variant of `model_for_block`.
///
/// W40.1: tries the multipart-aware pipeline first; then the variant
/// pipeline; then the hand-coded special_models fallback.
/// W41.1: reads from cache (no disk I/O during meshing).
/// W45.5: W42 gate removed. All blocks with a cache entry route
/// through the multipart-aware JSON pipeline.
pub(crate) fn model_for_block_with_neighbors(
    id: u16,
    pack: &super::ResourcePack,
    neighbors: &blockstates::NeighborStates,
) -> Option<BlockModel> {
    if let Some(m) = resolve_block_model_with_neighbors(pack, id, neighbors) {
        return Some(m);
    }
    // Fall through to hand-coded table (placeholder pack / cross plants).
    name_for_block(id).and_then(lookup)
}

// ---------------------------------------------------------------
// Builders. Each returns a `BlockModel` with textures already
// resolved against the W9 atlas key shape (file stems, not
// `block/<name>` paths). The chunk mesher feeds the file stem
// into `ResourcePack::block_texture` for the final UV rect.
// ---------------------------------------------------------------

/// Build a face with the standard `[0, 0, 16, 16]` full-tile UV.
fn full_face(texture: &str) -> ModelFace {
    ModelFace {
        uv: [0.0, 0.0, 16.0, 16.0],
        texture: texture.to_string(),
        cullface: None,
        tintindex: None,
        rotation: 0,
    }
}

/// Build a face with an explicit UV rect (in 16ths of a tile).
fn uv_face(uv: [f32; 4], texture: &str, cullface: Option<CubeFace>) -> ModelFace {
    ModelFace {
        uv,
        texture: texture.to_string(),
        cullface,
        tintindex: None,
        rotation: 0,
    }
}

/// Full-cube faces - all six sample the same texture stem with the
/// `[0, 0, 16, 16]` full tile. Used by slab + fence-post side
/// faces.
fn cube_all_faces(texture: &str) -> ElementFaces {
    ElementFaces {
        down: Some(full_face(texture)),
        up: Some(full_face(texture)),
        north: Some(full_face(texture)),
        south: Some(full_face(texture)),
        west: Some(full_face(texture)),
        east: Some(full_face(texture)),
    }
}

/// Torch stick - mirrors `template_torch.json` exactly. Note that
/// MC samples a sub-rect of the torch texture for the side faces
/// (the lit tip), so we keep the `[7, 6, 9, 16]` UV for sides.
///
/// W36.2: texture key changed from "torch" to "torch" so it matches
/// the MC pack filename `torch_on.png`. The placeholder pack registers
/// both "torch" and "torch" for block id 10 (see fallback.rs).
pub(crate) fn build_torch() -> BlockModel {
    let mut textures = HashMap::new();
    textures.insert("torch".to_string(), "torch".to_string());
    BlockModel {
        elements: vec![ModelElement {
            from: [7.0, 0.0, 7.0],
            to: [9.0, 10.0, 9.0],
            rotation: None,
            shade: false,
            faces: ElementFaces {
                // Cite: template_torch.json down/up/sides.
                down: Some(uv_face([7.0, 13.0, 9.0, 15.0], "torch", Some(CubeFace::Down))),
                up: Some(uv_face([7.0, 6.0, 9.0, 8.0], "torch", None)),
                north: Some(uv_face([7.0, 6.0, 9.0, 16.0], "torch", None)),
                east: Some(uv_face([7.0, 6.0, 9.0, 16.0], "torch", None)),
                south: Some(uv_face([7.0, 6.0, 9.0, 16.0], "torch", None)),
                west: Some(uv_face([7.0, 6.0, 9.0, 16.0], "torch", None)),
            },
        }],
        textures,
        ambient_occlusion: false,
    }
}

/// Wall torch - the `template_torch_wall.json` shape: a 2-wide
/// torch tilted `-22.5deg` around Z so the flame leans away from
/// the wall. Origin sits at the wall mount, NOT the floor.
///
/// W36.2: texture key changed to "torch" to match MC pack filename.
pub(crate) fn build_wall_torch() -> BlockModel {
    let mut textures = HashMap::new();
    textures.insert("torch".to_string(), "torch".to_string());
    BlockModel {
        elements: vec![ModelElement {
            // Cite: template_torch_wall.json from/to/rotation.
            from: [-1.0, 3.5, 7.0],
            to: [1.0, 13.5, 9.0],
            rotation: Some(ElementRotation {
                origin: [0.0, 3.5, 8.0],
                axis: RotationAxis::Z,
                angle_deg: -22.5,
                rescale: false,
            }),
            shade: false,
            faces: ElementFaces {
                down: Some(uv_face([7.0, 13.0, 9.0, 15.0], "torch", None)),
                up: Some(uv_face([7.0, 6.0, 9.0, 8.0], "torch", None)),
                north: Some(uv_face([7.0, 6.0, 9.0, 16.0], "torch", None)),
                east: Some(uv_face([7.0, 6.0, 9.0, 16.0], "torch", None)),
                south: Some(uv_face([7.0, 6.0, 9.0, 16.0], "torch", None)),
                west: Some(uv_face([7.0, 6.0, 9.0, 16.0], "torch", None)),
            },
        }],
        textures,
        ambient_occlusion: false,
    }
}

/// Slab bottom-half. Cite: `slab.json` `from = [0,0,0] to =
/// [16,8,16]` + UV layout reuses the lower half of the side
/// texture (so a half-slab in stone reads "the bottom 8 pixels of
/// the stone texture wrap the sides").
pub(crate) fn build_slab_bottom() -> BlockModel {
    BlockModel {
        elements: vec![ModelElement {
            from: [0.0, 0.0, 0.0],
            to: [16.0, 8.0, 16.0],
            rotation: None,
            shade: true,
            faces: ElementFaces {
                down: Some(uv_face([0.0, 0.0, 16.0, 16.0], "#bottom", Some(CubeFace::Down))),
                up: Some(uv_face([0.0, 0.0, 16.0, 16.0], "#top", None)),
                north: Some(uv_face([0.0, 8.0, 16.0, 16.0], "#side", Some(CubeFace::North))),
                south: Some(uv_face([0.0, 8.0, 16.0, 16.0], "#side", Some(CubeFace::South))),
                west: Some(uv_face([0.0, 8.0, 16.0, 16.0], "#side", Some(CubeFace::West))),
                east: Some(uv_face([0.0, 8.0, 16.0, 16.0], "#side", Some(CubeFace::East))),
            },
        }],
        textures: HashMap::new(),
        ambient_occlusion: true,
    }
}

/// Slab top-half. Cite: `slab_top.json` `from = [0,8,0] to =
/// [16,16,16]`. Side UVs reverse so the lower 8 pixels of the
/// texture wrap the top half's sides (MC mirrors the slab to
/// keep the visible cube edge consistent).
pub(crate) fn build_slab_top() -> BlockModel {
    BlockModel {
        elements: vec![ModelElement {
            from: [0.0, 8.0, 0.0],
            to: [16.0, 16.0, 16.0],
            rotation: None,
            shade: true,
            faces: ElementFaces {
                down: Some(uv_face([0.0, 0.0, 16.0, 16.0], "#bottom", None)),
                up: Some(uv_face([0.0, 0.0, 16.0, 16.0], "#top", Some(CubeFace::Up))),
                north: Some(uv_face([0.0, 0.0, 16.0, 8.0], "#side", Some(CubeFace::North))),
                south: Some(uv_face([0.0, 0.0, 16.0, 8.0], "#side", Some(CubeFace::South))),
                west: Some(uv_face([0.0, 0.0, 16.0, 8.0], "#side", Some(CubeFace::West))),
                east: Some(uv_face([0.0, 0.0, 16.0, 8.0], "#side", Some(CubeFace::East))),
            },
        }],
        textures: HashMap::new(),
        ambient_occlusion: true,
    }
}

/// Fence post (the centre 4x16x4 cuboid). Cite: `fence_post.json`.
/// Side-connection arms are a follow-up that needs neighbour-aware
/// multipart blockstate dispatch.
pub(crate) fn build_fence_post() -> BlockModel {
    // W61.4: use literal `oak_planks` texture so isolated fence
    // posts render as a wood pillar instead of the magenta
    // __missing checker. nether_brick_fence still falls through
    // to JSON pipeline (multipart) on the real pack so it gets
    // its own texture there.
    BlockModel {
        elements: vec![ModelElement {
            from: [6.0, 0.0, 6.0],
            to: [10.0, 16.0, 10.0],
            rotation: None,
            shade: true,
            faces: cube_all_faces("oak_planks"),
        }],
        textures: HashMap::new(),
        ambient_occlusion: true,
    }
}

/// W59.1: Proper MC chest static-mesh geometry. Mirrors
/// `ChestRenderer.java` for the closed single-chest layout:
/// - Base   `(1, 0, 1)..(15, 10, 15)` - 14x10x14 box.
/// - Lid    `(1, 9, 1)..(15, 14, 15)` - 14x5x14 box sitting on top.
/// - Latch  `(7, 4, 0)..(9, 8, 1)`    - 2x4x1 knob on the north
///   face. North = Z- = MC's chest-front by convention.
///
/// Entity texture (`textures/entity/chest/normal.png`) isn't in
/// the atlas - keep planks_oak as the texture key so the
/// geometry reads as a wood chest. Entity-renderer + chest
/// sprite belongs to the future BlockEntityRenderer wave.
pub(crate) fn build_chest() -> BlockModel {
    let mk_face = |uv: [f32; 4], face: Option<CubeFace>| -> ModelFace {
        uv_face(uv, "oak_planks", face)
    };
    let cube = |from: [f32; 3], to: [f32; 3]| -> ModelElement {
        let dx = to[0] - from[0];
        let dy = to[1] - from[1];
        let dz = to[2] - from[2];
        ModelElement {
            from,
            to,
            rotation: None,
            shade: true,
            faces: ElementFaces {
                down: Some(mk_face([0.0, 0.0, dx, dz], Some(CubeFace::Down))),
                up: Some(mk_face([0.0, 0.0, dx, dz], Some(CubeFace::Up))),
                north: Some(mk_face([0.0, 0.0, dx, dy], Some(CubeFace::North))),
                south: Some(mk_face([0.0, 0.0, dx, dy], Some(CubeFace::South))),
                west: Some(mk_face([0.0, 0.0, dz, dy], Some(CubeFace::West))),
                east: Some(mk_face([0.0, 0.0, dz, dy], Some(CubeFace::East))),
            },
        }
    };
    BlockModel {
        elements: vec![
            cube([1.0, 0.0, 1.0], [15.0, 10.0, 15.0]),   // base
            cube([1.0, 9.0, 1.0], [15.0, 14.0, 15.0]),   // lid
            cube([7.0, 4.0, 0.0], [9.0, 8.0, 1.0]),       // latch
        ],
        textures: HashMap::new(),
        ambient_occlusion: true,
    }
}

/// W55.1: Trapdoor (closed, bottom-resting). 16x3x16 slab on the
/// ground. Cite `template_orientable_trapdoor_bottom.json`. The
/// JSON pipeline picks the correct open / orientation variant from
/// MC's blockstate; this hand-coded fallback covers the placeholder
/// pack + the "closed, bottom" default state.
pub(crate) fn build_trapdoor() -> BlockModel {
    BlockModel {
        elements: vec![ModelElement {
            from: [0.0, 0.0, 0.0],
            to: [16.0, 3.0, 16.0],
            rotation: None,
            shade: true,
            faces: ElementFaces {
                down: Some(uv_face([0.0, 0.0, 16.0, 16.0], "#texture", Some(CubeFace::Down))),
                up: Some(uv_face([0.0, 0.0, 16.0, 16.0], "#texture", Some(CubeFace::Up))),
                north: Some(uv_face([0.0, 0.0, 16.0, 3.0], "#texture", Some(CubeFace::North))),
                south: Some(uv_face([0.0, 0.0, 16.0, 3.0], "#texture", Some(CubeFace::South))),
                west: Some(uv_face([0.0, 0.0, 16.0, 3.0], "#texture", Some(CubeFace::West))),
                east: Some(uv_face([0.0, 0.0, 16.0, 3.0], "#texture", Some(CubeFace::East))),
            },
        }],
        textures: HashMap::new(),
        ambient_occlusion: true,
    }
}

/// Door lower half. Cite: `door_bottom_left.json` - a 3/16-thick
/// slab on the X axis with the full 16x16 texture mapped to the
/// big faces and a 3x16 strip mapped to the edge faces.
pub(crate) fn build_door_lower() -> BlockModel {
    BlockModel {
        elements: vec![ModelElement {
            from: [0.0, 0.0, 0.0],
            to: [3.0, 16.0, 16.0],
            rotation: None,
            shade: true,
            faces: ElementFaces {
                down: Some(uv_face([0.0, 0.0, 3.0, 16.0], "#bottom", None)),
                up: Some(uv_face([0.0, 0.0, 3.0, 16.0], "#top", None)),
                north: Some(uv_face([0.0, 0.0, 3.0, 16.0], "#side", None)),
                south: Some(uv_face([13.0, 0.0, 16.0, 16.0], "#side", None)),
                west: Some(uv_face([0.0, 0.0, 16.0, 16.0], "#bottom", Some(CubeFace::West))),
                east: Some(uv_face([0.0, 0.0, 16.0, 16.0], "#bottom", Some(CubeFace::East))),
            },
        }],
        textures: HashMap::new(),
        ambient_occlusion: true,
    }
}

/// Door upper half. Cite: `door_top_left.json` - mirror of the
/// lower half with the texture sampled from the upper door image
/// (the engine resolves `door_oak_upper.png` per the W12.2
/// BlockType row's `top`/`bottom`/`sides` tile spec).
pub(crate) fn build_door_upper() -> BlockModel {
    BlockModel {
        elements: vec![ModelElement {
            from: [0.0, 0.0, 0.0],
            to: [3.0, 16.0, 16.0],
            rotation: None,
            shade: true,
            faces: ElementFaces {
                down: Some(uv_face([0.0, 0.0, 3.0, 16.0], "#bottom", None)),
                up: Some(uv_face([0.0, 0.0, 3.0, 16.0], "#top", None)),
                north: Some(uv_face([0.0, 0.0, 3.0, 16.0], "#side", None)),
                south: Some(uv_face([13.0, 0.0, 16.0, 16.0], "#side", None)),
                west: Some(uv_face([0.0, 0.0, 16.0, 16.0], "#top", None)),
                east: Some(uv_face([0.0, 0.0, 16.0, 16.0], "#top", None)),
            },
        }],
        textures: HashMap::new(),
        ambient_occlusion: true,
    }
}

/// Stairs (east-facing default). Cite: `models/block/stairs.json`.
///
/// 2 elements:
/// - bottom slab `[0,0,0]->[16,8,16]`
/// - upper half step `[8,8,0]->[16,16,16]` (visible east-side step).
pub(crate) fn build_stairs() -> BlockModel {
    let bottom = ModelElement {
        from: [0.0, 0.0, 0.0],
        to: [16.0, 8.0, 16.0],
        rotation: None,
        shade: true,
        faces: ElementFaces {
            down: Some(uv_face([0.0, 0.0, 16.0, 16.0], "#bottom", Some(CubeFace::Down))),
            up: Some(uv_face([0.0, 0.0, 16.0, 16.0], "#top", None)),
            north: Some(uv_face([0.0, 8.0, 16.0, 16.0], "#side", Some(CubeFace::North))),
            south: Some(uv_face([0.0, 8.0, 16.0, 16.0], "#side", Some(CubeFace::South))),
            west: Some(uv_face([0.0, 8.0, 16.0, 16.0], "#side", Some(CubeFace::West))),
            east: Some(uv_face([0.0, 8.0, 16.0, 16.0], "#side", Some(CubeFace::East))),
        },
    };
    let step = ModelElement {
        from: [8.0, 8.0, 0.0],
        to: [16.0, 16.0, 16.0],
        rotation: None,
        shade: true,
        faces: ElementFaces {
            down: Some(uv_face([8.0, 0.0, 16.0, 16.0], "#top", None)),
            up: Some(uv_face([8.0, 0.0, 16.0, 16.0], "#top", Some(CubeFace::Up))),
            north: Some(uv_face([0.0, 0.0, 8.0, 8.0], "#side", Some(CubeFace::North))),
            south: Some(uv_face([0.0, 0.0, 8.0, 8.0], "#side", Some(CubeFace::South))),
            west: Some(uv_face([0.0, 0.0, 16.0, 8.0], "#side", None)),
            east: Some(uv_face([0.0, 0.0, 16.0, 8.0], "#side", Some(CubeFace::East))),
        },
    };
    BlockModel {
        elements: vec![bottom, step],
        textures: HashMap::new(),
        ambient_occlusion: true,
    }
}

/// Thin pane / iron bars centre post. Cite:
/// `models/block/glass_pane_post.json` + `models/block/iron_bars_post.json`.
/// 2x16x2 centre column; the 4 connection arms are a follow-up
/// (multipart blockstate). Glass pane and iron bars share the
/// same post geometry; only the texture differs.
pub(crate) fn build_thin_pane() -> BlockModel {
    // W61.4: literal `glass` texture so the center pane renders
    // instead of magenta __missing. Iron bars / stained panes
    // route through JSON pipeline first when MC pack present.
    BlockModel {
        elements: vec![ModelElement {
            from: [7.0, 0.0, 7.0],
            to: [9.0, 16.0, 9.0],
            rotation: None,
            shade: true,
            faces: cube_all_faces("glass"),
        }],
        textures: HashMap::new(),
        ambient_occlusion: true,
    }
}

/// Cobblestone wall post. Cite: `models/block/template_wall_post.json`
/// -> centre 8x16x8 column. Side arms (`template_wall_side.json`)
/// are a follow-up tied to the multipart blockstate dispatch.
pub(crate) fn build_wall_post() -> BlockModel {
    // W61.4: literal `cobblestone` texture so isolated walls
    // render correctly.
    BlockModel {
        elements: vec![ModelElement {
            from: [4.0, 0.0, 4.0],
            to: [12.0, 16.0, 12.0],
            rotation: None,
            shade: true,
            faces: cube_all_faces("cobblestone"),
        }],
        textures: HashMap::new(),
        ambient_occlusion: true,
    }
}

/// Bed foot. Cite: `bed_foot.json` (simplified to one cuboid -
/// the real MC bed splits foot vs head into two voxels with
/// shared `bed.json` textures).
pub(crate) fn build_bed_foot() -> BlockModel {
    BlockModel {
        elements: vec![ModelElement {
            from: [0.0, 0.0, 0.0],
            to: [16.0, 9.0, 16.0],
            rotation: None,
            shade: true,
            faces: cube_all_faces("#texture"),
        }],
        textures: HashMap::new(),
        ambient_occlusion: true,
    }
}

/// Redstone torch stick - same geometry as `build_torch()` but the
/// texture key is "redstone_torch" so it samples the correct atlas tile.
/// Used as the placeholder-pack fallback when no MC JSON is on disk.
/// W45.2: primary path now goes through block/redstone_torch.json.
pub(crate) fn build_redstone_torch() -> BlockModel {
    let mut textures = HashMap::new();
    textures.insert("torch".to_string(), "redstone_torch".to_string());
    BlockModel {
        elements: vec![ModelElement {
            from: [7.0, 0.0, 7.0],
            to: [9.0, 10.0, 9.0],
            rotation: None,
            shade: false,
            faces: ElementFaces {
                down: Some(uv_face([7.0, 13.0, 9.0, 15.0], "torch", Some(CubeFace::Down))),
                up: Some(uv_face([7.0, 6.0, 9.0, 8.0], "torch", None)),
                north: Some(uv_face([7.0, 6.0, 9.0, 16.0], "torch", None)),
                east: Some(uv_face([7.0, 6.0, 9.0, 16.0], "torch", None)),
                south: Some(uv_face([7.0, 6.0, 9.0, 16.0], "torch", None)),
                west: Some(uv_face([7.0, 6.0, 9.0, 16.0], "torch", None)),
            },
        }],
        textures,
        ambient_occlusion: false,
    }
}

/// Soul torch stick - same geometry as `build_torch()` but texture
/// key is "soul_torch". Used as placeholder-pack fallback.
/// W45.2: primary path now goes through block/soul_torch.json.
pub(crate) fn build_soul_torch() -> BlockModel {
    let mut textures = HashMap::new();
    textures.insert("torch".to_string(), "soul_torch".to_string());
    BlockModel {
        elements: vec![ModelElement {
            from: [7.0, 0.0, 7.0],
            to: [9.0, 10.0, 9.0],
            rotation: None,
            shade: false,
            faces: ElementFaces {
                down: Some(uv_face([7.0, 13.0, 9.0, 15.0], "torch", Some(CubeFace::Down))),
                up: Some(uv_face([7.0, 6.0, 9.0, 8.0], "torch", None)),
                north: Some(uv_face([7.0, 6.0, 9.0, 16.0], "torch", None)),
                east: Some(uv_face([7.0, 6.0, 9.0, 16.0], "torch", None)),
                south: Some(uv_face([7.0, 6.0, 9.0, 16.0], "torch", None)),
                west: Some(uv_face([7.0, 6.0, 9.0, 16.0], "torch", None)),
            },
        }],
        textures,
        ambient_occlusion: false,
    }
}

/// MC `cross.json` shape - two quads forming an X, used for plant
/// blocks (flowers, dandelions, short_grass, saplings).
///
/// Cite: `assets/minecraft/models/block/cross.json`.
/// Each quad is a flat, infinitely-thin face that spans the full
/// 16x16 tile in Y. The two quads are offset slightly off-centre
/// (0.8 from the block edge rather than exactly 0 or 8) so they
/// form a proper X that is visible from all horizontal directions.
/// Both quads carry `shade: false` (matches MC's `"shade": false`
/// in cross.json) so they don't darken on the underside.
pub(crate) fn build_cross(texture: &str) -> BlockModel {
    // Quad 1: runs north-south (along Z), tilted 45deg around Y.
    let quad1 = ModelElement {
        from: [0.8, 0.0, 8.0],
        to: [15.2, 16.0, 8.0],
        rotation: Some(ElementRotation {
            origin: [8.0, 8.0, 8.0],
            axis: RotationAxis::Y,
            angle_deg: 45.0,
            rescale: true,
        }),
        shade: false,
        faces: ElementFaces {
            north: Some(full_face(texture)),
            south: Some(full_face(texture)),
            ..Default::default()
        },
    };
    // Quad 2: runs east-west (along X), tilted 45deg around Y.
    let quad2 = ModelElement {
        from: [8.0, 0.0, 0.8],
        to: [8.0, 16.0, 15.2],
        rotation: Some(ElementRotation {
            origin: [8.0, 8.0, 8.0],
            axis: RotationAxis::Y,
            angle_deg: 45.0,
            rescale: true,
        }),
        shade: false,
        faces: ElementFaces {
            east: Some(full_face(texture)),
            west: Some(full_face(texture)),
            ..Default::default()
        },
    };
    BlockModel {
        elements: vec![quad1, quad2],
        textures: HashMap::new(),
        ambient_occlusion: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// W38.1: all model_for_block tests need a pack.  The placeholder
    /// pack has no JSON model files so the loader always falls back to
    /// the hand-coded table, preserving existing test behaviour.
    fn placeholder() -> crate::resource_pack::ResourcePack {
        crate::resource_pack::fallback::placeholder_pack()
    }

    #[test]
    fn torch_emits_one_element_with_six_faces() {
        let m = build_torch();
        assert_eq!(m.elements.len(), 1);
        let e = &m.elements[0];
        assert_eq!(e.from, [7.0, 0.0, 7.0]);
        assert_eq!(e.to, [9.0, 10.0, 9.0]);
        // All 6 faces present (the stick is visible from every side).
        assert!(e.faces.down.is_some());
        assert!(e.faces.up.is_some());
        assert!(e.faces.north.is_some());
        assert!(e.faces.south.is_some());
        assert!(e.faces.east.is_some());
        assert!(e.faces.west.is_some());
        assert!(!m.ambient_occlusion);
        assert!(!e.shade);
    }

    #[test]
    fn torch_stick_geometry_correct() {
        // Cross-reference: template_torch.json `from`/`to` MUST
        // match exactly so the rendered torch sits in the centre
        // of the voxel and stops at 10/16 height.
        let m = build_torch();
        let e = &m.elements[0];
        assert_eq!(e.from, [7.0, 0.0, 7.0]);
        assert_eq!(e.to, [9.0, 10.0, 9.0]);
    }

    #[test]
    fn wall_torch_applies_z_axis_rotation() {
        let m = build_wall_torch();
        let e = &m.elements[0];
        let r = e.rotation.expect("wall torch has rotation");
        assert_eq!(r.axis, RotationAxis::Z);
        assert!((r.angle_deg + 22.5).abs() < 1e-3);
        assert_eq!(r.origin, [0.0, 3.5, 8.0]);
        // Wall torch reaches into negative X (sticks out from the
        // wall in the -X direction before the rotation pulls it
        // back).
        assert_eq!(e.from, [-1.0, 3.5, 7.0]);
        assert_eq!(e.to, [1.0, 13.5, 9.0]);
    }

    #[test]
    fn slab_bottom_emits_top_face_at_y_8() {
        // Slab bottom: `from.y = 0, to.y = 8`. Renderer reads
        // `to.y / 16.0` for the world-space height; 8/16 = 0.5.
        let m = build_slab_bottom();
        let e = &m.elements[0];
        assert_eq!(e.from[1], 0.0);
        assert_eq!(e.to[1], 8.0);
        let up = e.faces.up.as_ref().expect("up face present");
        assert_eq!(up.uv, [0.0, 0.0, 16.0, 16.0]);
        // Top face has NO cullface (slab top in bottom-half is
        // exposed unless capped by another slab).
        assert!(up.cullface.is_none());
    }

    #[test]
    fn slab_top_emits_bottom_face_at_y_8() {
        let m = build_slab_top();
        let e = &m.elements[0];
        assert_eq!(e.from[1], 8.0);
        assert_eq!(e.to[1], 16.0);
        // Top face culls against the cell above (it sits at
        // full y=16/16).
        assert_eq!(e.faces.up.as_ref().unwrap().cullface, Some(CubeFace::Up));
    }

    #[test]
    fn fence_post_centre_only() {
        let m = build_fence_post();
        let e = &m.elements[0];
        // Centre 4x4 footprint, full height. No connections in W9.x.
        assert_eq!(e.from, [6.0, 0.0, 6.0]);
        assert_eq!(e.to, [10.0, 16.0, 10.0]);
    }

    #[test]
    fn door_lower_is_thin_slab() {
        let m = build_door_lower();
        let e = &m.elements[0];
        assert_eq!(e.from[0], 0.0);
        assert_eq!(e.to[0], 3.0);
        assert_eq!(e.to[1], 16.0);
        assert_eq!(e.to[2], 16.0);
    }

    #[test]
    fn bed_foot_emits_9_high_block() {
        let m = build_bed_foot();
        let e = &m.elements[0];
        assert_eq!(e.from, [0.0, 0.0, 0.0]);
        assert_eq!(e.to, [16.0, 9.0, 16.0]);
    }

    #[test]
    fn lookup_accepts_block_prefix() {
        assert!(lookup("block/torch").is_some());
        assert!(lookup("torch").is_some());
        assert!(lookup("block/slab").is_some());
        assert!(lookup("slab").is_some());
        assert!(lookup("nonsense").is_none());
    }

    #[test]
    fn name_for_torch_block_id_routes() {
        use crate::block::TORCH_ID;
        assert_eq!(name_for_block(TORCH_ID), Some("block/torch"));
        // Stone id 1 has no special model.
        assert_eq!(name_for_block(1), None);
    }

    #[test]
    fn model_for_block_returns_full_torch_model() {
        use crate::block::TORCH_ID;
        let pack = placeholder();
        let m = model_for_block(TORCH_ID, &pack).expect("torch model");
        assert_eq!(m.elements.len(), 1);
        assert_eq!(m.elements[0].from, [7.0, 0.0, 7.0]);
    }

    // ---------------------------------------------------------------
    // W12.2 - Slab / fence / door / stairs / pane / wall routing.
    // ---------------------------------------------------------------

    fn w12_ids() -> &'static [u16] {
        use crate::block::{
            COBBLESTONE_SLAB_ID, COBBLESTONE_STAIRS_ID, COBBLESTONE_WALL_ID,
            GLASS_PANE_ID, IRON_BARS_ID, IRON_DOOR_ID, NETHER_BRICK_FENCE_ID,
            OAK_DOOR_ID, OAK_DOOR_UPPER_ID, OAK_FENCE_ID, OAK_SLAB_ID,
            OAK_SLAB_TOP_ID, OAK_STAIRS_ID, STONE_SLAB_ID, STONE_STAIRS_ID,
        };
        // Boxed to a 'static via `Box::leak`. Cheap; runs once.
        static IDS: std::sync::OnceLock<Vec<u16>> = std::sync::OnceLock::new();
        IDS.get_or_init(|| {
            vec![
                OAK_SLAB_ID, STONE_SLAB_ID, COBBLESTONE_SLAB_ID,
                OAK_SLAB_TOP_ID, OAK_STAIRS_ID, STONE_STAIRS_ID,
                COBBLESTONE_STAIRS_ID, OAK_FENCE_ID, NETHER_BRICK_FENCE_ID,
                OAK_DOOR_ID, OAK_DOOR_UPPER_ID, IRON_DOOR_ID,
                GLASS_PANE_ID, IRON_BARS_ID, COBBLESTONE_WALL_ID,
            ]
        })
    }

    #[test]
    fn slab_bottom_routes_to_block_slab() {
        use crate::block::{COBBLESTONE_SLAB_ID, OAK_SLAB_ID, STONE_SLAB_ID};
        assert_eq!(name_for_block(OAK_SLAB_ID), Some("block/slab"));
        assert_eq!(name_for_block(STONE_SLAB_ID), Some("block/slab"));
        assert_eq!(name_for_block(COBBLESTONE_SLAB_ID), Some("block/slab"));
    }

    #[test]
    fn slab_top_routes_to_block_slab_top() {
        use crate::block::OAK_SLAB_TOP_ID;
        assert_eq!(name_for_block(OAK_SLAB_TOP_ID), Some("block/slab_top"));
    }

    #[test]
    fn slab_bottom_emits_top_face_at_y_0_5() {
        use crate::block::OAK_SLAB_ID;
        // Slab model: `to[1] = 8.0`, which means top face sits
        // at y = 8/16 = 0.5 in voxel-local space.
        let pack = placeholder();
        let m = model_for_block(OAK_SLAB_ID, &pack).expect("oak slab routes");
        let e = &m.elements[0];
        let y_top_frac = e.to[1] / 16.0;
        assert!(
            (y_top_frac - 0.5).abs() < 1e-3,
            "slab bottom top-face at y = {y_top_frac}, want 0.5",
        );
    }

    #[test]
    fn door_lower_emits_thin_slab_quad() {
        use crate::block::OAK_DOOR_ID;
        let pack = placeholder();
        let m = model_for_block(OAK_DOOR_ID, &pack).expect("oak door routes");
        let e = &m.elements[0];
        assert!((e.to[0] - 3.0).abs() < 1e-3, "door not 3/16 thick");
        assert_eq!(e.to[1], 16.0);
        assert_eq!(e.to[2], 16.0);
    }

    #[test]
    fn fence_post_visible_in_chunk_mesh() {
        use crate::block::OAK_FENCE_ID;
        let pack = placeholder();
        let m = model_for_block(OAK_FENCE_ID, &pack).expect("oak fence routes");
        let e = &m.elements[0];
        assert_eq!(e.from, [6.0, 0.0, 6.0]);
        assert_eq!(e.to, [10.0, 16.0, 10.0]);
    }

    #[test]
    fn stairs_emits_two_elements() {
        use crate::block::OAK_STAIRS_ID;
        let pack = placeholder();
        let m = model_for_block(OAK_STAIRS_ID, &pack).expect("oak stairs routes");
        assert_eq!(m.elements.len(), 2);
        assert_eq!(m.elements[0].to, [16.0, 8.0, 16.0]);
        assert_eq!(m.elements[1].from, [8.0, 8.0, 0.0]);
    }

    #[test]
    fn glass_pane_and_iron_bars_share_post_shape() {
        use crate::block::{GLASS_PANE_ID, IRON_BARS_ID};
        let pack = placeholder();
        let gp = model_for_block(GLASS_PANE_ID, &pack).expect("glass pane routes");
        let ib = model_for_block(IRON_BARS_ID, &pack).expect("iron bars routes");
        assert_eq!(gp.elements[0].from, [7.0, 0.0, 7.0]);
        assert_eq!(gp.elements[0].to, [9.0, 16.0, 9.0]);
        assert_eq!(ib.elements[0].from, [7.0, 0.0, 7.0]);
        assert_eq!(ib.elements[0].to, [9.0, 16.0, 9.0]);
    }

    #[test]
    fn cobblestone_wall_post_centre_only() {
        use crate::block::COBBLESTONE_WALL_ID;
        let pack = placeholder();
        let m = model_for_block(COBBLESTONE_WALL_ID, &pack).expect("wall routes");
        let e = &m.elements[0];
        assert_eq!(e.from, [4.0, 0.0, 4.0]);
        assert_eq!(e.to, [12.0, 16.0, 12.0]);
    }

    #[test]
    fn door_upper_distinct_from_door_lower() {
        use crate::block::{OAK_DOOR_ID, OAK_DOOR_UPPER_ID};
        assert_eq!(name_for_block(OAK_DOOR_ID), Some("block/door_lower"));
        assert_eq!(
            name_for_block(OAK_DOOR_UPPER_ID),
            Some("block/door_upper"),
        );
    }

    #[test]
    fn iron_door_shares_lower_geometry() {
        use crate::block::{IRON_DOOR_ID, OAK_DOOR_ID};
        assert_eq!(name_for_block(IRON_DOOR_ID), name_for_block(OAK_DOOR_ID));
    }

    #[test]
    fn every_w12_block_resolves_a_model() {
        let pack = placeholder();
        for &id in w12_ids() {
            assert!(
                model_for_block(id, &pack).is_some(),
                "id {id} has no resolvable special model",
            );
        }
    }

    // ---------------------------------------------------------------
    // W16.1 - Cross-pattern model tests.
    // ---------------------------------------------------------------

    #[test]
    fn cross_model_has_two_elements_forming_x() {
        let m = build_cross("poppy");
        assert_eq!(m.elements.len(), 2, "cross needs exactly 2 quads");
        // Quad 1: north + south faces present, east + west absent.
        let q1 = &m.elements[0];
        assert!(q1.faces.north.is_some(), "quad1 north face missing");
        assert!(q1.faces.south.is_some(), "quad1 south face missing");
        assert!(q1.faces.east.is_none(), "quad1 east should be absent");
        assert!(q1.faces.west.is_none(), "quad1 west should be absent");
        // Quad 2: east + west faces present, north + south absent.
        let q2 = &m.elements[1];
        assert!(q2.faces.east.is_some(), "quad2 east face missing");
        assert!(q2.faces.west.is_some(), "quad2 west face missing");
        assert!(q2.faces.north.is_none(), "quad2 north should be absent");
        assert!(q2.faces.south.is_none(), "quad2 south should be absent");
    }

    #[test]
    fn cross_model_quads_are_centered_at_block_origin() {
        let m = build_cross("dandelion");
        // Both quads rotate around the block centre (8, 8, 8) at 45 deg Y.
        for (i, elem) in m.elements.iter().enumerate() {
            let r = elem
                .rotation
                .unwrap_or_else(|| panic!("element {i} has no rotation"));
            assert_eq!(r.origin, [8.0, 8.0, 8.0], "rotation origin not block centre");
            assert_eq!(r.axis, RotationAxis::Y, "rotation axis must be Y");
            assert!(
                (r.angle_deg - 45.0).abs() < 1e-3,
                "rotation angle must be 45 deg, got {}", r.angle_deg,
            );
            assert!(r.rescale, "cross quads must rescale to avoid gaps");
        }
    }

    #[test]
    fn cross_model_no_ambient_occlusion() {
        let m = build_cross("blue_orchid");
        assert!(!m.ambient_occlusion, "cross model must disable ambient occlusion");
    }

    #[test]
    fn cross_model_shade_false_on_both_quads() {
        let m = build_cross("poppy");
        for (i, elem) in m.elements.iter().enumerate() {
            assert!(!elem.shade, "element {i} shade must be false (matches cross.json)");
        }
    }

    #[test]
    fn cross_model_texture_name_propagates_to_faces() {
        let m = build_cross("my_flower");
        let q1 = &m.elements[0];
        let q2 = &m.elements[1];
        assert_eq!(q1.faces.north.as_ref().unwrap().texture, "my_flower");
        assert_eq!(q1.faces.south.as_ref().unwrap().texture, "my_flower");
        assert_eq!(q2.faces.east.as_ref().unwrap().texture, "my_flower");
        assert_eq!(q2.faces.west.as_ref().unwrap().texture, "my_flower");
    }

    #[test]
    fn lookup_cross_key_builds_model_with_texture() {
        let m = lookup("block/cross/poppy").expect("cross/poppy should resolve");
        assert_eq!(m.elements.len(), 2);
        // Texture on face should match the trailing stem.
        assert_eq!(
            m.elements[0].faces.north.as_ref().unwrap().texture,
            "poppy",
        );
    }

    #[test]
    fn flower_ids_route_to_distinct_cross_keys() {
        use crate::block::{FLOWER_BLUE_ID, FLOWER_RED_ID, FLOWER_YELLOW_ID};
        let red = name_for_block(FLOWER_RED_ID).expect("red flower routes");
        let yellow = name_for_block(FLOWER_YELLOW_ID).expect("yellow flower routes");
        let blue = name_for_block(FLOWER_BLUE_ID).expect("blue flower routes");
        assert!(red.starts_with("block/cross/"), "red must use cross key");
        assert!(yellow.starts_with("block/cross/"), "yellow must use cross key");
        assert!(blue.starts_with("block/cross/"), "blue must use cross key");
        // All three should have different texture stems.
        assert_ne!(red, yellow);
        assert_ne!(red, blue);
        assert_ne!(yellow, blue);
    }

    #[test]
    fn flower_model_for_block_resolves_cross_model() {
        use crate::block::FLOWER_RED_ID;
        let pack = placeholder();
        let m = model_for_block(FLOWER_RED_ID, &pack).expect("flower should resolve to cross model");
        assert_eq!(m.elements.len(), 2, "cross model has 2 quads");
        assert!(!m.ambient_occlusion);
    }

    #[test]
    fn cross_face_uvs_cover_full_tile() {
        let m = build_cross("poppy");
        // Full 16x16 tile UV (MC cross.json has full tile on each face).
        let north_uv = m.elements[0].faces.north.as_ref().unwrap().uv;
        assert_eq!(north_uv, [0.0, 0.0, 16.0, 16.0]);
        let east_uv = m.elements[1].faces.east.as_ref().unwrap().uv;
        assert_eq!(east_uv, [0.0, 0.0, 16.0, 16.0]);
    }

    #[test]
    fn cross_model_quad1_spans_full_block_height() {
        let m = build_cross("poppy");
        let q1 = &m.elements[0];
        assert_eq!(q1.from[1], 0.0, "quad1 bottom must be at y=0");
        assert_eq!(q1.to[1], 16.0, "quad1 top must be at y=16");
    }

    #[test]
    fn nether_brick_fence_uses_post_geometry() {
        use crate::block::NETHER_BRICK_FENCE_ID;
        let pack = placeholder();
        let m = model_for_block(NETHER_BRICK_FENCE_ID, &pack).expect("nb fence routes");
        let e = &m.elements[0];
        // Same fence-post 4x16x4 centre as oak.
        assert_eq!(e.from, [6.0, 0.0, 6.0]);
        assert_eq!(e.to, [10.0, 16.0, 10.0]);
    }

    #[test]
    fn full_face_uv_is_default_tile() {
        let f = full_face("dirt");
        assert_eq!(f.uv, [0.0, 0.0, 16.0, 16.0]);
        assert_eq!(f.texture, "dirt");
        assert!(f.cullface.is_none());
    }

    #[test]
    fn uv_face_preserves_rect() {
        let f = uv_face([1.0, 2.0, 3.0, 4.0], "stone", Some(CubeFace::Down));
        assert_eq!(f.uv, [1.0, 2.0, 3.0, 4.0]);
        assert_eq!(f.cullface, Some(CubeFace::Down));
    }

    #[test]
    fn slab_bottom_side_uvs_use_lower_half() {
        // Side faces of a bottom slab sample the LOWER half of
        // the texture (`v0 = 8` of 16) so the visible side edge
        // shows the bottom 8 pixels of the source.
        let m = build_slab_bottom();
        let e = &m.elements[0];
        let north = e.faces.north.as_ref().unwrap();
        assert_eq!(north.uv, [0.0, 8.0, 16.0, 16.0]);
    }

    #[test]
    fn slab_top_side_uvs_use_upper_half() {
        // Mirror of the bottom: side faces sample the UPPER half
        // (`v0 = 0` of 16, `v1 = 8`) because MC mirrors the
        // wrap so the visible cube edge stays consistent.
        let m = build_slab_top();
        let e = &m.elements[0];
        let north = e.faces.north.as_ref().unwrap();
        assert_eq!(north.uv, [0.0, 0.0, 16.0, 8.0]);
    }

    // ---------------------------------------------------------------
    // W45.2 - Redstone/soul torch JSON routing tests.
    // ---------------------------------------------------------------

    #[test]
    fn w45_redstone_torch_routes_to_block_redstone_torch() {
        // W45.2: ids 50 + 51 (redstone_torch, redstone_wall_torch) must
        // now map to "block/redstone_torch" - no longer the cross hack.
        assert_eq!(name_for_block(50), Some("block/redstone_torch"));
        assert_eq!(name_for_block(51), Some("block/redstone_torch"));
    }

    #[test]
    fn w45_soul_torch_routes_to_block_soul_torch() {
        // W45.2: ids 52 + 53 (soul_torch, soul_wall_torch) use JSON path.
        assert_eq!(name_for_block(52), Some("block/soul_torch"));
        assert_eq!(name_for_block(53), Some("block/soul_torch"));
    }

    #[test]
    fn w45_lookup_redstone_torch_builds_stick_geometry() {
        // Fallback lookup must produce torch-stick geometry (1 element,
        // from=[7,0,7] to=[9,10,9]) with "redstone_torch" texture.
        let m = lookup("block/redstone_torch").expect("redstone torch fallback");
        assert_eq!(m.elements.len(), 1);
        let e = &m.elements[0];
        assert_eq!(e.from, [7.0, 0.0, 7.0]);
        assert_eq!(e.to, [9.0, 10.0, 9.0]);
        // Texture map: "torch" -> "redstone_torch"
        assert_eq!(m.textures.get("torch").map(|s| s.as_str()), Some("redstone_torch"));
        assert!(!m.ambient_occlusion);
    }

    #[test]
    fn w45_lookup_soul_torch_builds_stick_geometry() {
        let m = lookup("block/soul_torch").expect("soul torch fallback");
        assert_eq!(m.elements.len(), 1);
        let e = &m.elements[0];
        assert_eq!(e.from, [7.0, 0.0, 7.0]);
        assert_eq!(e.to, [9.0, 10.0, 9.0]);
        assert_eq!(m.textures.get("torch").map(|s| s.as_str()), Some("soul_torch"));
    }

    #[test]
    fn w45_redstone_torch_model_for_block_resolves() {
        let pack = placeholder();
        // Both redstone torch IDs must resolve a model (via placeholder fallback).
        let m50 = model_for_block(50, &pack).expect("redstone torch id=50");
        let m51 = model_for_block(51, &pack).expect("redstone torch id=51");
        assert_eq!(m50.elements.len(), 1);
        assert_eq!(m51.elements.len(), 1);
        // Geometry same as regular torch.
        assert_eq!(m50.elements[0].from, [7.0, 0.0, 7.0]);
    }

    #[test]
    fn w45_soul_torch_model_for_block_resolves() {
        let pack = placeholder();
        let m52 = model_for_block(52, &pack).expect("soul torch id=52");
        let m53 = model_for_block(53, &pack).expect("soul torch id=53");
        assert_eq!(m52.elements.len(), 1);
        assert_eq!(m53.elements.len(), 1);
    }
}
