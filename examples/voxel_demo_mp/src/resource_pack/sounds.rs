//! MC `sounds.json` parser + OGG path resolver.
//!
//! MC's sound event registry lives at
//! `assets/minecraft/sounds.json`. The JSON is keyed by event
//! name (`"block.stone.break"`, `"entity.cow.ambient"`, ...) and
//! each entry carries a `sounds` array whose elements can be:
//!
//! - A bare string (`"block/stone/break1"`) - the path under
//!   `assets/minecraft/sounds/` minus the `.ogg` extension.
//! - An object (`{ "name": "block/stone/break1", "stream": false,
//!   "type": "sound" }`) - same path, plus optional metadata.
//! - A reference (`{ "name": "block.stone.break", "type": "event" }`)
//!   pointing at ANOTHER event by name. For the loader we resolve
//!   one level of indirection + flatten the targets into the parent.
//!
//! `parse_sounds_json` turns that text into a
//! `HashMap<event_name, Vec<file_path>>`. The file paths are
//! absolute - rooted at `assets/minecraft/sounds/<path>.ogg`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Parse `sounds.json` text into the engine's sound event
/// registry. `mc_root` is the `assets/minecraft/` dir so we can
/// resolve each entry's OGG path. Missing variants (files that
/// don't exist on disk) are skipped; callers can still fire the
/// event - they just won't get audio.
pub(crate) fn parse_sounds_json(
    text: &str,
    mc_root: &Path,
) -> HashMap<String, Vec<PathBuf>> {
    let mut out: HashMap<String, Vec<PathBuf>> = HashMap::new();
    let Ok(v) = serde_json::from_str::<serde_json::Value>(text) else {
        return out;
    };
    let Some(obj) = v.as_object() else { return out };
    let sound_root = mc_root.join("sounds");
    // First pass: collect direct sound files. Event-references get
    // resolved in the second pass.
    let mut event_refs: HashMap<String, Vec<String>> = HashMap::new();
    for (event_name, body) in obj {
        let Some(body_obj) = body.as_object() else { continue };
        let Some(sounds) = body_obj.get("sounds").and_then(|s| s.as_array()) else {
            continue;
        };
        let mut variants: Vec<PathBuf> = Vec::new();
        let mut refs: Vec<String> = Vec::new();
        for entry in sounds {
            match entry {
                serde_json::Value::String(s) => {
                    let p = sound_root.join(format!("{s}.ogg"));
                    if p.exists() {
                        variants.push(p);
                    }
                }
                serde_json::Value::Object(o) => {
                    let kind = o
                        .get("type")
                        .and_then(|t| t.as_str())
                        .unwrap_or("sound");
                    let name = o.get("name").and_then(|n| n.as_str()).unwrap_or("");
                    if name.is_empty() {
                        continue;
                    }
                    if kind == "event" {
                        refs.push(name.to_string());
                    } else {
                        let p = sound_root.join(format!("{name}.ogg"));
                        if p.exists() {
                            variants.push(p);
                        }
                    }
                }
                _ => {}
            }
        }
        if !variants.is_empty() {
            out.insert(event_name.clone(), variants);
        }
        if !refs.is_empty() {
            event_refs.insert(event_name.clone(), refs);
        }
    }
    // Second pass: resolve event-references one level deep. MC's
    // dispatch tree (e.g. "block.wood.break" -> "block.oak_wood.break")
    // would need full transitive resolution; for W9 the single hop
    // covers every event the engine actually fires today.
    let snapshot: HashMap<String, Vec<PathBuf>> = out.clone();
    for (event, refs) in event_refs {
        let mut merged: Vec<PathBuf> = out.get(&event).cloned().unwrap_or_default();
        for r in refs {
            if let Some(target) = snapshot.get(&r) {
                merged.extend(target.iter().cloned());
            }
        }
        if !merged.is_empty() {
            out.insert(event, merged);
        }
    }
    out
}

/// Load `sounds.json` from the MC root + return the parsed
/// registry. Empty when the file is missing.
pub(crate) fn load_sounds_json(mc_root: &Path) -> HashMap<String, Vec<PathBuf>> {
    let p = mc_root.join("sounds.json");
    match std::fs::read_to_string(&p) {
        Ok(t) => parse_sounds_json(&t, mc_root),
        Err(_) => HashMap::new(),
    }
}

/// Mapping from infengine sound keys (matching the existing
/// `AudioBank` keys like `"step:grass"`, `"dig:stone"`) to MC
/// event names. The loader consults this to populate the
/// existing bank from the new registry without forcing every
/// existing call site to learn MC's naming.
pub(crate) const ENGINE_TO_MC: &[(&str, &str)] = &[
    // Block break / step events.
    ("dig:stone", "block.stone.break"),
    ("dig:wood", "block.wood.break"),
    ("dig:grass", "block.grass.break"),
    ("dig:dirt", "block.gravel.break"),
    ("dig:sand", "block.sand.break"),
    ("dig:gravel", "block.gravel.break"),
    ("dig:glass", "block.glass.break"),
    ("step:stone", "block.stone.step"),
    ("step:wood", "block.wood.step"),
    ("step:grass", "block.grass.step"),
    ("step:sand", "block.sand.step"),
    ("step:gravel", "block.gravel.step"),
    // Fire + lava.
    ("fire:ignite", "item.flintandsteel.use"),
    ("fire:ambient", "block.fire.ambient"),
    ("fire:extinguish", "block.fire.extinguish"),
    ("lava:pop", "block.lava.pop"),
    ("lava:ambient", "block.lava.ambient"),
    // Liquids.
    ("splash", "entity.player.splash"),
    ("splash_out", "entity.generic.splash"),
    ("water_flow", "block.water.ambient"),
    // UI.
    ("ui:click", "ui.button.click"),
    // Mobs.
    ("mob/sheep/say", "entity.sheep.ambient"),
    ("mob/sheep/step", "entity.sheep.step"),
];

