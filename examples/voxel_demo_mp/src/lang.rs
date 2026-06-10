//! W124 - MC language files: localized item/block display names +
//! the multilingual search haystack.
//!
//! MC mirror:
//! - `assets/minecraft/lang/<code>.json` is a flat
//!   `"translation.key" -> "Display Name"` map. `en_us.json` ships
//!   in the client jar (our extracted pack dir); every other
//!   language lives in the content-addressable asset store
//!   (`minecraft/lang/<code>.json`).
//! - Item keys are `item.minecraft.<name>`, block items
//!   `block.minecraft.<name>`. We try both (block items resolve
//!   under `block.*` in MC).
//!
//! The active language is a process-wide snapshot (`RwLock`) so the
//! existing free-function call sites (`item_display_name` users in
//! the UI) can swap to `localized_item_name` without threading
//! state. Search ALWAYS matches the active language AND English AND
//! the raw MC name AND the per-item extra keywords
//! (`item::item_search_keywords`), per the user mandate.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use crate::resource_pack::ResourcePack;

static ACTIVE: RwLock<Option<Arc<LangState>>> = RwLock::new(None);

/// Loaded language snapshot.
pub(crate) struct LangState {
    /// Active-language translation map.
    active: HashMap<String, String>,
    /// English map - the fallback layer + the second search corpus.
    english: HashMap<String, String>,
}

/// Parse a flat MC lang JSON (`"key": "value"`). Non-string values
/// are skipped (MC lang files are all-string).
fn parse_lang_json(text: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let Ok(v) = serde_json::from_str::<serde_json::Value>(text) else {
        return out;
    };
    let Some(obj) = v.as_object() else { return out };
    for (k, val) in obj {
        if let Some(s) = val.as_str() {
            out.insert(k.clone(), s.to_string());
        }
    }
    out
}

/// Load one language map by code. Resolution order:
/// 1. the extracted pack dir (`<assets_root>/lang/<code>.json`,
///    where `en_us.json` lives),
/// 2. the asset store (`minecraft/lang/<code>.json`, every other
///    shipped language).
fn load_lang_map(pack: &ResourcePack, code: &str) -> HashMap<String, String> {
    if !pack.is_fallback {
        let p = pack.source.join("lang").join(format!("{code}.json"));
        if let Ok(text) = std::fs::read_to_string(&p) {
            let m = parse_lang_json(&text);
            if !m.is_empty() {
                return m;
            }
        }
    }
    if let Some(store) = crate::resource_pack::mc_asset_store::discover_asset_store()
        && let Some(p) = store.resolve(&format!("minecraft/lang/{code}.json"))
        && let Ok(text) = std::fs::read_to_string(&p)
    {
        let m = parse_lang_json(&text);
        if !m.is_empty() {
            return m;
        }
    }
    HashMap::new()
}

/// Load `code` + English and publish them as the process-wide
/// language snapshot. Call at bootstrap and whenever the Options
/// language picker changes.
pub(crate) fn load_and_set(pack: &ResourcePack, code: &str) {
    let english = load_lang_map(pack, "en_us");
    let active = if code == "en_us" {
        english.clone()
    } else {
        load_lang_map(pack, code)
    };
    tracing::info!(
        code,
        active = active.len(),
        english = english.len(),
        "language maps loaded",
    );
    let state = LangState { active, english };
    *ACTIVE.write().expect("lang lock") = Some(Arc::new(state));
}

/// Snapshot the active language state (cheap Arc clone).
fn snapshot() -> Option<Arc<LangState>> {
    ACTIVE.read().ok().and_then(|g| g.clone())
}

/// List the language codes available on this machine: `en_us` from
/// the pack dir plus everything the asset store ships. Sorted.
pub(crate) fn available_languages() -> Vec<String> {
    let mut out = vec!["en_us".to_string()];
    if let Some(store) = crate::resource_pack::mc_asset_store::discover_asset_store() {
        for code in store.list_lang_codes() {
            if !out.contains(&code) {
                out.push(code);
            }
        }
    }
    out.sort();
    out
}

