//! MC blockstates JSON parser.
//!
//! Blockstates files live at
//! `assets/minecraft/blockstates/<block_name>.json` and pick which
//! model to render given the block's state properties. Two shapes:
//!
//! 1. **`variants`** - a key->model map keyed by a comma-separated
//!    `<prop>=<value>` string (e.g. `"facing=north"`). The empty
//!    key `""` matches any state - that's the shape `torch.json`
//!    uses today. Multiple model entries (a JSON array) are MC's
//!    random-weight variant picker; for Wave 9.x we collapse to
//!    the FIRST entry to keep behaviour deterministic.
//!
//! 2. **`multipart`** - a list of `{ "when": ..., "apply": ... }`
//!    rules that compose multiple model fragments based on
//!    neighbour connections (used by fences, walls, redstone).
//!    Multipart is DEFERRED in Wave 9.x - the parser preserves
//!    enough metadata to know the file uses it, but selection
//!    returns `None`.
//!
//! Reference: decompiled MC 26.1.2 ships the loader at
//! `net/minecraft/client/resources/model/BlockStateModelLoader.java`
//! (Mojang's full implementation). Our parser is a much-simplified
//! port - we keep just enough to dispatch the engine's basic
//! blocks to their models.

use std::collections::HashMap;
use std::path::Path;

use serde_json::Value;

use super::ResourcePack;

/// A single model reference inside a variant entry. `model` is the
/// resolved model path (e.g. `"block/torch"`). `x` / `y` are the
/// extrinsic rotations in degrees (0/90/180/270). `uvlock` keeps
/// the UVs world-aligned after rotation. `weight` is the random
/// pick weight (default 1) - we ignore it but parse it for round-
/// trip correctness.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ModelRef {
    pub(crate) model: String,
    pub(crate) x: u16,
    pub(crate) y: u16,
    pub(crate) uvlock: bool,
    pub(crate) weight: u8,
}

impl ModelRef {
    pub(crate) fn new(model: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            x: 0,
            y: 0,
            uvlock: false,
            weight: 1,
        }
    }
}

/// A single key inside the `variants` map: an unordered set of
/// `name=value` pairs. The empty key matches every state. We
/// store the parsed pairs so callers can do exact-property
/// matches without re-parsing strings on every lookup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BlockStateKey {
    pub(crate) pairs: Vec<(String, String)>,
}

impl BlockStateKey {
    /// Parse `"facing=north,half=top"` into key-value pairs. The
    /// empty input yields an empty `pairs` vec (matches every
    /// state). Each pair is trimmed but values are preserved
    /// case-sensitive.
    pub(crate) fn parse(raw: &str) -> Self {
        let mut pairs = Vec::new();
        if raw.trim().is_empty() {
            return Self { pairs };
        }
        for chunk in raw.split(',') {
            if let Some((k, v)) = chunk.split_once('=') {
                pairs.push((k.trim().to_string(), v.trim().to_string()));
            }
        }
        Self { pairs }
    }

    /// True when every pair in `self` is also present in `props`.
    /// The empty key matches every set of props (default variant).
    pub(crate) fn matches(&self, props: &BlockStateProps) -> bool {
        self.pairs.iter().all(|(k, v)| props.get(k) == Some(v.as_str()))
    }
}

/// Runtime block-state property map. Today the engine carries
/// limited state on blocks (most are stateless); this struct exists
/// so the future per-block-state machinery has a clean integration
/// point.
#[derive(Debug, Clone, Default)]
pub(crate) struct BlockStateProps {
    inner: HashMap<String, String>,
}

impl BlockStateProps {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn with(mut self, k: impl Into<String>, v: impl Into<String>) -> Self {
        self.inner.insert(k.into(), v.into());
        self
    }

    pub(crate) fn get(&self, k: &str) -> Option<&str> {
        self.inner.get(k).map(String::as_str)
    }
}