#[cfg(test)]
mod tests {
    use super::*;

    fn stage_sounds_root(dir: &Path) -> PathBuf {
        let mc = dir.join("assets").join("minecraft");
        let s = mc.join("sounds");
        std::fs::create_dir_all(s.join("block").join("stone")).unwrap();
        std::fs::create_dir_all(s.join("block").join("lava")).unwrap();
        std::fs::create_dir_all(s.join("item").join("flintandsteel")).unwrap();
        // Stage a few fake OGG bytes; the loader checks `exists()`.
        for n in 1..=4 {
            std::fs::write(
                s.join("block").join("stone").join(format!("break{n}.ogg")),
                b"oggdata",
            )
            .unwrap();
        }
        std::fs::write(s.join("block").join("lava").join("pop.ogg"), b"oggdata").unwrap();
        std::fs::write(
            s.join("item").join("flintandsteel").join("use.ogg"),
            b"oggdata",
        )
        .unwrap();
        mc
    }

    fn write_sounds_json(mc: &Path, body: &str) {
        std::fs::write(mc.join("sounds.json"), body).unwrap();
    }

    #[test]
    fn parses_basic_string_form_entries() {
        let tmp = tempfile::tempdir().unwrap();
        let mc = stage_sounds_root(tmp.path());
        let body = r#"{
            "block.stone.break": {
                "sounds": [
                    "block/stone/break1",
                    "block/stone/break2",
                    "block/stone/break3",
                    "block/stone/break4"
                ]
            }
        }"#;
        let reg = parse_sounds_json(body, &mc);
        assert_eq!(reg.get("block.stone.break").map(|v| v.len()), Some(4));
    }

    #[test]
    fn parses_object_form_entries() {
        let tmp = tempfile::tempdir().unwrap();
        let mc = stage_sounds_root(tmp.path());
        let body = r#"{
            "block.lava.pop": {
                "sounds": [
                    { "name": "block/lava/pop", "stream": false }
                ]
            }
        }"#;
        let reg = parse_sounds_json(body, &mc);
        assert_eq!(reg.get("block.lava.pop").map(|v| v.len()), Some(1));
    }

    #[test]
    fn skips_variants_that_dont_exist_on_disk() {
        let tmp = tempfile::tempdir().unwrap();
        let mc = stage_sounds_root(tmp.path());
        let body = r#"{
            "ghost.event": {
                "sounds": [
                    "block/stone/break1",
                    "nonexistent/file/path"
                ]
            }
        }"#;
        let reg = parse_sounds_json(body, &mc);
        // Only one variant resolves to an existing path.
        assert_eq!(reg.get("ghost.event").map(|v| v.len()), Some(1));
    }

    #[test]
    fn unknown_event_returns_empty_registry_entry() {
        let tmp = tempfile::tempdir().unwrap();
        let mc = stage_sounds_root(tmp.path());
        let body = r#"{}"#;
        let reg = parse_sounds_json(body, &mc);
        assert!(!reg.contains_key("anything"));
    }

    #[test]
    fn resolves_event_reference_one_level_deep() {
        let tmp = tempfile::tempdir().unwrap();
        let mc = stage_sounds_root(tmp.path());
        let body = r#"{
            "block.stone.break": {
                "sounds": ["block/stone/break1", "block/stone/break2"]
            },
            "alias.stone.break": {
                "sounds": [ { "name": "block.stone.break", "type": "event" } ]
            }
        }"#;
        let reg = parse_sounds_json(body, &mc);
        // Alias should pick up the parent's 2 variants.
        assert_eq!(reg.get("alias.stone.break").map(|v| v.len()), Some(2));
    }

    #[test]
    fn malformed_json_returns_empty_registry() {
        let tmp = tempfile::tempdir().unwrap();
        let mc = stage_sounds_root(tmp.path());
        let reg = parse_sounds_json("not json", &mc);
        assert!(reg.is_empty());
    }

    #[test]
    fn load_sounds_json_handles_missing_file() {
        let tmp = tempfile::tempdir().unwrap();
        let mc = stage_sounds_root(tmp.path());
        // No sounds.json file written; loader must not panic.
        let reg = load_sounds_json(&mc);
        assert!(reg.is_empty());
    }

    #[test]
    fn load_sounds_json_picks_up_written_file() {
        let tmp = tempfile::tempdir().unwrap();
        let mc = stage_sounds_root(tmp.path());
        write_sounds_json(
            &mc,
            r#"{ "block.stone.break": { "sounds": ["block/stone/break1"] } }"#,
        );
        let reg = load_sounds_json(&mc);
        assert!(reg.contains_key("block.stone.break"));
    }

    #[test]
    fn engine_to_mc_map_has_expected_keys() {
        // Sanity: the existing AudioBank keys all have an MC
        // event partner.
        let keys: std::collections::HashSet<&str> =
            ENGINE_TO_MC.iter().map(|(k, _)| *k).collect();
        for k in [
            "dig:stone",
            "dig:grass",
            "step:stone",
            "fire:ignite",
            "lava:pop",
            "splash",
            "ui:click",
        ] {
            assert!(keys.contains(k), "missing engine key {k}");
        }
    }
}
