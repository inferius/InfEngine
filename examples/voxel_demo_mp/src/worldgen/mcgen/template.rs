//! W245.1 - structure templates: the parsed form of
//! `data/minecraft/structure/**.nbt` (`StructureTemplate.java`).
//!
//! A template is a palette of block states + a list of cells indexing
//! it. Jigsaw blocks (`minecraft:jigsaw`) carry their connection
//! metadata in per-cell block-entity nbt - the W245 jigsaw solver
//! walks those to attach the next piece; `final_state` is what the
//! jigsaw cell becomes once resolved.
//!
//! This wave ships the loader + geometry only (rotations land with
//! the solver): block-state -> engine-id mapping and the placement
//! pipeline are the next W245 steps.

use super::nbt::{Nbt, parse_gzip};

/// One palette entry: a blockstate name + its property pairs, e.g.
/// `minecraft:oak_stairs` + `[(facing, north), (half, bottom)]`.
#[derive(Debug, Clone)]
pub(crate) struct PaletteState {
    pub(crate) name: String,
    pub(crate) props: Vec<(String, String)>,
}

impl PaletteState {
    pub(crate) fn prop(&self, key: &str) -> Option<&str> {
        self.props
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }
}

/// `minecraft:jigsaw` connection metadata (JigsawBlockEntity.java).
#[derive(Debug, Clone)]
pub(crate) struct JigsawInfo {
    /// This socket's name (matched by the neighbour's `target`).
    pub(crate) name: String,
    /// The socket name this jigsaw wants to attach to.
    pub(crate) target: String,
    /// Template pool the solver draws the neighbour piece from.
    pub(crate) pool: String,
    /// Block state the jigsaw cell turns into after resolution.
    pub(crate) final_state: String,
    /// "rollable" (any rotation) or "aligned" (fixed) for vertical
    /// connections.
    pub(crate) joint_rollable: bool,
    /// Selection/placement priorities (26.1.2 additions; 0 default).
    pub(crate) selection_priority: i32,
    pub(crate) placement_priority: i32,
}

/// One template cell: local position + palette index (+ jigsaw
/// metadata when the state is a jigsaw block, + the data-marker
/// string when it's a DATA-mode structure block - shipwreck loot
/// markers like `map_chest` ride these).
#[derive(Debug, Clone)]
pub(crate) struct TemplateCell {
    pub(crate) pos: [i32; 3],
    pub(crate) state: usize,
    pub(crate) jigsaw: Option<JigsawInfo>,
    pub(crate) data_marker: Option<String>,
}

/// A parsed structure template.
#[derive(Debug)]
pub(crate) struct StructureTemplate {
    pub(crate) size: [i32; 3],
    pub(crate) palette: Vec<PaletteState>,
    pub(crate) cells: Vec<TemplateCell>,
}

impl StructureTemplate {
    /// Parse a gzip `.nbt` file body.
    pub(crate) fn from_nbt_bytes(data: &[u8]) -> Result<Self, String> {
        let root = parse_gzip(data)?;
        let size = root
            .get("size")
            .and_then(Nbt::as_ivec3)
            .ok_or("template: missing size")?;
        // Vanilla writes `palette`; the random-palette variant writes
        // `palettes` (a list of palettes) - the first one is the
        // canonical shape set.
        let palette_nbt = match root.get("palette") {
            Some(p) => p,
            None => root
                .get("palettes")
                .and_then(Nbt::as_list)
                .and_then(|l| l.first())
                .ok_or("template: missing palette(s)")?,
        };
        let palette: Vec<PaletteState> = palette_nbt
            .as_list()
            .ok_or("template: palette not a list")?
            .iter()
            .map(|e| {
                let name = e
                    .get("Name")
                    .and_then(Nbt::as_str)
                    .unwrap_or("minecraft:air")
                    .to_string();
                let props = e
                    .get("Properties")
                    .and_then(Nbt::as_compound)
                    .map(|m| {
                        let mut kv: Vec<(String, String)> = m
                            .iter()
                            .filter_map(|(k, v)| {
                                v.as_str().map(|s| (k.clone(), s.to_string()))
                            })
                            .collect();
                        kv.sort();
                        kv
                    })
                    .unwrap_or_default();
                PaletteState { name, props }
            })
            .collect();
        let mut cells = Vec::new();
        for b in root
            .get("blocks")
            .and_then(Nbt::as_list)
            .ok_or("template: missing blocks")?
        {
            let pos = b
                .get("pos")
                .and_then(Nbt::as_ivec3)
                .ok_or("template: block without pos")?;
            let state = b
                .get("state")
                .and_then(Nbt::as_i32)
                .ok_or("template: block without state")? as usize;
            if state >= palette.len() {
                return Err(format!(
                    "template: state {state} out of palette ({})",
                    palette.len()
                ));
            }
            let jigsaw = if palette[state].name == "minecraft:jigsaw" {
                b.get("nbt").map(|n| JigsawInfo {
                    name: n
                        .get("name")
                        .and_then(Nbt::as_str)
                        .unwrap_or("minecraft:empty")
                        .to_string(),
                    target: n
                        .get("target")
                        .and_then(Nbt::as_str)
                        .unwrap_or("minecraft:empty")
                        .to_string(),
                    pool: n
                        .get("pool")
                        .and_then(Nbt::as_str)
                        .unwrap_or("minecraft:empty")
                        .to_string(),
                    final_state: n
                        .get("final_state")
                        .and_then(Nbt::as_str)
                        .unwrap_or("minecraft:air")
                        .to_string(),
                    joint_rollable: n
                        .get("joint")
                        .and_then(Nbt::as_str)
                        .map(|j| j == "rollable")
                        .unwrap_or(true),
                    selection_priority: n
                        .get("selection_priority")
                        .and_then(Nbt::as_i32)
                        .unwrap_or(0),
                    placement_priority: n
                        .get("placement_priority")
                        .and_then(Nbt::as_i32)
                        .unwrap_or(0),
                })
            } else {
                None
            };
            let data_marker = if palette[state].name == "minecraft:structure_block" {
                b.get("nbt").and_then(|n| {
                    (n.get("mode").and_then(Nbt::as_str) == Some("DATA"))
                        .then(|| {
                            n.get("metadata")
                                .and_then(Nbt::as_str)
                                .map(str::to_string)
                        })
                        .flatten()
                })
            } else {
                None
            };
            cells.push(TemplateCell { pos, state, jigsaw, data_marker });
        }
        Ok(StructureTemplate { size, palette, cells })
    }

