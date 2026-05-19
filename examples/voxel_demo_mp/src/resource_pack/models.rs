//! MC block-model JSON parser with parent-chain resolution.
//!
//! MC ships per-block geometry in `assets/minecraft/models/block/*.json`.
//! Each file MAY declare `parent`, `textures`, and `elements`. The
//! engine that loads them walks the parent chain accumulating both
//! `textures` (later files override earlier ones) and the deepest
//! non-empty `elements` array.
//!
//! Example chain for `block/oak_log`:
//! - `oak_log.json` -> parent = `block/cube_column`,
//!   textures = { end -> oak_log_top, side -> oak_log }
//! - `cube_column.json` -> parent = `block/cube`,
//!   textures = { down -> #end, up -> #end, north -> #side, ... }
//! - `cube.json` -> parent = `block/block`,
//!   elements = [single cube with 6 faces sampling #down/#up/...]
//! - `block.json` -> no parent, no elements (just display transforms).
//!
//! Texture references take two forms:
//! - `"block/dirt"` - an absolute MC namespace path (we strip the
//!   `minecraft:` prefix + the `block/` folder hint to get the file
//!   stem the W9.2 atlas already keys by).
//! - `"#var"` - a variable resolved through the merged textures map;
//!   the chain MUST eventually resolve every `#var` to an absolute
//!   path or the face is dropped.
//!
//! Cuboid geometry uses MC's "16ths of a block" convention. `from =
//! [0, 0, 0]` + `to = [16, 16, 16]` is a full cube. A slab bottom is
//! `to = [16, 8, 16]`. A torch stick is `[7, 0, 7] -> [9, 10, 9]`.
//!
//! Reference: decompiled MC `26.1.2-sources` ships the loader at
//! `net/minecraft/client/resources/model/BlockModel.java` (the
//! Mojang implementation). Our parser is a much-simplified port -
//! we keep just enough to render the engine's block roster, not
//! the full Mojang grammar (no `gui_light`, no `display`
//! transforms, no `ambientocclusion` per-face override).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::ResourcePack;

/// One cuboid element inside a block model. `from` + `to` are in
/// MC's 16ths-of-a-block units; `[0, 0, 0]` to `[16, 16, 16]` is a
/// full unit-sized voxel cube.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ModelElement {
    pub(crate) from: [f32; 3],
    pub(crate) to: [f32; 3],
    pub(crate) rotation: Option<ElementRotation>,
    pub(crate) faces: ElementFaces,
    pub(crate) shade: bool,
}

/// Optional rotation around `origin` on a single axis. MC only
/// supports `{-45, -22.5, 0, 22.5, 45}` degrees on a SINGLE axis -
/// we accept any float in case a pack diverges.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ElementRotation {
    pub(crate) origin: [f32; 3],
    pub(crate) axis: RotationAxis,
    pub(crate) angle_deg: f32,
    pub(crate) rescale: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RotationAxis {
    X,
    Y,
    Z,
}

/// Six possible cube faces. `None` means the JSON did not declare
/// the face at all -> the renderer should not emit it.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct ElementFaces {
    pub(crate) down: Option<ModelFace>,
    pub(crate) up: Option<ModelFace>,
    pub(crate) north: Option<ModelFace>,
    pub(crate) south: Option<ModelFace>,
    pub(crate) west: Option<ModelFace>,
    pub(crate) east: Option<ModelFace>,
}

/// Per-face data: UV rect in 16ths, the resolved texture file stem
/// (or `"#unresolved"` if a `#var` could not be resolved), an
/// optional cullface (which neighbour face culls this one if
/// solid), an optional tint index (biome tint hook), and a UV
/// rotation in degrees (0 / 90 / 180 / 270).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ModelFace {
    pub(crate) uv: [f32; 4],
    pub(crate) texture: String,
    pub(crate) cullface: Option<CubeFace>,
    pub(crate) tintindex: Option<i32>,
    pub(crate) rotation: u16,
}