/// Neighbour connection flags for multipart blockstate resolution.
///
/// MC fences/walls/panes/bars use `"north": "true"` / `"false"` conditions
/// to pick which arm models to show. The engine samples the 4 horizontal
/// neighbours and passes this struct down to the multipart selector.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct NeighborStates {
    pub(crate) north: bool,
    pub(crate) south: bool,
    pub(crate) east: bool,
    pub(crate) west: bool,
    pub(crate) up: bool,
    pub(crate) down: bool,
}

impl NeighborStates {
    /// Look up the boolean value of a direction property by name.
    /// Returns `None` for unknown property names.
    /// W455: the six connection bools as true/false props - the
    /// shared multipart matcher consumes props only.
    pub(crate) fn as_props(&self) -> BlockStateProps {
        let b = |v: bool| if v { "true" } else { "false" };
        BlockStateProps::new()
            .with("north", b(self.north))
            .with("south", b(self.south))
            .with("east", b(self.east))
            .with("west", b(self.west))
            .with("up", b(self.up))
            .with("down", b(self.down))
    }

    pub(crate) fn get_bool(&self, prop: &str) -> Option<bool> {
        match prop {
            "north" => Some(self.north),
            "south" => Some(self.south),
            "east"  => Some(self.east),
            "west"  => Some(self.west),
            "up"    => Some(self.up),
            "down"  => Some(self.down),
            _ => None,
        }
    }
}

/// A single condition inside a multipart `when` clause.
/// Supports the simple `{ "prop": "value" }` form and the
/// `{ "OR": [ ... ] }` compound form.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum MultipartCondition {
    /// All key=value pairs must match (AND of pairs).
    All(Vec<(String, String)>),
    /// Any of the sub-conditions must match (OR).
    AnyOf(Vec<MultipartCondition>),
}

impl MultipartCondition {
    /// True when the condition matches the given neighbour states.
    /// W455: delegates through real props so fences/panes (plain
    /// true/false conditions) share one matcher semantic with the
    /// wall arm's none/low/tall state.
    pub(crate) fn matches_neighbors(&self, n: &NeighborStates) -> bool {
        self.matches_props(&n.as_props())
    }

    /// W455: evaluate the condition against REAL state props (the
    /// wall mesher passes none/low/tall arm states). Values support
    /// MC's `|` alternation ("low|tall"). Unknown props never
    /// match - a bool-fed pane condition cannot accidentally turn
    /// on a wall arm.
    pub(crate) fn matches_props(&self, props: &BlockStateProps) -> bool {
        match self {
            MultipartCondition::All(pairs) => {
                pairs.iter().all(|(k, v)| {
                    props
                        .get(k)
                        .map(|actual| {
                            v.split('|').any(|alt| alt == actual)
                        })
                        .unwrap_or(false)
                })
            }
            MultipartCondition::AnyOf(subs) => {
                subs.iter().any(|c| c.matches_props(props))
            }
        }
    }
}

/// One part in a `multipart` blockstates array.
/// `when` is `None` for unconditional parts (always applied).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MultipartPart {
    pub(crate) when: Option<MultipartCondition>,
    pub(crate) apply: ModelRef,
}

/// Parsed blockstates file. For variant-form blocks we keep the
/// ordered (key, model) list so a default-pick + multi-prop match
/// share the same data path. Multipart blocks store the parts list;
/// `variants` is empty for them.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BlockState {
    pub(crate) variants: Vec<(BlockStateKey, ModelRef)>,
    pub(crate) is_multipart: bool,
    /// Populated only when `is_multipart == true`.
    pub(crate) parts: Vec<MultipartPart>,
}

impl BlockState {
    /// True when the file declared `multipart`. W40.1 now resolves
    /// multipart geometry via `select_multipart_models`.
    pub(crate) fn is_multipart(&self) -> bool {
        self.is_multipart
    }