    /// Every jigsaw socket in the template.
    pub(crate) fn jigsaws(&self) -> impl Iterator<Item = (&TemplateCell, &JigsawInfo)> {
        self.cells
            .iter()
            .filter_map(|c| c.jigsaw.as_ref().map(|j| (c, j)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// The decompiled-source data root (same convention as the
    /// mc-source pipeline): tests that need real templates skip
    /// quietly when the store is absent.
    fn mc_data_root() -> Option<PathBuf> {
        let candidates = [
            std::env::var("INFENGINE_MC_DATA").ok(),
            Some("F:/Data/mc_assets/mc_src/26.1.2/data".to_string()),
        ];
        for c in candidates.into_iter().flatten() {
            let p = PathBuf::from(c);
            if p.join("minecraft/structure").is_dir() {
                return Some(p);
            }
        }
        None
    }

    #[test]
    fn loads_a_real_village_town_center() {
        let Some(root) = mc_data_root() else {
            // No MC data store on this machine - skip quietly.
            return;
        };
        let path = root.join(
            "minecraft/structure/village/plains/town_centers/plains_fountain_01.nbt",
        );
        let bytes = std::fs::read(&path).expect("template file reads");
        let t = StructureTemplate::from_nbt_bytes(&bytes).expect("template parses");
        // The fountain plaza is a small flat plate with streets
        // hanging off it: sane bounds + a populated palette.
        assert!(t.size[0] > 0 && t.size[1] > 0 && t.size[2] > 0);
        assert!(t.size.iter().all(|d| *d <= 48), "village pieces are small");
        assert!(!t.palette.is_empty());
        assert!(!t.cells.is_empty());
        // Every town centre carries at least one street socket.
        let jigsaws: Vec<_> = t.jigsaws().collect();
        assert!(!jigsaws.is_empty(), "town centre without jigsaw sockets");
        for (_c, j) in &jigsaws {
            assert!(j.pool.starts_with("minecraft:"), "pool id: {}", j.pool);
        }
        // Cells stay inside the declared size.
        for c in &t.cells {
            for a in 0..3 {
                assert!(
                    c.pos[a] >= 0 && c.pos[a] < t.size[a],
                    "cell {:?} outside size {:?}",
                    c.pos,
                    t.size
                );
            }
        }
    }

    #[test]
    fn loads_every_plains_template() {
        let Some(root) = mc_data_root() else {
            // No MC data store on this machine - skip quietly.
            return;
        };
        let dir = root.join("minecraft/structure/village/plains");
        let mut count = 0usize;
        let mut stack = vec![dir];
        while let Some(d) = stack.pop() {
            for entry in std::fs::read_dir(&d).expect("dir reads") {
                let p = entry.expect("entry").path();
                if p.is_dir() {
                    stack.push(p);
                } else if p.extension().is_some_and(|e| e == "nbt") {
                    let bytes = std::fs::read(&p).expect("template reads");
                    let t = StructureTemplate::from_nbt_bytes(&bytes)
                        .unwrap_or_else(|e| panic!("{} failed: {e}", p.display()));
                    assert!(!t.palette.is_empty(), "{}", p.display());
                    count += 1;
                }
            }
        }
        assert!(count > 50, "plains set holds dozens of templates: {count}");
    }
}