/// Cardinal cube face. Mirrors MC's `"down"/"up"/"north"/.../"east"`
/// face names + a few cullface mnemonics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CubeFace {
    Down,
    Up,
    North,
    South,
    West,
    East,
}

impl CubeFace {
    pub(super) fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "down" | "bottom" => Self::Down,
            "up" | "top" => Self::Up,
            "north" => Self::North,
            "south" => Self::South,
            "west" => Self::West,
            "east" => Self::East,
            _ => return None,
        })
    }

    /// Test helper exposing `parse` to the sibling test file.
    #[cfg(test)]
    pub(super) fn parse_pub(s: &str) -> Option<Self> {
        Self::parse(s)
    }
}

/// A fully-resolved block model: every `#var` has been replaced
/// with the final texture file stem (or dropped). The renderer can
/// consume this without re-walking parents.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BlockModel {
    pub(crate) elements: Vec<ModelElement>,
    pub(crate) textures: HashMap<String, String>,
    pub(crate) ambient_occlusion: bool,
}

impl BlockModel {
    /// Convenience constructor used in tests + the W9.x.3 hand-built
    /// special model registry.
    pub(crate) fn new(elements: Vec<ModelElement>) -> Self {
        Self {
            elements,
            textures: HashMap::new(),
            ambient_occlusion: true,
        }
    }
}

/// Walk the parent chain for `name` (e.g. `"block/oak_log"`) and
/// return the fully-resolved `BlockModel`, or `None` when the head
/// model file can't be found / parsed. The function loads JSON
/// files via `pack.source` + `models/<name>.json`.
///
/// Resolution rules (matches MC):
/// 1. Recursively walk `parent` BEFORE evaluating the current
///    model so deeper parents apply first + the head model wins.
/// 2. `textures` from the child override (and add to) the parent
///    map.
/// 3. The deepest non-empty `elements` array wins - a child that
///    redeclares `elements` REPLACES the inherited list; a child
///    that omits `elements` inherits from the parent.
/// 4. After accumulation, every `#var` reference in face textures
///    is resolved through the merged textures map. Chains that
///    leave a variable unresolved drop that face on the floor.
pub(crate) fn load_model(pack: &ResourcePack, name: &str) -> Option<BlockModel> {
    // Even on the placeholder pack the loader may have a valid
    // `source` PathBuf for the "discovered MC root, no textures"
    // path. The model JSON files live under that root.
    load_model_at(&pack.source, name)
}

/// Same as `load_model` but takes an explicit MC `assets/minecraft/`
/// root. Exposed for tests that stage a synthetic models tree
/// without building a full `ResourcePack`.
pub(crate) fn load_model_at(root: &Path, name: &str) -> Option<BlockModel> {
    let mut accumulated = AccumulatedModel::default();
    walk_chain(root, name, &mut accumulated, 0)?;
    Some(accumulated.finalize())
}

/// In-progress accumulator while walking the parent chain. Each
/// `walk_chain` recursion mutates this; the leaf-most call returns
/// the final `BlockModel` via `finalize`.
#[derive(Debug, Default)]
struct AccumulatedModel {
    elements: Vec<ModelElement>,
    textures: HashMap<String, String>,
    ambient_occlusion: bool,
    has_elements: bool,
}

