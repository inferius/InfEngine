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

pub(crate) mod fallback;
pub(crate) mod mcmeta;
pub(crate) mod name_map;
pub(crate) mod paths;
pub(crate) mod textures;

#[cfg(test)]
mod tests;

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
                match textures::load_atlas(&root) {
                    Some(atlas) => {
                        tracing::info!(
                            size = atlas.atlas_size.0,
                            count = atlas.textures.len(),
                            "MC resource pack atlas built",
                        );
                        ResourcePack {
                            textures: atlas.textures,
                            item_icons: atlas.item_icons,
                            sounds: HashMap::new(),
                            source: root,
                            is_fallback: false,
                            atlas_size: atlas.atlas_size,
                            atlas_rgba: atlas.atlas_rgba,
                        }
                    }
                    None => {
                        tracing::warn!(
                            path = %root.display(),
                            "MC resource pack contains no textures - falling back",
                        );
                        let mut pack = fallback::placeholder_pack();
                        pack.source = root;
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
