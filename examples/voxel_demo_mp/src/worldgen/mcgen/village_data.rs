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
}

/// One weighted pool entry, decoded to the solver's vocabulary.
#[derive(Debug, Clone)]
pub(crate) struct PoolElement {
    pub(crate) weight: u32,
    pub(crate) kind: ElementKind,
}

#[derive(Debug, Clone)]
pub(crate) enum ElementKind {
    /// `legacy_single_pool_element` - one template, either anchored
    /// rigidly to its joint or dropped onto the terrain heightmap.
    Single { location: String, terrain_matching: bool },
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
        .map(|w| {
            let kind = match w.element.element_type.as_str() {
                "minecraft:legacy_single_pool_element"
                | "minecraft:single_pool_element" => ElementKind::Single {
                    location: w.element.location.unwrap_or_default(),
                    terrain_matching: w.element.projection.as_deref()
                        == Some("terrain_matching"),
                },
                "minecraft:feature_pool_element" => ElementKind::Feature {
                    feature: w.element.feature.unwrap_or_default(),
                },
                _ => ElementKind::Empty,
            };
            PoolElement { weight: w.weight, kind }
        })
        .collect();
    Some(TemplatePool { elements, fallback: json.fallback })
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
        let mut checked_templates = 0usize;
        for id in &pool_ids {
            let p = pool(id).unwrap_or_else(|| panic!("pool {id} must parse"));
            for e in &p.elements {
                if let ElementKind::Single { location, .. } = &e.kind {
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
