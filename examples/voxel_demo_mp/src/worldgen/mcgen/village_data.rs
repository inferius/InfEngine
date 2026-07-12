//! W245.2 - the village jigsaw DATA: template pools + structure
//! templates, shipped as one embedded zip.
//!
//! `data/village_data.zip` packs the 26.1.2 data-pack subtree the
//! village system needs (63 template-pool JSONs incl. the `empty`
//! fallback, 483 structure `.nbt` templates, 40 processor lists for
//! the future mossify/zombie pass) - ~780 KB, regenerated from
//! `F:\Data\mc_assets` by `scripts` when the MC version bumps. One
//! archive keeps the include_bytes table at a single entry and the
//! runtime hermetic (no external data directory needed), matching
//! the include_str convention the rest of `mcgen/data` uses.
//!
//! `pool()` / `template()` hand out `Rc`s from per-thread caches -
//! the jigsaw solver re-reads the same handful of pools per village.

use std::cell::RefCell;
use std::collections::HashMap;
use std::io::Read;
use std::rc::Rc;

use serde::Deserialize;

use super::template::StructureTemplate;

static VILLAGE_ZIP: &[u8] = include_bytes!("data/village_data.zip");

/// Read one entry out of the embedded archive.
fn zip_entry(path: &str) -> Option<Vec<u8>> {
    let cursor = std::io::Cursor::new(VILLAGE_ZIP);
    let mut ar = zip::ZipArchive::new(cursor).ok()?;
    let mut f = ar.by_name(path).ok()?;
    let mut out = Vec::with_capacity(f.size() as usize);
    f.read_to_end(&mut out).ok()?;
    Some(out)
}

/// Strip the `minecraft:` namespace (the only one vanilla data uses).
fn strip_ns(id: &str) -> &str {
    id.strip_prefix("minecraft:").unwrap_or(id)
}

// ----------------------------------------------------------------
// Template pools (`worldgen/template_pool/**.json`).
// ----------------------------------------------------------------

#[derive(Deserialize)]
struct PoolJson {
    elements: Vec<WeightedElementJson>,
    fallback: String,
}

#[derive(Deserialize)]
struct WeightedElementJson {
    weight: u32,
    element: ElementJson,
}

#[derive(Deserialize)]
struct ElementJson {
    element_type: String,
    #[serde(default)]
    location: Option<String>,
    #[serde(default)]
    projection: Option<String>,
    #[serde(default)]
    feature: Option<String>,
    /// `list_pool_element` inner elements (W251 outpost towers).
    #[serde(default)]
    elements: Option<Vec<ElementJson>>,
    /// Either an inline `{processors: []}` object or a processor-list
    /// REFERENCE string like `"minecraft:outpost_rot"`.
    #[serde(default)]
    processors: Option<serde_json::Value>,
}

/// One weighted pool entry, decoded to the solver's vocabulary.
#[derive(Debug, Clone)]
pub(crate) struct PoolElement {
    pub(crate) weight: u32,
    pub(crate) kind: ElementKind,
}

/// W251: one stacked element of a `list_pool_element` beyond the
/// first - carved over the base template with a per-cell keep roll.
#[derive(Debug, Clone)]
pub(crate) struct ListOverlay {
    pub(crate) location: String,
    /// `block_rot` keep-probability from the element's referenced
    /// processor list (1.0 = every cell writes).
    pub(crate) integrity: f32,
}

#[derive(Debug, Clone)]
pub(crate) enum ElementKind {
    /// `legacy_single_pool_element` - one template, either anchored
    /// rigidly to its joint or dropped onto the terrain heightmap.
    /// `processors` is the element's processor-list REFERENCE (W261:
    /// the ancient-city degradation rules run per cell at carve).
    Single {
        location: String,
        terrain_matching: bool,
        processors: Option<String>,
    },
    /// `list_pool_element` - templates stacked at one anchor. The
    /// FIRST defines the box + jigsaws (`ListPoolElement` delegates
    /// both to element 0); the rest overlay at carve time (the
    /// outpost's overgrown-watchtower rot pattern).
    List {
        first: String,
        overlays: Vec<ListOverlay>,
        terrain_matching: bool,
        processors: Option<String>,
    },
    /// `feature_pool_element` - a placed feature (village trees /
    /// pile decorations); carried through for a later wave.
    Feature { feature: String },
    /// `empty_pool_element` - a real no-op entry.
    Empty,
}

