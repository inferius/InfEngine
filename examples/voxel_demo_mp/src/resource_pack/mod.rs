//! MC resource-pack loader (Wave 9).
//!
//! Loads block + item + particle textures (with `.mcmeta` animation
//! metadata), block models, and sound-event registries from a stock
//! Minecraft 26.1.2 install or any "vanilla layout" resource pack.
//! The engine does NOT redistribute Mojang assets - this module
//! READS them from the user's local install at runtime.
//!
//! Path resolution (top to bottom; first hit wins):
//! 1. `INFENGINE_MC_ASSETS` env var (absolute path to an
//!    `assets/minecraft/` dir OR a dir containing it).
//! 2. `%APPDATA%/.minecraft/versions/26.1.2/26.1.2/assets/minecraft/`
//!    (well-known MC install layout).
//! 3. `%APPDATA%/.minecraft/versions/26.1.2/26.1.2.jar` (extracted
//!    lazily on first load into `<data>/cache/mc_assets/`).
//! 4. User-dropped `resourcepacks/<name>/` directory or `.zip` under
//!    the InfEngine data dir.
//! 5. Bundled placeholder atlas - the engine boots with synthetic
//!    swatches so the UI is usable even without MC installed.
//!
//! Files under this folder:
//! - `paths.rs`     - 4-tier discovery + APPDATA expansion.
//! - `jar.rs`       - .jar / .zip extractor (deferred to W9.2+).
//! - `textures.rs`  - PNG decode + atlas packing (W9.2).
//! - `mcmeta.rs`    - `.mcmeta` animation metadata parser (W9.2).
//! - `models.rs`    - block-model JSON parser (W9.2+).
//! - `sounds.rs`    - `sounds.json` + per-event OGG resolver (W9.6).
//! - `name_map.rs`  - infengine block id <-> MC asset name table.
//! - `fallback.rs`  - placeholder atlas used when discovery fails.

use std::collections::HashMap;
use std::path::PathBuf;

pub(crate) mod anim_registry;
pub(crate) mod blockstates;
pub(crate) mod element_mesh;
pub(crate) mod fallback;
pub(crate) mod mcmeta;
pub(crate) mod models;
pub(crate) mod name_map;
pub(crate) mod particles;
pub(crate) mod paths;
pub(crate) mod sounds;
pub(crate) mod special_models;
pub(crate) mod textures;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod models_tests;

/// A single texture entry inside the runtime atlas - either a static
/// 16x16 tile or the head of an animation strip whose frames live
/// sequentially below `atlas_uv`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct TextureRef {
    /// (u0, v0, u1, v1) of frame 0 in the runtime atlas.
    pub(crate) atlas_uv: [f32; 4],
    /// 1 = static, > 1 = animated frame strip.
    pub(crate) frames: u32,
    /// Milliseconds per frame. Static tiles report 0.
    pub(crate) frametime_ms: u32,
    /// MC `.mcmeta` interpolation flag. Renderer may ignore.
    pub(crate) interpolate: bool,
}

impl TextureRef {
    pub(crate) fn placeholder(uv: [f32; 4]) -> Self {
        Self {
            atlas_uv: uv,
            frames: 1,
            frametime_ms: 0,
            interpolate: false,
        }
    }

    /// Compute the UV rect for the active animation frame at the
    /// given `time_ms` clock. Static tiles return `atlas_uv` as-is.
    ///
    /// Frame stride: animated tiles place each frame sequentially
    /// in the atlas with frame 0 at `atlas_uv`. The vertical extent
    /// of one frame is `atlas_uv[3] - atlas_uv[1]`; advancing one
    /// frame shifts `v0` + `v1` by that amount.
    pub(crate) fn uv_at(&self, time_ms: u32) -> [f32; 4] {
        if self.frames <= 1 || self.frametime_ms == 0 {
            return self.atlas_uv;
        }
        let frame = (time_ms / self.frametime_ms) % self.frames;
        let frame_h = self.atlas_uv[3] - self.atlas_uv[1];
        let v0 = self.atlas_uv[1] + frame as f32 * frame_h;
        let v1 = v0 + frame_h;
        [self.atlas_uv[0], v0, self.atlas_uv[2], v1]
    }