    /// Return the first variant's `ModelRef`, or `None` when the
    /// blockstate is multipart or has no variants at all.
    ///
    /// W39: used by `resolve_block_model` as the "pick first" policy.
    /// Works for all simple variant-form blockstates. Multipart files
    /// route through `select_multipart_models` instead.
    pub(crate) fn first_variant_model(&self) -> Option<&ModelRef> {
        if self.is_multipart {
            return None;
        }
        // W46.35: prefer the "lit=true" / "powered=true" / "on=true"
        // variant when present. MC stores redstone_torch with
        // `lit=false` first which our engine resolved to the unlit
        // template (1 stick element, no halo). The engine has no
        // redstone-power state yet so we always want the lit variant
        // - it carries the proper 7-element model with the halo
        // glow around the tip.
        const LIT_HINTS: &[(&str, &str)] = &[
            ("lit", "true"),
            ("powered", "true"),
            ("on", "true"),
            ("active", "true"),
        ];
        for (key, val) in LIT_HINTS {
            if let Some((_, m)) = self
                .variants
                .iter()
                .find(|(k, _)| {
                    k.pairs.iter().any(|(pk, pv)| pk == key && pv == val)
                })
            {
                return Some(m);
            }
        }
        // W229.1: pillar blocks (logs / basalt / bone...) list their
        // variants as axis=x/y/z with x FIRST - picking that rendered
        // every JSON-resolved log lying sideways (user report:
        // cherry_log ring texture on the trunk sides). The engine has
        // no per-block axis state yet; upright is the placement +
        // worldgen default.
        if let Some((_, m)) = self
            .variants
            .iter()
            .find(|(k, _)| k.pairs.iter().any(|(pk, pv)| pk == "axis" && pv == "y"))
        {
            return Some(m);
        }
        self.variants.first().map(|(_, m)| m)
    }

    /// Return all model refs whose `when` condition matches `neighbors`.
    /// Unconditional parts (no `when`) are always included.
    /// Returns an empty slice for non-multipart blockstates.
    pub(crate) fn select_multipart_models<'a>(
        &'a self,
        neighbors: &NeighborStates,
    ) -> Vec<&'a ModelRef> {
        if !self.is_multipart {
            return Vec::new();
        }
        self.parts
            .iter()
            .filter(|p| match &p.when {
                None => true,
                Some(cond) => cond.matches_neighbors(neighbors),
            })
            .map(|p| &p.apply)
            .collect()
    }
}

/// Look up a blockstates file under `pack.source/blockstates/<name>.json`.
/// Returns `None` when the file is missing or unparseable.
pub(crate) fn load_blockstate(pack: &ResourcePack, name: &str) -> Option<BlockState> {
    load_blockstate_at(&pack.source, name)
}

/// Same as `load_blockstate` but takes an explicit MC root path.
/// Used by tests to stage a synthetic blockstates tree.
pub(crate) fn load_blockstate_at(root: &Path, name: &str) -> Option<BlockState> {
    let path = root.join("blockstates").join(format!("{name}.json"));
    let text = std::fs::read_to_string(&path).ok()?;
    parse(&text)
}

/// Parse blockstates JSON text. Exposed for tests; production
/// code uses `load_blockstate_at`.
pub(crate) fn parse(text: &str) -> Option<BlockState> {
    let v: Value = serde_json::from_str(text).ok()?;
    if let Some(variants_obj) = v.get("variants").and_then(|x| x.as_object()) {
        let mut variants: Vec<(BlockStateKey, ModelRef)> = Vec::with_capacity(variants_obj.len());
        for (k, val) in variants_obj {
            let key = BlockStateKey::parse(k);
            let model_ref = match val {
                // Plain object form: one model.
                Value::Object(_) => parse_model_ref(val),
                // Array form: MC's random-weighted picker. We
                // collapse to the FIRST entry for determinism.
                Value::Array(arr) => arr.first().and_then(parse_model_ref),
                _ => None,
            };
            if let Some(m) = model_ref {
                variants.push((key, m));
            }
        }
        return Some(BlockState {
            variants,
            is_multipart: false,
            parts: Vec::new(),
        });
    }
    if let Some(multipart_arr) = v.get("multipart").and_then(|x| x.as_array()) {
        let parts = multipart_arr
            .iter()
            .filter_map(parse_multipart_part)
            .collect();
        return Some(BlockState {
            variants: Vec::new(),
            is_multipart: true,
            parts,
        });
    }
    None
}