impl AccumulatedModel {
    fn finalize(mut self) -> BlockModel {
        // Resolve `#var` references on every face's texture field
        // through the merged textures map. Variables that bottom
        // out unresolved keep the literal `"#var"` so the renderer
        // can fall back to the placeholder atlas tile via a name
        // lookup miss.
        for el in &mut self.elements {
            for face in [
                el.faces.down.as_mut(),
                el.faces.up.as_mut(),
                el.faces.north.as_mut(),
                el.faces.south.as_mut(),
                el.faces.west.as_mut(),
                el.faces.east.as_mut(),
            ]
            .into_iter()
            .flatten()
            {
                face.texture = resolve_texture(&face.texture, &self.textures);
            }
        }
        // Normalise the textures map too so callers that look up a
        // variable directly (e.g. `"particle"`) get the final stem.
        let mut resolved = HashMap::with_capacity(self.textures.len());
        for (k, v) in &self.textures {
            resolved.insert(k.clone(), resolve_texture(v, &self.textures));
        }
        self.textures = resolved;
        BlockModel {
            elements: self.elements,
            textures: self.textures,
            ambient_occlusion: self.ambient_occlusion,
        }
    }
}

/// Recursive parent-walk. Returns `Some(())` when the head model
/// file at `name` parsed correctly + everything below it was at
/// least readable; the leaf failures are non-fatal.
fn walk_chain(
    root: &Path,
    name: &str,
    acc: &mut AccumulatedModel,
    depth: usize,
) -> Option<()> {
    // Guard against pathological cycles + deeply-nested parents.
    // MC's stock chain depth is 3-4; 16 is comfortably above.
    if depth > 16 {
        return None;
    }
    let path = model_path(root, name);
    let text = std::fs::read_to_string(&path).ok()?;
    let v: Value = serde_json::from_str(&text).ok()?;
    // Recurse into parent FIRST so child overrides land last.
    if let Some(parent) = v.get("parent").and_then(|p| p.as_str()) {
        let parent_norm = strip_namespace(parent);
        // Best-effort: a missing parent is non-fatal so packs with
        // partial coverage still resolve what they can.
        let _ = walk_chain(root, &parent_norm, acc, depth + 1);
    }
    // Apply this layer's textures + elements.
    if let Some(tex) = v.get("textures").and_then(|t| t.as_object()) {
        for (k, val) in tex {
            if let Some(s) = val.as_str() {
                acc.textures.insert(k.clone(), strip_namespace(s));
            }
        }
    }
    if let Some(ao) = v.get("ambientocclusion").and_then(|a| a.as_bool()) {
        acc.ambient_occlusion = ao;
    } else if depth == 0 {
        acc.ambient_occlusion = true;
    }
    if let Some(elems) = v.get("elements").and_then(|e| e.as_array()) {
        // A child that DECLARES elements completely replaces the
        // parent's list (matches MC's behaviour); keep ours instead
        // of appending.
        acc.elements = elems
            .iter()
            .filter_map(parse_element)
            .collect();
        acc.has_elements = true;
    }
    Some(())
}

/// Convert a model name into a file path under the MC pack.
fn model_path(root: &Path, name: &str) -> PathBuf {
    root.join("models").join(format!("{name}.json"))
}

/// Strip the optional `minecraft:` namespace + a leading `block/`
/// folder hint so the result keys into the W9.2 texture atlas by
/// bare file stem (e.g. `"oak_log_top"`).
///
/// Model NAMES preserve the folder hint (so we can build the JSON
/// path correctly); TEXTURE references collapse to the file stem
/// because that's the W9.2 atlas key shape.
pub(crate) fn strip_namespace(s: &str) -> String {
    let s = s.strip_prefix("minecraft:").unwrap_or(s);
    s.to_string()
}

/// Look up a texture reference in the merged map. References of the
/// form `"#var"` chase through the map (up to a small depth cap to
/// avoid cycles) until they bottom out at an absolute path. Absolute
/// references (`"block/oak_log"`) collapse to the file stem
/// (`"oak_log"`) so callers can index the W9.2 atlas by stem.
fn resolve_texture(initial: &str, textures: &HashMap<String, String>) -> String {
    let mut cur = initial.to_string();
    for _ in 0..16 {
        if let Some(var) = cur.strip_prefix('#') {
            match textures.get(var) {
                Some(next) => cur = next.clone(),
                None => return cur, // unresolved -> leave as #var
            }
        } else {
            // Absolute reference: strip `block/`, `item/`, etc. to
            // get the bare file stem the W9.2 atlas keys by.
            return cur
                .rsplit_once('/')
                .map(|(_, stem)| stem.to_string())
                .unwrap_or(cur);
        }
    }
    cur
}