    pub(crate) fn is_animated(&self) -> bool {
        self.frames > 1 && self.frametime_ms > 0
    }
}

/// Atlas-relative texture key: which sub-folder under
/// `assets/minecraft/textures/` the texture comes from, plus the
/// bare file stem. Keeps block + item + particle keys disjoint.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum TextureKey {
    Block(String),
    Item(String),
    Particle(String),
}

/// Loaded resource pack. Built once at startup + on the settings
/// "Reload" action. When `is_fallback` is true the runtime falls
/// back to the synthetic placeholder atlas and no MC assets were
/// actually loaded - downstream consumers should keep their
/// existing tinted-swatch fallbacks alive.
#[derive(Debug)]
pub(crate) struct ResourcePack {
    /// Block / item / particle texture entries keyed by source path.
    pub(crate) textures: HashMap<TextureKey, TextureRef>,
    /// Item-icon entries keyed by MC item name ("stone", "iron_ingot",
    /// "bucket_water", ...). Block items reuse the block's top face
    /// texture; non-block items resolve their own entry here.
    pub(crate) item_icons: HashMap<String, TextureRef>,
    /// Sound event registry: `"block.stone.break" -> Vec<PathBuf>`
    /// of OGG variants. Empty when no pack loaded or `sounds.json`
    /// missing.
    pub(crate) sounds: HashMap<String, Vec<PathBuf>>,
    /// Path the pack loaded from (informational; shown in the
    /// settings page).
    pub(crate) source: PathBuf,
    /// True when the loader could not find a real MC install and
    /// dropped back onto the synthetic placeholder atlas.
    pub(crate) is_fallback: bool,
    /// Atlas pixel dimensions (width, height). Always a power of
    /// two; min 256, grows in powers of two up to 2048.
    pub(crate) atlas_size: (u32, u32),
    /// Atlas RGBA pixels (4 bytes per pixel, row-major).
    pub(crate) atlas_rgba: Vec<u8>,
    /// Wave 9.y: animation registry. Built from every animated
    /// `TextureRef` after the atlas lands - holds up to 32 slots
    /// keyed by `(v0, v_step, frame_count, frametime_ms)`. The
    /// renderer's `TileAnimsUniform` is filled from this; the
    /// chunk mesher looks up each tile's slot via
    /// `anim_slot_for_texture` and writes it into the vertex's
    /// `anim_id` field.
    pub(crate) anim_registry: anim_registry::AnimRegistry,
    /// Wave 9.y: reverse lookup from a texture entry to its
    /// animation slot id. Static tiles aren't in the table; the
    /// helper returns `STATIC_SLOT` (0) for those. Keyed by the
    /// same `TextureKey` shape as `textures`.
    pub(crate) anim_slots: HashMap<TextureKey, u8>,
}

impl ResourcePack {
    /// Run the path discovery + build a `ResourcePack`. Never fails:
    /// when no MC install is found, returns the fallback pack with
    /// `is_fallback = true` and an empty textures map - callers
    /// already have synthetic-swatch paths to fall back on.
    pub(crate) fn load() -> Self {
        Self::load_with_override(None)
    }