#[derive(Debug)]
pub(crate) struct TemplatePool {
    pub(crate) elements: Vec<PoolElement>,
    pub(crate) fallback: String,
}

impl TemplatePool {
    pub(crate) fn total_weight(&self) -> u32 {
        self.elements.iter().map(|e| e.weight).sum()
    }
}

/// Load + cache a template pool by its data id (e.g.
/// `minecraft:village/plains/town_centers`).
pub(crate) fn pool(id: &str) -> Option<Rc<TemplatePool>> {
    thread_local! {
        static CACHE: RefCell<HashMap<String, Option<Rc<TemplatePool>>>> =
            RefCell::new(HashMap::new());
    }
    if let Some(hit) = CACHE.with(|c| c.borrow().get(id).cloned()) {
        return hit;
    }
    let loaded = load_pool(id).map(Rc::new);
    if loaded.is_none() {
        tracing::warn!(pool = id, "W245: template pool missing from village_data.zip");
    }
    CACHE.with(|c| c.borrow_mut().insert(id.to_string(), loaded.clone()));
    loaded
}

fn load_pool(id: &str) -> Option<TemplatePool> {
    let path = format!("worldgen/template_pool/{}.json", strip_ns(id));
    let bytes = zip_entry(&path)?;
    let json: PoolJson = serde_json::from_slice(&bytes)
        .map_err(|e| tracing::warn!(pool = id, error = %e, "W245: pool JSON parse failed"))
        .ok()?;
    let elements = json
        .elements
        .into_iter()
        .map(|w| PoolElement { weight: w.weight, kind: decode_element(&w.element) })
        .collect();
    Some(TemplatePool { elements, fallback: json.fallback })
}

fn decode_element(e: &ElementJson) -> ElementKind {
    let terrain_matching = e.projection.as_deref() == Some("terrain_matching");
    // Processor-list REFERENCE only (inline `{processors: []}`
    // objects carry nothing the carve pass needs).
    let proc_ref = |el: &ElementJson| -> Option<String> {
        el.processors
            .as_ref()
            .and_then(|p| p.as_str())
            .map(String::from)
    };
    match e.element_type.as_str() {
        "minecraft:legacy_single_pool_element" | "minecraft:single_pool_element" => {
            ElementKind::Single {
                location: e.location.clone().unwrap_or_default(),
                terrain_matching,
                processors: proc_ref(e),
            }
        }
        "minecraft:list_pool_element" => {
            // Element 0 rules the box + jigsaws; the rest carve as
            // overlays with their processor's block_rot integrity.
            let inner: Vec<&ElementJson> = e
                .elements
                .as_deref()
                .unwrap_or_default()
                .iter()
                .filter(|i| i.location.is_some())
                .collect();
            let Some(first) = inner.first() else {
                return ElementKind::Empty;
            };
            let overlays = inner[1..]
                .iter()
                .map(|i| ListOverlay {
                    location: i.location.clone().unwrap_or_default(),
                    integrity: i
                        .processors
                        .as_ref()
                        .and_then(|p| p.as_str())
                        .and_then(block_rot_integrity)
                        .unwrap_or(1.0),
                })
                .collect();
            ElementKind::List {
                first: first.location.clone().unwrap_or_default(),
                overlays,
                terrain_matching,
                processors: proc_ref(first),
            }
        }
        "minecraft:feature_pool_element" => ElementKind::Feature {
            feature: e.feature.clone().unwrap_or_default(),
        },
        _ => ElementKind::Empty,
    }
}