/// Candidate translation keys for an item id. The explicit
/// `name_map` row (engine name -> MC name) wins; the raw
/// `ITEM_TYPES` name covers the (majority) ids whose engine name
/// IS the MC name. Block items resolve under `block.minecraft.*`.
fn item_lang_keys(id: u16) -> Vec<String> {
    let mut names: Vec<&str> = Vec::with_capacity(2);
    if let Some(mc) = crate::resource_pack::name_map::lookup_item(id) {
        names.push(mc);
    }
    if let Some(it) = crate::item::item_type(id)
        && !names.contains(&it.name)
    {
        names.push(it.name);
    }
    let mut out = Vec::with_capacity(names.len() * 2);
    for n in names {
        out.push(format!("item.minecraft.{n}"));
        out.push(format!("block.minecraft.{n}"));
    }
    out
}

/// Candidate translation keys for a BLOCK id (HUD target label).
fn block_lang_keys(id: u16) -> Vec<String> {
    let mut names: Vec<&str> = Vec::with_capacity(2);
    if let Some(m) = crate::resource_pack::name_map::lookup_block(id) {
        names.push(m.block);
    }
    if let Some(bt) = crate::block::block_type(id)
        && !names.contains(&bt.name)
    {
        names.push(bt.name);
    }
    let mut out = Vec::with_capacity(names.len() * 2);
    for n in names {
        out.push(format!("block.minecraft.{n}"));
        out.push(format!("item.minecraft.{n}"));
    }
    out
}

fn translate(keys: &[String]) -> Option<String> {
    let state = snapshot()?;
    for k in keys {
        if let Some(v) = state.active.get(k) {
            return Some(v.clone());
        }
    }
    for k in keys {
        if let Some(v) = state.english.get(k) {
            return Some(v.clone());
        }
    }
    None
}

/// Localized display name for an item id. Resolution: active
/// language -> English -> the engine's `item_display_name`
/// (display override / title-cased name).
pub(crate) fn localized_item_name(id: u16) -> String {
    translate(&item_lang_keys(id)).unwrap_or_else(|| crate::item::item_display_name(id))
}

/// Localized display name for a block id (HUD target label).
pub(crate) fn localized_block_name(id: u16) -> String {
    if let Some(v) = translate(&block_lang_keys(id)) {
        return v;
    }
    match crate::block::block_type(id) {
        Some(bt) => bt.name.replace('_', " "),
        None => format!("block_{id}"),
    }
}

/// Lower-cased search corpus for an item: localized name + English
/// name + raw MC/engine names (underscores spaced) + the per-item
/// extra keywords. Creative tab + recipe book both filter against
/// this so a Czech UI still finds "chest" and a keyword like
/// "bedna" works in any language.
pub(crate) fn search_haystack(id: u16) -> String {
    let mut hay = String::with_capacity(96);
    let keys = item_lang_keys(id);
    if let Some(state) = snapshot() {
        for k in &keys {
            if let Some(v) = state.active.get(k) {
                hay.push_str(v);
                hay.push(' ');
                break;
            }
        }
        for k in &keys {
            if let Some(v) = state.english.get(k) {
                hay.push_str(v);
                hay.push(' ');
                break;
            }
        }
    }
    // Engine display name (covers the no-pack fallback + display
    // overrides that differ from MC).
    hay.push_str(&crate::item::item_display_name(id));
    hay.push(' ');
    // Raw snake names, spaced.
    if let Some(it) = crate::item::item_type(id) {
        hay.push_str(&it.name.replace('_', " "));
        hay.push(' ');
    }
    if let Some(mc) = crate::resource_pack::name_map::lookup_item(id) {
        hay.push_str(&mc.replace('_', " "));
        hay.push(' ');
    }
    // Extra search keywords (recipes + creative, per user mandate).
    for kw in crate::item::item_search_keywords(id) {
        hay.push_str(kw);
        hay.push(' ');
    }
    hay.to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_lang_json_reads_flat_map() {
        let m = parse_lang_json(
            r#"{ "block.minecraft.stone": "Kámen", "item.minecraft.stick": "Klacek", "num": 3 }"#,
        );
        assert_eq!(m.get("block.minecraft.stone").map(String::as_str), Some("Kámen"));
        assert_eq!(m.get("item.minecraft.stick").map(String::as_str), Some("Klacek"));
        assert_eq!(m.len(), 2, "non-string values skipped");
    }

    #[test]
    fn haystack_works_without_lang_state() {
        // No lang loaded in unit tests: the haystack still carries
        // the engine display name + raw names + keywords.
        let hay = search_haystack(1);
        assert!(hay.contains("stone"), "haystack: {hay}");
    }
}