    /// Same as `load`, but with an explicit override path (used by
    /// the settings page reload button when the user pastes a path).
    /// The override takes priority over the env-var + well-known
    /// locations.
    pub(crate) fn load_with_override(override_path: Option<PathBuf>) -> Self {
        let discovered = override_path
            .and_then(paths::canonicalize_assets_root)
            .or_else(paths::discover_assets_root);
        match discovered {
            Some(root) => {
                tracing::info!(path = %root.display(), "MC resource pack discovered");
                let sounds_reg = sounds::load_sounds_json(&root);
                if !sounds_reg.is_empty() {
                    tracing::info!(
                        events = sounds_reg.len(),
                        "MC sounds.json loaded",
                    );
                }
                match textures::load_atlas(&root) {
                    Some(atlas) => {
                        tracing::info!(
                            size = atlas.atlas_size.0,
                            count = atlas.textures.len(),
                            "MC resource pack atlas built",
                        );
                        let (reg, slots) = build_anim_index(&atlas.textures);
                        if !reg.is_empty() {
                            tracing::info!(
                                slots = reg.len(),
                                "MC animated tiles registered",
                            );
                        }
                        ResourcePack {
                            textures: atlas.textures,
                            item_icons: atlas.item_icons,
                            sounds: sounds_reg,
                            source: root,
                            is_fallback: false,
                            atlas_size: atlas.atlas_size,
                            atlas_rgba: atlas.atlas_rgba,
                            anim_registry: reg,
                            anim_slots: slots,
                        }
                    }
                    None => {
                        tracing::warn!(
                            path = %root.display(),
                            "MC resource pack contains no textures - falling back",
                        );
                        let mut pack = fallback::placeholder_pack();
                        pack.source = root;
                        pack.sounds = sounds_reg;
                        pack
                    }
                }
            }
            None => {
                tracing::info!("no MC resource pack found - using placeholder atlas");
                fallback::placeholder_pack()
            }
        }
    }

    /// True when the pack carries no real MC asset data.
    pub(crate) fn is_placeholder(&self) -> bool {
        self.is_fallback
    }

    /// Look up a block face texture by MC name. Returns `None` when
    /// the pack is the placeholder or the name isn't registered.
    pub(crate) fn block_texture(&self, name: &str) -> Option<&TextureRef> {
        self.textures.get(&TextureKey::Block(name.to_string()))
    }

    /// Look up an item icon by MC item name. Block items should
    /// reuse the block's top-face texture; this helper covers
    /// non-block items (tools, buckets, ingots, food, ...).
    pub(crate) fn item_icon(&self, name: &str) -> Option<&TextureRef> {
        self.item_icons.get(name)
    }

    /// Resolve an engine sound key (`"step:grass"`, `"lava:pop"`,
    /// ...) into a list of OGG file paths. Walks the engine -> MC
    /// translation table + the registry; returns an empty slice
    /// when the key is unknown or no variants resolved.
    pub(crate) fn sound_paths(&self, engine_key: &str) -> &[PathBuf] {
        for (k, mc_name) in sounds::ENGINE_TO_MC {
            if *k == engine_key
                && let Some(v) = self.sounds.get(*mc_name)
            {
                return v.as_slice();
            }
        }
        // Direct MC-name lookup as a fallback so callers can
        // pass the MC event name explicitly.
        if let Some(v) = self.sounds.get(engine_key) {
            return v.as_slice();
        }
        &[]
    }

    /// Resolve a block face (top / bottom / sides) for an infengine
    /// block id to a `TextureRef` in this pack. Falls through the
    /// per-face override -> shared block name -> `None` (caller
    /// keeps its placeholder atlas tile alive).
    pub(crate) fn block_face_ref(
        &self,
        infengine_id: u16,
        face: BlockFace,
    ) -> Option<&TextureRef> {
        let map = name_map::lookup_block(infengine_id)?;
        let face_name = match face {
            BlockFace::Top => map.top,
            BlockFace::Bottom => map.bottom,
            BlockFace::Sides => map.sides,
        };
        let primary = if !face_name.is_empty() {
            self.block_texture(face_name)
        } else {
            None
        };
        primary.or_else(|| self.block_texture(map.block))
    }