/// W261: one decoded per-cell processor program - the subset the
/// carve pass executes (`StructureProcessor` list order):
/// `block_rot` first (a failed keep roll SKIPS the cell, leaving the
/// world block - the ruin holes), then the `rule` entries (first
/// matching `random_block_match` replaces the block). Rule blocks
/// resolve to engine ids up front; unresolvable names drop their
/// rule (warn-once via the blockmap's own logging path).
#[derive(Debug, Default)]
pub(crate) struct CellProcessors {
    /// `block_rot` keep probability (1.0 = keep everything).
    pub(crate) rot_integrity: f32,
    /// Engine ids the rot applies to (empty = every block).
    pub(crate) rottable: Vec<u16>,
    /// `rule` rows: (input engine id, probability, output engine id).
    pub(crate) rules: Vec<(u16, f32, u16)>,
}

/// Load + cache the carve-executable processors of a processor list.
pub(crate) fn cell_processors(id: &str) -> Rc<CellProcessors> {
    thread_local! {
        static CACHE: RefCell<HashMap<String, Rc<CellProcessors>>> =
            RefCell::new(HashMap::new());
    }
    if let Some(hit) = CACHE.with(|c| c.borrow().get(id).cloned()) {
        return hit;
    }
    let built = Rc::new(load_cell_processors(id).unwrap_or_default());
    CACHE.with(|c| c.borrow_mut().insert(id.to_string(), built.clone()));
    built
}

fn load_cell_processors(id: &str) -> Option<CellProcessors> {
    use super::features::block_id_by_name;
    let path = format!("worldgen/processor_list/{}.json", strip_ns(id));
    let bytes = zip_entry(&path)?;
    let json: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    let block_id = |name: &str| -> Option<u16> {
        if strip_ns(name) == "air" {
            return Some(0);
        }
        block_id_by_name(name)
    };
    let mut out = CellProcessors { rot_integrity: 1.0, ..Default::default() };
    for p in json.get("processors")?.as_array()? {
        match p.get("processor_type").and_then(|v| v.as_str()) {
            Some("minecraft:block_rot") => {
                out.rot_integrity = p
                    .get("integrity")
                    .and_then(|v| v.as_f64())
                    .map(|f| f as f32)
                    .unwrap_or(1.0);
                if let Some(tag) = p.get("rottable_blocks").and_then(|v| v.as_str()) {
                    // `#minecraft:<tag>` -> tags/block/<tag>.json
                    // (packed alongside the processor lists).
                    let tag_path =
                        format!("tags/block/{}.json", strip_ns(tag.trim_start_matches('#')));
                    if let Some(tb) = zip_entry(&tag_path)
                        .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
                    {
                        out.rottable = tb
                            .get("values")
                            .and_then(|v| v.as_array())
                            .map(|vals| {
                                vals.iter()
                                    .filter_map(|v| v.as_str())
                                    .filter_map(block_id)
                                    .collect()
                            })
                            .unwrap_or_default();
                    }
                }
            }
            Some("minecraft:rule") => {
                for rule in p.get("rules").and_then(|v| v.as_array()).unwrap_or(&Vec::new()) {
                    let ip = rule.get("input_predicate");
                    if ip.and_then(|v| v.get("predicate_type")).and_then(|v| v.as_str())
                        != Some("minecraft:random_block_match")
                    {
                        continue;
                    }
                    let Some(input) = ip
                        .and_then(|v| v.get("block"))
                        .and_then(|v| v.as_str())
                        .and_then(block_id)
                    else {
                        continue;
                    };
                    let prob = ip
                        .and_then(|v| v.get("probability"))
                        .and_then(|v| v.as_f64())
                        .unwrap_or(1.0) as f32;
                    let Some(output) = rule
                        .get("output_state")
                        .and_then(|v| v.get("Name"))
                        .and_then(|v| v.as_str())
                        .and_then(block_id)
                    else {
                        continue;
                    };
                    out.rules.push((input, prob, output));
                }
            }
            // protected_blocks etc.: nothing the carve consults.
            _ => {}
        }
    }
    Some(out)
}