/// Parse one element of the `multipart` array.
fn parse_multipart_part(v: &Value) -> Option<MultipartPart> {
    // `apply` is required; `when` is optional.
    let apply_val = v.get("apply")?;
    let apply = match apply_val {
        Value::Array(arr) => arr.first().and_then(parse_model_ref)?,
        _ => parse_model_ref(apply_val)?,
    };
    let when = v.get("when").and_then(parse_multipart_condition);
    Some(MultipartPart { when, apply })
}

/// Parse a `when` object. Supports:
/// - `{ "prop": "value", ... }` -> `MultipartCondition::All`
/// - `{ "OR": [ ... ] }` -> `MultipartCondition::AnyOf`
fn parse_multipart_condition(v: &Value) -> Option<MultipartCondition> {
    let obj = v.as_object()?;
    // OR form
    if let Some(or_arr) = obj.get("OR").and_then(|x| x.as_array()) {
        let subs: Vec<_> = or_arr.iter().filter_map(parse_multipart_condition).collect();
        if subs.is_empty() {
            return None;
        }
        return Some(MultipartCondition::AnyOf(subs));
    }
    // Simple key=value form
    let pairs: Vec<(String, String)> = obj
        .iter()
        .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
        .collect();
    if pairs.is_empty() {
        return None;
    }
    Some(MultipartCondition::All(pairs))
}

fn parse_model_ref(v: &Value) -> Option<ModelRef> {
    let obj = v.as_object()?;
    let model_raw = obj.get("model").and_then(|m| m.as_str())?;
    let model = super::models::strip_namespace(model_raw);
    let x = obj.get("x").and_then(|n| n.as_u64()).unwrap_or(0) as u16;
    let y = obj.get("y").and_then(|n| n.as_u64()).unwrap_or(0) as u16;
    let uvlock = obj.get("uvlock").and_then(|b| b.as_bool()).unwrap_or(false);
    let weight = obj.get("weight").and_then(|n| n.as_u64()).unwrap_or(1) as u8;
    Some(ModelRef {
        model,
        x,
        y,
        uvlock,
        weight,
    })
}

