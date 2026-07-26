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

/// W57 - Load sound registry from the MC content-addressable
/// asset store. Modern launchers ship `sounds.json` + the .ogg
/// files only here (the decompiled `assets/minecraft/` tree
/// doesn't carry them). Walks the index for both the sounds.json
/// object and every `.ogg` variant referenced inside it.
pub(crate) fn load_sounds_via_store(
    store: &super::mc_asset_store::AssetStore,
) -> HashMap<String, Vec<PathBuf>> {
    let mut out: HashMap<String, Vec<PathBuf>> = HashMap::new();
    // Step 1: load sounds.json itself.
    let sounds_json_path = match store.resolve("minecraft/sounds.json") {
        Some(p) => p,
        None => {
            tracing::warn!(
                "asset-store has no `minecraft/sounds.json` entry",
            );
            return out;
        }
    };
    let text = match std::fs::read_to_string(&sounds_json_path) {
        Ok(t) => t,
        Err(_) => return out,
    };
    let v: serde_json::Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let obj = match v.as_object() {
        Some(o) => o,
        None => return out,
    };
    // Step 2: walk each event + collect direct + event-ref names.
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
                    let logical = format!("minecraft/sounds/{s}.ogg");
                    if let Some(p) = store.resolve(&logical) {
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
                        let logical = format!("minecraft/sounds/{name}.ogg");
                        if let Some(p) = store.resolve(&logical) {
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
    // Step 3: resolve one-level event references.
    let snapshot = out.clone();
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
    tracing::info!(
        events = out.len(),
        "MC sounds.json loaded from asset store",
    );
    out
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
    // W201 - underwater plants (kelp / seagrass) use MC's wet_grass
    // sound family.
    ("dig:wet_grass", "block.wet_grass.break"),
    ("step:wet_grass", "block.wet_grass.step"),
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
    ("mob/sheep/hurt", "entity.sheep.hurt"),
    ("mob/sheep/death", "entity.sheep.death"),
    // W55.4 - per-mob ambient + step + hurt + death events.
    ("mob/zombie/say", "entity.zombie.ambient"),
    // W315: cat (stray meows for the village prowlers).
    ("mob/cat/say", "entity.cat.ambient"),
    ("mob/cat/hurt", "entity.cat.hurt"),
    ("mob/cat/death", "entity.cat.death"),
    // W322: iron golem has NO ambient voice in MC - say stays
    // unmapped on purpose (sfx_at misses silently, W56.1 opt-out).
    ("mob/iron_golem/hurt", "entity.iron_golem.hurt"),
    ("mob/iron_golem/death", "entity.iron_golem.death"),
    // W339: the defense swing.
    ("mob/iron_golem/attack", "entity.iron_golem.attack"),
    // W348: riding + taming.
    ("mob/horse/angry", "entity.horse.angry"),
    ("mob/horse/jump", "entity.horse.jump"),
    ("mob/donkey/angry", "entity.donkey.angry"),
    // W322: horse.
    ("mob/horse/say", "entity.horse.ambient"),
    ("mob/horse/hurt", "entity.horse.hurt"),
    ("mob/horse/death", "entity.horse.death"),
    // W327: ghast (moans carry across the nether).
    ("mob/ghast/say", "entity.ghast.ambient"),
    ("mob/ghast/hurt", "entity.ghast.hurt"),
    ("mob/ghast/death", "entity.ghast.death"),
    // W337: fireball attack + the blast itself.
    ("mob/ghast/charge", "entity.ghast.warn"),
    ("mob/ghast/fireball", "entity.ghast.shoot"),
    ("explosion", "entity.generic.explode"),
    ("tnt/primed", "entity.tnt.primed"),
    // W343: creeper fuse + hurt/death (no ambient in vanilla).
    ("mob/creeper/fuse", "entity.creeper.primed"),
    ("mob/creeper/hurt", "entity.creeper.hurt"),
    ("mob/creeper/death", "entity.creeper.death"),
    // W409: camel.
    ("mob/camel/say", "entity.camel.ambient"),
    ("mob/camel/hurt", "entity.camel.hurt"),
    ("mob/camel/death", "entity.camel.death"),
    ("mob/camel/step", "entity.camel.step"),
    // W344: donkey.
    ("mob/donkey/say", "entity.donkey.ambient"),
    ("mob/donkey/hurt", "entity.donkey.hurt"),
    ("mob/donkey/death", "entity.donkey.death"),
    // W327: magma cube (squishy hops).
    ("mob/magma_cube/say", "entity.magma_cube.squish"),
    ("mob/magma_cube/hurt", "entity.magma_cube.hurt"),
    ("mob/magma_cube/death", "entity.magma_cube.death"),
    // W330: zombie villager.
    ("mob/zombie_villager/say", "entity.zombie_villager.ambient"),
    ("mob/zombie_villager/hurt", "entity.zombie_villager.hurt"),
    ("mob/zombie_villager/death", "entity.zombie_villager.death"),
    // W302: villager (the classic hrmm).
    ("mob/villager/say", "entity.villager.ambient"),
    ("mob/villager/hurt", "entity.villager.hurt"),
    ("mob/villager/death", "entity.villager.death"),
    // W300: zombified piglin (zpig*.ogg family in the vanilla pack).
    ("mob/zombified_piglin/say", "entity.zombified_piglin.ambient"),
    ("mob/zombified_piglin/hurt", "entity.zombified_piglin.hurt"),
    ("mob/zombified_piglin/death", "entity.zombified_piglin.death"),
    ("mob/zombie/step", "entity.zombie.step"),
    ("mob/zombie/hurt", "entity.zombie.hurt"),
    ("mob/zombie/death", "entity.zombie.death"),
    ("mob/chicken/say", "entity.chicken.ambient"),
    ("mob/chicken/step", "entity.chicken.step"),
    ("mob/chicken/hurt", "entity.chicken.hurt"),
    ("mob/chicken/death", "entity.chicken.death"),
    ("mob/pig/say", "entity.pig.ambient"),
    ("mob/pig/step", "entity.pig.step"),
    ("mob/pig/hurt", "entity.pig.hurt"),
    ("mob/pig/death", "entity.pig.death"),
    ("mob/cow/say", "entity.cow.ambient"),
    ("mob/cow/step", "entity.cow.step"),
    ("mob/cow/hurt", "entity.cow.hurt"),
    ("mob/cow/death", "entity.cow.death"),
    ("mob/skeleton/say", "entity.skeleton.ambient"),
    ("mob/skeleton/step", "entity.skeleton.step"),
    ("mob/skeleton/hurt", "entity.skeleton.hurt"),
    ("mob/skeleton/death", "entity.skeleton.death"),
    // W140 - skeleton bow + arrow impacts.
    ("mob/skeleton/shoot", "entity.skeleton.shoot"),
    ("arrow/hit", "entity.arrow.hit"),
    // W156 - nether portal hum + travel whoosh.
    ("portal/trigger", "block.portal.trigger"),
    ("portal/travel", "block.portal.travel"),
    // W158 - wolf.
    ("mob/wolf/say", "entity.wolf.ambient"),
    ("mob/wolf/step", "entity.wolf.step"),
    ("mob/wolf/hurt", "entity.wolf.hurt"),
    ("mob/wolf/death", "entity.wolf.death"),
    // W161 - polar bear.
    ("mob/polar_bear/say", "entity.polar_bear.ambient"),
    ("mob/polar_bear/step", "entity.polar_bear.step"),
    ("mob/polar_bear/hurt", "entity.polar_bear.hurt"),
    ("mob/polar_bear/death", "entity.polar_bear.death"),
    // W162 - rabbit.
    ("mob/rabbit/say", "entity.rabbit.ambient"),
    ("mob/rabbit/step", "entity.rabbit.jump"),
    ("mob/rabbit/hurt", "entity.rabbit.hurt"),
    ("mob/rabbit/death", "entity.rabbit.death"),
    ("mob/spider/say", "entity.spider.ambient"),
    ("mob/spider/step", "entity.spider.step"),
    ("mob/spider/hurt", "entity.spider.hurt"),
    ("mob/spider/death", "entity.spider.death"),
    // W51 - per-material dig + step (MC `block.<mat>.*` events).
    ("dig:wool", "block.wool.break"),
    ("step:wool", "block.wool.step"),
    ("dig:snow", "block.snow.break"),
    ("step:snow", "block.snow.step"),
    ("dig:metal", "block.metal.break"),
    ("step:metal", "block.metal.step"),
    ("dig:slime", "block.slime_block.break"),
    ("step:slime", "block.slime_block.step"),
    ("dig:honey", "block.honey_block.break"),
    ("step:honey", "block.honey_block.step"),
    ("dig:coral", "block.coral_block.break"),
    ("step:coral", "block.coral_block.step"),
    ("dig:sculk", "block.sculk.break"),
    ("step:sculk", "block.sculk.step"),
    ("dig:nether_wart", "block.nether_wart.break"),
    ("step:nether_wart", "block.nether_wart.step"),
    ("dig:nylium", "block.nylium.break"),
    ("step:nylium", "block.nylium.step"),
    ("dig:netherrack", "block.netherrack.break"),
    ("step:netherrack", "block.netherrack.step"),
    ("dig:basalt", "block.basalt.break"),
    ("step:basalt", "block.basalt.step"),
    ("dig:bone", "block.bone_block.break"),
    ("step:bone", "block.bone_block.step"),
    ("dig:nether_bricks", "block.nether_bricks.break"),
    ("step:nether_bricks", "block.nether_bricks.step"),
    // Doors (wooden + iron) open / close.
    ("door/wood/open", "block.wooden_door.open"),
    ("door/wood/close", "block.wooden_door.close"),
    ("door/iron/open", "block.iron_door.open"),
    ("door/iron/close", "block.iron_door.close"),
    // Trapdoors.
    ("trapdoor/wood/open", "block.wooden_trapdoor.open"),
    ("trapdoor/wood/close", "block.wooden_trapdoor.close"),
    ("trapdoor/iron/open", "block.iron_trapdoor.open"),
    ("trapdoor/iron/close", "block.iron_trapdoor.close"),
    // Containers.
    ("container/chest/open", "block.chest.open"),
    ("container/chest/close", "block.chest.close"),
    ("container/barrel/open", "block.barrel.open"),
    ("container/barrel/close", "block.barrel.close"),
    ("container/ender_chest/open", "block.ender_chest.open"),
    ("container/ender_chest/close", "block.ender_chest.close"),
    ("container/shulker_box/open", "block.shulker_box.open"),
    ("container/shulker_box/close", "block.shulker_box.close"),
    // Redstone hardware.
    ("redstone/button/click_on", "block.stone_button.click_on"),
    ("redstone/button/click_off", "block.stone_button.click_off"),
    ("redstone/button/wood/click_on", "block.wooden_button.click_on"),
    ("redstone/button/wood/click_off", "block.wooden_button.click_off"),
    ("redstone/lever/click", "block.lever.click"),
    ("redstone/piston/extend", "block.piston.extend"),
    ("redstone/piston/contract", "block.piston.contract"),
    ("redstone/dispenser/dispense", "block.dispenser.dispense"),
    ("redstone/dispenser/fail", "block.dispenser.fail"),
    ("redstone/note_block/bass", "block.note_block.bass"),
    ("redstone/note_block/snare", "block.note_block.snare"),
    ("redstone/note_block/hat", "block.note_block.hat"),
    ("redstone/note_block/harp", "block.note_block.harp"),
    ("redstone/note_block/bell", "block.note_block.bell"),
    ("redstone/note_block/flute", "block.note_block.flute"),
    ("redstone/note_block/chime", "block.note_block.chime"),
    ("redstone/note_block/guitar", "block.note_block.guitar"),
    // Workstations / utility blocks.
    ("workstation/furnace/fire_crackle", "block.furnace.fire_crackle"),
    ("workstation/smoker/smoke", "block.smoker.smoke"),
    ("workstation/blast_furnace/fire_crackle", "block.blastfurnace.fire_crackle"),
    ("workstation/anvil/use", "block.anvil.use"),
    ("workstation/anvil/land", "block.anvil.land"),
    ("workstation/anvil/break", "block.anvil.break"),
    ("workstation/grindstone/use", "block.grindstone.use"),
    ("workstation/smithing_table/use", "block.smithing_table.use"),
    ("workstation/brewing_stand/brew", "block.brewing_stand.brew"),
    ("workstation/beacon/activate", "block.beacon.activate"),
    ("workstation/beacon/deactivate", "block.beacon.deactivate"),
    ("workstation/beacon/ambient", "block.beacon.ambient"),
    ("workstation/conduit/activate", "block.conduit.activate"),
    ("workstation/conduit/deactivate", "block.conduit.deactivate"),
    ("workstation/conduit/ambient", "block.conduit.ambient"),
    ("workstation/bell/use", "block.bell.use"),
    ("workstation/bell/resonate", "block.bell.resonate"),
    // Misc ambient.
    ("portal/ambient", "block.portal.ambient"),
    ("portal/travel", "block.portal.travel"),
    ("portal/trigger", "block.portal.trigger"),
    ("dragon_egg/jump", "block.dragon_egg.jump"),
    // W123 - player feedback events.
    ("player/hurt", "entity.player.hurt"),
    ("player/death", "entity.player.death"),
    ("hurt", "entity.player.hurt"),
    ("drown", "entity.player.hurt_drown"),
    ("burn", "entity.player.hurt_on_fire"),
    ("void", "entity.player.hurt"),
    ("eat", "entity.generic.eat"),
    ("burp", "entity.player.burp"),
    ("item/pickup", "entity.item.pickup"),
    ("xp/pickup", "entity.experience_orb.pickup"),
    ("player/levelup", "entity.player.levelup"),
    // W123 - weather + world ambience.
    ("thunder", "entity.lightning_bolt.thunder"),
    ("thunder/impact", "entity.lightning_bolt.impact"),
    ("ambient/rain", "weather.rain"),
    ("ambient/cave", "ambient.cave"),
    ("ambient/underwater", "ambient.underwater.loop"),
    ("ambient/underwater_enter", "ambient.underwater.enter"),
    ("ambient/underwater_exit", "ambient.underwater.exit"),
    // W123 - misc events with real MC OGGs.
    ("firework/launch", "entity.firework_rocket.launch"),
    ("firework/blast", "entity.firework_rocket.blast"),
    // W123 - per-material dig/step added with the name-driven
    // block_material classifier.
    ("dig:deepslate", "block.deepslate.break"),
    ("step:deepslate", "block.deepslate.step"),
    ("step:glass", "block.glass.step"),
    ("dig:mud", "block.mud.break"),
    ("step:mud", "block.mud.step"),
    // W123 - background music (lazy-loaded one track at a time;
    // see app_on_update music timer).
    ("music/game", "music.game"),
    ("music/creative", "music.creative"),
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

    /// W123: every material `block_material` can emit must have
    /// dig+step rows here, or the bank silently never loads it and
    /// digging that family falls back to the synth thud.
    #[test]
    fn every_emitted_material_has_engine_rows() {
        use crate::block::{BLOCK_TYPES, block_material};
        use infengine_voxel::BlockId;
        let keys: std::collections::HashSet<&str> =
            ENGINE_TO_MC.iter().map(|(k, _)| *k).collect();
        let mut seen = std::collections::HashSet::new();
        for bt in BLOCK_TYPES {
            seen.insert(block_material(BlockId(bt.id)));
        }
        for mat in seen {
            let dig = format!("dig:{mat}");
            let step = format!("step:{mat}");
            assert!(keys.contains(dig.as_str()), "missing {dig} row");
            assert!(keys.contains(step.as_str()), "missing {step} row");
        }
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