/// The `block_rot` keep-probability of a processor list in the zip
/// (`worldgen/processor_list/<id>.json`) - `None` when the list has
/// no block_rot entry.
fn block_rot_integrity(id: &str) -> Option<f32> {
    #[derive(Deserialize)]
    struct ProcessorListJson {
        processors: Vec<serde_json::Value>,
    }
    let path = format!("worldgen/processor_list/{}.json", strip_ns(id));
    let bytes = zip_entry(&path)?;
    let json: ProcessorListJson = serde_json::from_slice(&bytes).ok()?;
    json.processors.iter().find_map(|p| {
        (p.get("processor_type").and_then(|v| v.as_str())
            == Some("minecraft:block_rot"))
        .then(|| p.get("integrity").and_then(|v| v.as_f64()).map(|f| f as f32))
        .flatten()
    })
}

// ----------------------------------------------------------------
// Structure templates (`structure/**.nbt`).
// ----------------------------------------------------------------

/// Load + cache a structure template by its data id (e.g.
/// `minecraft:village/plains/town_centers/plains_fountain_01`).
pub(crate) fn template(id: &str) -> Option<Rc<StructureTemplate>> {
    thread_local! {
        static CACHE: RefCell<HashMap<String, Option<Rc<StructureTemplate>>>> =
            RefCell::new(HashMap::new());
    }
    if let Some(hit) = CACHE.with(|c| c.borrow().get(id).cloned()) {
        return hit;
    }
    let loaded = load_template(id).map(Rc::new);
    if loaded.is_none() {
        tracing::warn!(
            template = id,
            "W245: structure template missing from village_data.zip"
        );
    }
    CACHE.with(|c| c.borrow_mut().insert(id.to_string(), loaded.clone()));
    loaded
}

/// Every structure-template data id in the archive (test + audit
/// helper - the runtime always asks by id).
pub(crate) fn all_template_ids() -> Vec<String> {
    let cursor = std::io::Cursor::new(VILLAGE_ZIP);
    let Ok(mut ar) = zip::ZipArchive::new(cursor) else {
        return Vec::new();
    };
    (0..ar.len())
        .filter_map(|i| {
            let name = ar.by_index(i).ok()?.name().to_string();
            let p = name.strip_prefix("structure/")?.strip_suffix(".nbt")?;
            Some(format!("minecraft:{p}"))
        })
        .collect()
}