/// Parse one element JSON object into a `ModelElement`. Returns
/// `None` for malformed inputs.
fn parse_element(v: &Value) -> Option<ModelElement> {
    let from = parse_vec3(v.get("from")?)?;
    let to = parse_vec3(v.get("to")?)?;
    let shade = v.get("shade").and_then(|s| s.as_bool()).unwrap_or(true);
    let rotation = v.get("rotation").and_then(parse_rotation);
    let faces = v
        .get("faces")
        .and_then(|f| f.as_object())
        .map(parse_faces)
        .unwrap_or_default();
    Some(ModelElement {
        from,
        to,
        rotation,
        faces,
        shade,
    })
}

fn parse_rotation(v: &Value) -> Option<ElementRotation> {
    let origin = parse_vec3(v.get("origin")?)?;
    let axis = match v.get("axis").and_then(|a| a.as_str())? {
        "x" => RotationAxis::X,
        "y" => RotationAxis::Y,
        "z" => RotationAxis::Z,
        _ => return None,
    };
    let angle_deg = v.get("angle").and_then(|a| a.as_f64()).unwrap_or(0.0) as f32;
    let rescale = v
        .get("rescale")
        .and_then(|r| r.as_bool())
        .unwrap_or(false);
    Some(ElementRotation {
        origin,
        axis,
        angle_deg,
        rescale,
    })
}

fn parse_faces(obj: &serde_json::Map<String, Value>) -> ElementFaces {
    let mut out = ElementFaces::default();
    for (k, v) in obj {
        let face = parse_one_face(v);
        match CubeFace::parse(k) {
            Some(CubeFace::Down) => out.down = face,
            Some(CubeFace::Up) => out.up = face,
            Some(CubeFace::North) => out.north = face,
            Some(CubeFace::South) => out.south = face,
            Some(CubeFace::West) => out.west = face,
            Some(CubeFace::East) => out.east = face,
            None => {}
        }
    }
    out
}

fn parse_one_face(v: &Value) -> Option<ModelFace> {
    // `uv` is optional - MC defaults to projecting `from`/`to` onto
    // the face. We store `[0, 0, 16, 16]` as a sentinel for "use the
    // default projection" so the renderer can plug in its own
    // computation later.
    let uv = v
        .get("uv")
        .and_then(|u| u.as_array())
        .and_then(|a| {
            if a.len() == 4 {
                Some([
                    a[0].as_f64().unwrap_or(0.0) as f32,
                    a[1].as_f64().unwrap_or(0.0) as f32,
                    a[2].as_f64().unwrap_or(16.0) as f32,
                    a[3].as_f64().unwrap_or(16.0) as f32,
                ])
            } else {
                None
            }
        })
        .unwrap_or([0.0, 0.0, 16.0, 16.0]);
    let texture = v.get("texture").and_then(|t| t.as_str())?.to_string();
    let cullface = v
        .get("cullface")
        .and_then(|c| c.as_str())
        .and_then(CubeFace::parse);
    let tintindex = v.get("tintindex").and_then(|t| t.as_i64()).map(|n| n as i32);
    let rotation = v
        .get("rotation")
        .and_then(|r| r.as_u64())
        .map(|n| n as u16)
        .unwrap_or(0);
    Some(ModelFace {
        uv,
        texture,
        cullface,
        tintindex,
        rotation,
    })
}

fn parse_vec3(v: &Value) -> Option<[f32; 3]> {
    let a = v.as_array()?;
    if a.len() != 3 {
        return None;
    }
    Some([
        a[0].as_f64()? as f32,
        a[1].as_f64()? as f32,
        a[2].as_f64()? as f32,
    ])
}