    /// Resolve the inventory icon for an infengine item id. Block-form
    /// items return the block's top-face entry (matches MC inventory
    /// behaviour). Tools / buckets / ingots / food resolve through
    /// the item-name override table.
    pub(crate) fn icon_ref(&self, infengine_id: u16) -> Option<&TextureRef> {
        // Tool / ingredient / bucket / food: dedicated item icon.
        if let Some(item_name) = name_map::lookup_item(infengine_id)
            && let Some(r) = self.item_icon(item_name)
        {
            return Some(r);
        }
        // Block form: use the top face.
        self.block_face_ref(infengine_id, BlockFace::Top)
    }
}

/// Which face of a cube the renderer asks the pack to resolve.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BlockFace {
    Top,
    Bottom,
    Sides,
}

impl ResourcePack {
    /// Wave 9.y: resolve a texture entry to its animation slot
    /// id. Static tiles + entries not in the pack return
    /// `STATIC_SLOT` (0). Animated tiles return a 1-based slot
    /// the renderer's `tile_anim` uniform array reads.
    pub(crate) fn anim_slot(&self, key: &TextureKey) -> u8 {
        self.anim_slots.get(key).copied().unwrap_or(0)
    }

    /// Wave 9.y: convenience for the chunk mesher - given a
    /// `TextureRef` (already resolved into the atlas), search the
    /// reverse map for its slot. Returns 0 for static tiles.
    /// Used when the mesher emits a face with a `TextureRef` it
    /// has on hand but no `TextureKey`.
    pub(crate) fn anim_slot_for_ref(&self, tex: &TextureRef) -> u8 {
        if !tex.is_animated() {
            return 0;
        }
        // Walk the textures map to find a matching entry. This is
        // cheap because the atlas has O(hundreds) entries and the
        // mesher caches the slot per-face downstream.
        for (k, v) in &self.textures {
            if v.atlas_uv == tex.atlas_uv
                && v.frames == tex.frames
                && v.frametime_ms == tex.frametime_ms
            {
                return self.anim_slot(k);
            }
        }
        0
    }

    /// Wave 9.y: snapshot the registry into the renderer's
    /// uniform body. Called on pack load + reload so the GPU
    /// table mirrors `self.anim_registry`.
    pub(crate) fn build_tile_anims_uniform(
        &self,
    ) -> infengine_render::TileAnimsUniform {
        let entries: Vec<infengine_render::TileAnimEntry> = self
            .anim_registry
            .entries()
            .iter()
            .map(|s| infengine_render::TileAnimEntry {
                frame_count: s.frame_count,
                frametime_ms: s.frametime_ms,
                v_step: s.v_step,
                _pad: 0.0,
            })
            .collect();
        infengine_render::TileAnimsUniform::from_sequential(&entries)
    }
}

/// Wave 9.y: walk every entry in `textures` and register the
/// animated ones (frames > 1, frametime_ms > 0) into a fresh
/// `AnimRegistry`. Returns the registry + a reverse map keyed by
/// `TextureKey` so the chunk mesher can resolve a tile to its
/// slot id without re-scanning the textures map per face.
fn build_anim_index(
    textures: &HashMap<TextureKey, TextureRef>,
) -> (anim_registry::AnimRegistry, HashMap<TextureKey, u8>) {
    let mut reg = anim_registry::AnimRegistry::new();
    let mut slots: HashMap<TextureKey, u8> = HashMap::new();
    // Iterate in deterministic order: collect keys, sort, then
    // register. This keeps the slot ids stable across loads so
    // a reload assigns the same slot to the same animated tile
    // (water_still always -> slot 1 once the loader sees it
    // first).
    let mut keys: Vec<&TextureKey> = textures.keys().collect();
    keys.sort_by(|a, b| format!("{a:?}").cmp(&format!("{b:?}")));
    for key in keys {
        let Some(tex) = textures.get(key) else {
            continue;
        };
        if !tex.is_animated() {
            continue;
        }
        let slot = reg.register(tex);
        if slot != 0 {
            slots.insert(key.clone(), slot);
        }
    }
    (reg, slots)
}