fn load_template(id: &str) -> Option<StructureTemplate> {
    let path = format!("structure/{}.nbt", strip_ns(id));
    let bytes = zip_entry(&path)?;
    StructureTemplate::from_nbt_bytes(&bytes)
        .map_err(|e| {
            tracing::warn!(template = id, error = %e, "W245: template parse failed")
        })
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn town_centers_pool_parses() {
        let p = pool("minecraft:village/plains/town_centers").expect("pool loads");
        assert_eq!(p.elements.len(), 8);
        assert_eq!(p.total_weight(), 204, "4x50 + 4x1");
        assert_eq!(p.fallback, "minecraft:empty");
        assert!(p.elements.iter().all(|e| matches!(
            e.kind,
            ElementKind::Single { .. }
        )));
    }

    #[test]
    fn empty_fallback_pool_parses() {
        let p = pool("minecraft:empty").expect("the empty pool exists");
        assert!(p.elements.is_empty());
    }

    #[test]
    fn fountain_template_loads_from_the_zip() {
        let t = template("minecraft:village/plains/town_centers/plains_fountain_01")
            .expect("template loads");
        assert!(t.jigsaws().count() > 0);
    }

    /// W245.3 prep: the blockstate inventory across ALL village
    /// templates - the worklist for the blockstate -> engine-id map.
    /// `cargo test -p voxel_demo_mp --release --lib
    /// dump_village_palette -- --ignored --nocapture`
    #[test]
    #[ignore]
    #[allow(clippy::print_stdout)]
    fn dump_village_palette() {
        let cursor = std::io::Cursor::new(VILLAGE_ZIP);
        let mut ar = zip::ZipArchive::new(cursor).expect("zip opens");
        let mut names: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
        let ids: Vec<String> = (0..ar.len())
            .filter_map(|i| {
                let name = ar.by_index(i).ok()?.name().to_string();
                let p = name.strip_prefix("structure/")?.strip_suffix(".nbt")?;
                Some(format!("minecraft:{p}"))
            })
            .collect();
        for id in &ids {
            let t = template(id).expect("template parses");
            for s in &t.palette {
                *names.entry(s.name.clone()).or_insert(0) += 1;
            }
        }
        let mut sorted: Vec<(String, usize)> = names.into_iter().collect();
        sorted.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
        println!("{} templates, {} distinct block states:", ids.len(), sorted.len());
        for (name, n) in &sorted {
            let ours = crate::block::BLOCK_TYPES
                .iter()
                .any(|bt| bt.name == name.strip_prefix("minecraft:").unwrap_or(name));
            println!("{:>5}x {} {}", n, if ours { " " } else { "!" }, name);
        }
    }

    /// W261: the ancient-city degradation lists decode to the carve
    /// program - block_rot 0.95 over the replaceable tag + the three
    /// crack/snuff rules.
    #[test]
    fn ancient_city_degradation_processors_parse() {
        let p = cell_processors("minecraft:ancient_city_generic_degradation");
        assert!((p.rot_integrity - 0.95).abs() < 1e-6, "rot {}", p.rot_integrity);
        assert!(p.rottable.len() >= 8, "rottable tag ids: {}", p.rottable.len());
        assert_eq!(p.rules.len(), 3, "crack bricks/tiles + lantern-snuff rules");
        let start = cell_processors("minecraft:ancient_city_start_degradation");
        assert!((start.rot_integrity - 1.0).abs() < 1e-6, "start list has no rot");
        assert_eq!(start.rules.len(), 3);
    }

    /// Packaging integrity: every Single element of every village
    /// pool must resolve to a template inside the zip, and every
    /// pool referenced by a jigsaw socket of those templates must
    /// itself exist (villager/zombie pools included). A miss here
    /// means the zip regeneration script lost a subtree.
    #[test]
    fn every_pool_element_and_socket_resolves() {
        let cursor = std::io::Cursor::new(VILLAGE_ZIP);
        let mut ar = zip::ZipArchive::new(cursor).expect("zip opens");
        let mut pool_ids: Vec<String> = Vec::new();
        for i in 0..ar.len() {
            let name = ar.by_index(i).expect("entry").name().to_string();
            if let Some(p) = name
                .strip_prefix("worldgen/template_pool/")
                .and_then(|p| p.strip_suffix(".json"))
            {
                pool_ids.push(format!("minecraft:{p}"));
            }
        }
        assert!(pool_ids.len() > 60, "village pools packaged: {}", pool_ids.len());
        // Vanilla data bug: this pool entry references an NBT that
        // 26.1.2 does not ship (MC logs an error + skips it at
        // runtime, exactly like our solver's template miss path).
        const KNOWN_MISSING_TEMPLATES: &[&str] =
            &["minecraft:ancient_city/walls/intact_horizontal_wall_stairs_5"];
        let mut checked_templates = 0usize;
        for id in &pool_ids {
            let p = pool(id).unwrap_or_else(|| panic!("pool {id} must parse"));
            for e in &p.elements {
                if let ElementKind::Single { location, .. } = &e.kind {
                    if KNOWN_MISSING_TEMPLATES.contains(&location.as_str()) {
                        continue;
                    }
                    let t = template(location)
                        .unwrap_or_else(|| panic!("{id}: template {location} missing"));
                    checked_templates += 1;
                    for (_c, j) in t.jigsaws() {
                        if j.pool != "minecraft:empty" {
                            assert!(
                                pool(&j.pool).is_some(),
                                "{location}: socket pool {} missing",
                                j.pool
                            );
                        }
                    }
                }
            }
        }
        assert!(checked_templates > 400, "templates resolved: {checked_templates}");
    }
}