/// Pick the model ref to render given the runtime state properties.
/// Returns the FIRST exact match, falling back to the empty-key
/// "default" variant when no match exists. Returns `None` for
/// multipart blockstates (Wave 9.x defers them).
pub(crate) fn select_variant<'a>(
    state: &'a BlockState,
    props: &BlockStateProps,
) -> Option<&'a ModelRef> {
    if state.is_multipart {
        return None;
    }
    // Prefer the longest (most-specific) matching key so partial
    // overlaps don't shadow exact matches.
    state
        .variants
        .iter()
        .filter(|(k, _)| k.matches(props))
        .max_by_key(|(k, _)| k.pairs.len())
        .map(|(_, m)| m)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// W455: multipart conditions evaluate against real state
    /// props with MC's `|` alternation; the NeighborStates bool
    /// path builds true/false props and shares the matcher.
    #[test]
    fn multipart_conditions_match_real_props() {
        let cond = |k: &str, v: &str| {
            MultipartCondition::All(vec![(k.to_string(), v.to_string())])
        };
        let wall = BlockStateProps::new()
            .with("north", "low")
            .with("east", "tall")
            .with("south", "none")
            .with("up", "true");
        assert!(cond("north", "low").matches_props(&wall));
        assert!(!cond("north", "tall").matches_props(&wall));
        assert!(cond("east", "tall").matches_props(&wall));
        assert!(cond("south", "none").matches_props(&wall));
        assert!(cond("up", "true").matches_props(&wall));
        assert!(
            !cond("west", "low").matches_props(&wall),
            "missing prop never matches",
        );
        // MC alternation.
        assert!(cond("north", "low|tall").matches_props(&wall));
        assert!(cond("east", "low|tall").matches_props(&wall));
        assert!(!cond("south", "low|tall").matches_props(&wall));
        // Bool path delegates through true/false props: a pane's
        // "true" condition follows the connection, and a wall's
        // "low" condition can never fire from a bool feed.
        let n = NeighborStates {
            north: true,
            south: false,
            east: false,
            west: false,
            up: false,
            down: false,
        };
        assert!(cond("north", "true").matches_neighbors(&n));
        assert!(!cond("south", "true").matches_neighbors(&n));
        assert!(cond("south", "false").matches_neighbors(&n));
        assert!(!cond("north", "low").matches_neighbors(&n));
    }

    #[test]
    fn key_parse_empty_matches_anything() {
        let k = BlockStateKey::parse("");
        assert!(k.pairs.is_empty());
        assert!(k.matches(&BlockStateProps::new()));
        assert!(k.matches(&BlockStateProps::new().with("any", "value")));
    }

    #[test]
    fn key_parse_single_pair() {
        let k = BlockStateKey::parse("facing=north");
        assert_eq!(k.pairs, vec![("facing".into(), "north".into())]);
    }

    #[test]
    fn key_parse_multi_pair() {
        let k = BlockStateKey::parse("facing=east,half=top");
        assert_eq!(
            k.pairs,
            vec![
                ("facing".into(), "east".into()),
                ("half".into(), "top".into()),
            ]
        );
    }

    #[test]
    fn key_matches_subset_of_props() {
        let k = BlockStateKey::parse("facing=north");
        let p = BlockStateProps::new()
            .with("facing", "north")
            .with("half", "top");
        assert!(k.matches(&p));
    }

    #[test]
    fn key_fails_with_wrong_value() {
        let k = BlockStateKey::parse("facing=north");
        let p = BlockStateProps::new().with("facing", "south");
        assert!(!k.matches(&p));
    }

    #[test]
    fn parse_single_variant_picks_default() {
        // Matches MC's `torch.json` shape.
        let text = r#"{
            "variants": { "": { "model": "minecraft:block/torch" } }
        }"#;
        let s = parse(text).expect("parsed");
        assert!(!s.is_multipart());
        assert_eq!(s.variants.len(), 1);
        let m = select_variant(&s, &BlockStateProps::new()).expect("variant");
        assert_eq!(m.model, "block/torch");
        assert_eq!(m.x, 0);
        assert_eq!(m.y, 0);
    }

    #[test]
    fn parse_facing_variants() {
        // Matches MC's wall-torch / lever shape.
        let text = r#"{
            "variants": {
                "facing=north": { "model": "block/torch_wall", "y": 0 },
                "facing=east":  { "model": "block/torch_wall", "y": 90 },
                "facing=south": { "model": "block/torch_wall", "y": 180 },
                "facing=west":  { "model": "block/torch_wall", "y": 270 }
            }
        }"#;
        let s = parse(text).expect("parsed");
        let east = select_variant(&s, &BlockStateProps::new().with("facing", "east"))
            .expect("east variant");
        assert_eq!(east.model, "block/torch_wall");
        assert_eq!(east.y, 90);
        let west = select_variant(&s, &BlockStateProps::new().with("facing", "west"))
            .expect("west variant");
        assert_eq!(west.y, 270);
    }

    #[test]
    fn parse_array_variant_picks_first_entry() {
        // Matches MC's `stone.json` shape (random rotation).
        let text = r#"{
            "variants": {
                "": [
                    { "model": "minecraft:block/stone" },
                    { "model": "minecraft:block/stone_mirrored" },
                    { "model": "minecraft:block/stone", "y": 180 }
                ]
            }
        }"#;
        let s = parse(text).expect("parsed");
        let m = select_variant(&s, &BlockStateProps::new()).expect("variant");
        assert_eq!(m.model, "block/stone");
    }

    #[test]
    fn parse_multipart_returns_marker() {
        // Matches MC's `oak_fence.json` shape.
        let text = r#"{
            "multipart": [
                { "apply": { "model": "block/oak_fence_post" } },
                { "when": { "north": "true" },
                  "apply": { "model": "block/oak_fence_side", "uvlock": true } }
            ]
        }"#;
        let s = parse(text).expect("parsed");
        assert!(s.is_multipart());
        // Wave 9.x defers multipart selection.
        assert!(select_variant(&s, &BlockStateProps::new()).is_none());
    }

    #[test]
    fn parse_axis_variants_strips_namespace() {
        let text = r#"{
            "variants": {
                "axis=x": { "model": "minecraft:block/oak_log_horizontal", "x": 90, "y": 90 },
                "axis=y": { "model": "minecraft:block/oak_log" },
                "axis=z": { "model": "minecraft:block/oak_log_horizontal", "x": 90 }
            }
        }"#;
        let s = parse(text).expect("parsed");
        let y = select_variant(&s, &BlockStateProps::new().with("axis", "y"))
            .expect("axis=y");
        assert_eq!(y.model, "block/oak_log");
        let x = select_variant(&s, &BlockStateProps::new().with("axis", "x"))
            .expect("axis=x");
        assert_eq!(x.x, 90);
        assert_eq!(x.y, 90);
    }

    #[test]
    fn select_falls_back_to_empty_default() {
        // No exact match on `facing=north`, but empty key applies.
        let state = BlockState {
            variants: vec![(
                BlockStateKey::parse(""),
                ModelRef::new("block/torch"),
            )],
            is_multipart: false,
            parts: Vec::new(),
        };
        let m = select_variant(&state, &BlockStateProps::new().with("facing", "north"))
            .expect("default variant");
        assert_eq!(m.model, "block/torch");
    }

    #[test]
    fn select_prefers_most_specific_match() {
        // Both keys match props; the multi-pair one wins.
        let state = BlockState {
            variants: vec![
                (BlockStateKey::parse(""), ModelRef::new("block/generic")),
                (
                    BlockStateKey::parse("facing=north,half=top"),
                    ModelRef::new("block/specific"),
                ),
            ],
            is_multipart: false,
            parts: Vec::new(),
        };
        let p = BlockStateProps::new()
            .with("facing", "north")
            .with("half", "top");
        let m = select_variant(&state, &p).expect("variant");
        assert_eq!(m.model, "block/specific");
    }

    #[test]
    fn parse_uvlock_and_weight_round_trip() {
        let text = r#"{
            "variants": {
                "": { "model": "block/oak_planks", "uvlock": true, "weight": 5 }
            }
        }"#;
        let s = parse(text).expect("parsed");
        let m = select_variant(&s, &BlockStateProps::new()).expect("variant");
        assert!(m.uvlock);
        assert_eq!(m.weight, 5);
    }

    #[test]
    fn load_blockstate_reads_disk() {
        let tmp = tempfile::tempdir().unwrap();
        let bs_dir = tmp.path().join("blockstates");
        std::fs::create_dir_all(&bs_dir).unwrap();
        std::fs::write(
            bs_dir.join("torch.json"),
            r#"{ "variants": { "": { "model": "block/torch" } } }"#,
        )
        .unwrap();
        let s = load_blockstate_at(tmp.path(), "torch").expect("blockstate");
        let m = select_variant(&s, &BlockStateProps::new()).expect("variant");
        assert_eq!(m.model, "block/torch");
    }

    #[test]
    fn load_blockstate_missing_returns_none() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(load_blockstate_at(tmp.path(), "nope").is_none());
    }

    #[test]
    fn parse_malformed_json_returns_none() {
        assert!(parse("{ no").is_none());
    }

    // ---- W40.1: multipart parsing tests ----

    #[test]
    fn parse_multipart_stores_parts() {
        // Minimal oak_fence-like multipart JSON.
        let text = r#"{
            "multipart": [
                { "apply": { "model": "block/oak_fence_post" } },
                { "when": { "north": "true" },
                  "apply": { "model": "block/oak_fence_side", "uvlock": true } },
                { "when": { "south": "true" },
                  "apply": { "model": "block/oak_fence_side", "y": 180, "uvlock": true } }
            ]
        }"#;
        let s = parse(text).expect("parsed");
        assert!(s.is_multipart());
        assert_eq!(s.parts.len(), 3);
        // First part: no condition.
        assert!(s.parts[0].when.is_none());
        assert_eq!(s.parts[0].apply.model, "block/oak_fence_post");
        // Second part: north=true condition.
        assert!(s.parts[1].when.is_some());
        assert_eq!(s.parts[1].apply.model, "block/oak_fence_side");
        assert!(s.parts[1].apply.uvlock);
        // Third part: south=true, y=180.
        assert_eq!(s.parts[2].apply.y, 180);
    }

    #[test]
    fn select_multipart_unconditional_always_included() {
        let text = r#"{
            "multipart": [
                { "apply": { "model": "block/fence_post" } }
            ]
        }"#;
        let s = parse(text).expect("parsed");
        let refs = s.select_multipart_models(&NeighborStates::default());
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].model, "block/fence_post");
    }

    #[test]
    fn select_multipart_conditional_north_arm() {
        let text = r#"{
            "multipart": [
                { "apply": { "model": "block/fence_post" } },
                { "when": { "north": "true" },
                  "apply": { "model": "block/fence_side" } }
            ]
        }"#;
        let s = parse(text).expect("parsed");
        // Without north connection: only post.
        let no_north = NeighborStates { north: false, ..Default::default() };
        let refs = s.select_multipart_models(&no_north);
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].model, "block/fence_post");
        // With north connection: post + side.
        let with_north = NeighborStates { north: true, ..Default::default() };
        let refs2 = s.select_multipart_models(&with_north);
        assert_eq!(refs2.len(), 2);
        assert_eq!(refs2[1].model, "block/fence_side");
    }

    #[test]
    fn select_multipart_or_condition() {
        let text = r#"{
            "multipart": [
                { "when": { "OR": [
                    { "north": "true" },
                    { "south": "true" }
                  ] },
                  "apply": { "model": "block/fence_side" }
                }
            ]
        }"#;
        let s = parse(text).expect("parsed");
        // south=true should match the OR.
        let refs = s.select_multipart_models(&NeighborStates { south: true, ..Default::default() });
        assert_eq!(refs.len(), 1);
        // neither north nor south: no match.
        let refs2 = s.select_multipart_models(&NeighborStates::default());
        assert_eq!(refs2.len(), 0);
    }

    #[test]
    fn neighbor_states_get_bool_round_trip() {
        let n = NeighborStates {
            north: true, south: false, east: true,
            west: false, up: true, down: false,
        };
        assert_eq!(n.get_bool("north"), Some(true));
        assert_eq!(n.get_bool("south"), Some(false));
        assert_eq!(n.get_bool("east"),  Some(true));
        assert_eq!(n.get_bool("west"),  Some(false));
        assert_eq!(n.get_bool("up"),    Some(true));
        assert_eq!(n.get_bool("down"),  Some(false));
        assert_eq!(n.get_bool("unknown"), None);
    }
}
