//! Menu state-machine tests. The egui screens are exercised by
//! the surrounding integration but most behaviour is testable
//! directly on the `Menu` + `MenuAction` data types.

use super::*;

fn fresh_options() -> OptionsSnapshot {
    OptionsSnapshot {
        player_name: "Tester".to_string(),
        master_addr: "127.0.0.1:6000".to_string(),
        view_distance: 6,
        lod_distance: 0,
        lod2_distance: 0,
        fov_deg: 75.0,
        mouse_sensitivity: 0.6,
        show_fps: true,
        head_bob: true,
        invert_mouse_y: false,
        invert_wheel: false,
        lock_cursor: true,
        hide_cursor: true,
        mc_assets_path: String::new(),
        mc_assets_resolved: String::new(),
        mc_assets_is_fallback: true,
        language: "en_us".to_string(),
        ui_scale: 1.0,
    }
}

#[test]
fn menu_starts_on_main_screen() {
    let menu = Menu::new(fresh_options());
    assert!(matches!(menu.screen, MenuScreen::Main));
}

#[test]
fn esc_from_singleplayer_returns_to_main() {
    let mut menu = Menu::new(fresh_options());
    menu.apply_switch(default_singleplayer());
    assert!(matches!(menu.screen, MenuScreen::Singleplayer { .. }));
    let handled = menu.on_escape();
    assert!(handled);
    assert!(matches!(menu.screen, MenuScreen::Main));
}

#[test]
fn esc_from_multiplayer_returns_to_main() {
    let mut menu = Menu::new(fresh_options());
    menu.apply_switch(default_multiplayer());
    let handled = menu.on_escape();
    assert!(handled);
    assert!(matches!(menu.screen, MenuScreen::Main));
}

#[test]
fn esc_from_options_returns_to_main() {
    let mut menu = Menu::new(fresh_options());
    menu.apply_switch(MenuScreen::Options);
    let handled = menu.on_escape();
    assert!(handled);
    assert!(matches!(menu.screen, MenuScreen::Main));
}

#[test]
fn esc_on_main_is_a_noop_for_the_menu_module() {
    let mut menu = Menu::new(fresh_options());
    let handled = menu.on_escape();
    // Caller (main.rs) keeps its own handler for "Esc on Main";
    // the menu module reports it didn't consume the key.
    assert!(!handled);
    assert!(matches!(menu.screen, MenuScreen::Main));
}

#[test]
fn options_snapshot_round_trips_mc_assets_path() {
    // W9.7: ApplyOptions persists `mc_assets_path` via Settings; the
    // round-trip on the snapshot itself just verifies the field is
    // preserved across clone + clamp.
    let mut opts = fresh_options();
    opts.mc_assets_path = "C:/some/mc/assets".to_string();
    opts.clamp();
    assert_eq!(opts.mc_assets_path, "C:/some/mc/assets");
    // Clamp on the snapshot must not strip the resource path.
    let clone = opts.clone();
    assert_eq!(clone.mc_assets_path, "C:/some/mc/assets");
}

#[test]
fn reload_resource_pack_action_carries_path() {
    // W9.7: the Options screen emits a ReloadResourcePack action
    // with the current path. Direct equality check on the variant.
    let action = MenuAction::ReloadResourcePack {
        path: "Z:/test/mc".to_string(),
    };
    match action {
        MenuAction::ReloadResourcePack { path } => {
            assert_eq!(path, "Z:/test/mc");
        }
        _ => panic!("expected ReloadResourcePack variant"),
    }
}

#[test]
fn default_mc_assets_field_is_empty_string() {
    // W9.7: a fresh OptionsSnapshot starts with an empty
    // `mc_assets_path` so the auto-discovery path is the default.
    let opts = fresh_options();
    assert_eq!(opts.mc_assets_path, "");
    assert!(opts.mc_assets_is_fallback);
}

#[test]
fn esc_on_ingame_is_handled_by_the_caller() {
    let mut menu = Menu::new(fresh_options());
    menu.apply_switch(MenuScreen::InGame);
    let handled = menu.on_escape();
    assert!(!handled);
    assert!(matches!(menu.screen, MenuScreen::InGame));
}

#[test]
fn default_singleplayer_clears_form_buffers() {
    let s = default_singleplayer();
    match s {
        MenuScreen::Singleplayer {
            selected_slug,
            search,
            new_world_open,
            confirm_delete,
        } => {
            assert!(selected_slug.is_none());
            assert!(search.is_empty());
            assert!(new_world_open.is_none());
            assert!(confirm_delete.is_none());
        }
        _ => panic!("expected Singleplayer"),
    }
}

#[test]
fn default_multiplayer_clears_form_buffers() {
    let s = default_multiplayer();
    match s {
        MenuScreen::Multiplayer {
            selected_addr,
            search,
            direct_connect_open,
            add_server_open,
            confirm_remove,
        } => {
            assert!(selected_addr.is_none());
            assert!(search.is_empty());
            assert!(direct_connect_open.is_none());
            assert!(add_server_open.is_none());
            assert!(confirm_remove.is_none());
        }
        _ => panic!("expected Multiplayer"),
    }
}

#[test]
fn worlds_cache_is_stale_until_first_store() {
    let menu = Menu::new(fresh_options());
    assert!(menu.worlds_cache_stale());
}

#[test]
fn worlds_cache_fresh_after_store() {
    let mut menu = Menu::new(fresh_options());
    menu.store_worlds_cache(Vec::new());
    assert!(!menu.worlds_cache_stale());
}

#[test]
fn invalidate_caches_marks_both_stale() {
    let mut menu = Menu::new(fresh_options());
    menu.store_worlds_cache(Vec::new());
    menu.store_saved_servers(Vec::new());
    assert!(!menu.worlds_cache_stale());
    assert!(!menu.saved_servers_stale());
    menu.invalidate_caches();
    assert!(menu.worlds_cache_stale());
    assert!(menu.saved_servers_stale());
}

#[test]
fn options_snapshot_clamp_caps_view_distance() {
    // W22.1: cap raised from 16 to VIEW_DISTANCE_MAX (32).
    let mut opts = fresh_options();
    opts.view_distance = 99;
    opts.clamp();
    assert_eq!(opts.view_distance, crate::VIEW_DISTANCE_MAX as u8);
}

#[test]
fn options_snapshot_clamp_floors_view_distance() {
    let mut opts = fresh_options();
    opts.view_distance = 0;
    opts.clamp();
    assert_eq!(opts.view_distance, 2);
}

#[test]
fn options_snapshot_clamp_fov_range() {
    let mut opts = fresh_options();
    opts.fov_deg = 500.0;
    opts.clamp();
    assert_eq!(opts.fov_deg, 120.0);
    opts.fov_deg = 1.0;
    opts.clamp();
    assert_eq!(opts.fov_deg, 30.0);
}

#[test]
fn options_snapshot_clamp_truncates_player_name() {
    let mut opts = fresh_options();
    opts.player_name = "a".repeat(50);
    opts.clamp();
    assert_eq!(opts.player_name.len(), 24);
}

#[test]
fn options_snapshot_clamp_strips_control_chars() {
    let mut opts = fresh_options();
    opts.player_name = "Hello\x07\nWorld".to_string();
    opts.clamp();
    assert_eq!(opts.player_name, "HelloWorld");
}

#[test]
fn options_snapshot_clamp_mouse_sensitivity() {
    let mut opts = fresh_options();
    opts.mouse_sensitivity = 10.0;
    opts.clamp();
    assert_eq!(opts.mouse_sensitivity, 4.0);
}

#[test]
fn validate_world_name_rejects_empty() {
    assert!(validate_world_name("").is_none());
    assert!(validate_world_name("   ").is_none());
    assert!(validate_world_name("\x07\x08").is_none());
}

#[test]
fn validate_world_name_caps_length() {
    let big = "x".repeat(200);
    let cleaned = validate_world_name(&big).expect("valid");
    assert_eq!(cleaned.len(), 64);
}

#[test]
fn validate_world_name_trims_whitespace() {
    let cleaned = validate_world_name("  Hello World  ").expect("valid");
    assert_eq!(cleaned, "Hello World");
}

#[test]
fn quit_action_round_trips_through_vec() {
    // Trivial regression: the action enum is consumed by `main.rs`
    // by draining a `Vec<MenuAction>`. Make sure the variant we
    // push out of the Main screen survives a clone + equality.
    let action = MenuAction::Quit;
    let cloned = action.clone();
    assert_eq!(action, cloned);
}

#[test]
fn switch_screen_action_carries_target_variant() {
    let target = MenuScreen::Options;
    let action = MenuAction::SwitchScreen(target.clone());
    match action {
        MenuAction::SwitchScreen(t) => assert_eq!(t, target),
        _ => panic!("wrong action variant"),
    }
}

#[test]
fn play_offline_action_carries_slug() {
    let action = MenuAction::PlayOffline {
        slug: "default".to_string(),
    };
    match action {
        MenuAction::PlayOffline { slug } => assert_eq!(slug, "default"),
        _ => panic!("wrong action variant"),
    }
}

#[test]
fn saved_server_serde_roundtrip() {
    let s = SavedServer {
        name: "Home".to_string(),
        addr: "127.0.0.1:7777".to_string(),
    };
    let bytes = serde_json::to_vec(&s).unwrap();
    let back: SavedServer = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(s, back);
}

#[test]
fn cache_ttl_is_under_one_second() {
    // The brief asks for "<= 2 Hz from the menu cache". 500 ms
    // satisfies that with margin; assert the constant so a
    // future drift can't sneak past the contract.
    assert!(CACHE_TTL <= std::time::Duration::from_millis(500));
    assert!(CACHE_TTL >= std::time::Duration::from_millis(100));
}

#[test]
fn apply_switch_overwrites_screen() {
    let mut menu = Menu::new(fresh_options());
    menu.apply_switch(MenuScreen::Options);
    assert!(matches!(menu.screen, MenuScreen::Options));
    menu.apply_switch(default_multiplayer());
    assert!(matches!(menu.screen, MenuScreen::Multiplayer { .. }));
}

#[test]
fn invalidate_caches_clears_world_timestamp() {
    let mut menu = Menu::new(fresh_options());
    menu.store_worlds_cache(vec![]);
    assert!(menu.worlds_cache_at.is_some());
    menu.invalidate_caches();
    assert!(menu.worlds_cache_at.is_none());
}

#[test]
fn new_world_action_with_nonempty_name_round_trips() {
    let cleaned = validate_world_name("Pretty World").expect("valid");
    let action = MenuAction::NewWorld {
        name: cleaned.clone(),
        worldgen: String::new(),
        seed: None,
        game_mode: GameMode::Survival,
    };
    match action {
        MenuAction::NewWorld { name, worldgen, seed, game_mode } => {
            assert_eq!(name, "Pretty World");
            assert!(worldgen.is_empty());
            assert!(seed.is_none());
            assert_eq!(game_mode, GameMode::Survival);
        }
        _ => panic!("wrong variant"),
    }
}

#[test]
fn default_new_world_uses_mc_and_survival() {
    // W184: "mc" (faithful MC final_density terrain) is the default
    // generator; "Survival" is the default game mode. The form starts
    // with both pre-selected so the user can just type a name + click
    // Vytvorit svet.
    let s = default_new_world();
    match s {
        MenuScreen::NewWorld {
            name,
            seed,
            generator,
            game_mode,
        } => {
            assert!(name.is_empty());
            assert!(seed.is_empty());
            assert_eq!(generator, "mc");
            assert_eq!(game_mode, GameMode::Survival);
        }
        _ => panic!("expected NewWorld"),
    }
}

#[test]
fn parse_seed_field_empty_is_none() {
    // W8.2: empty seed field -> None (= runtime picks random).
    assert!(parse_seed_field("").is_none());
    assert!(parse_seed_field("   ").is_none());
}

#[test]
fn parse_seed_field_numeric_pins_value() {
    // W8.2: numeric input parses into a `u32` and pins exactly
    // that value into the action payload.
    assert_eq!(parse_seed_field("42"), Some(42));
    assert_eq!(parse_seed_field("17539214"), Some(17539214));
    assert_eq!(parse_seed_field("0"), Some(0));
}

#[test]
fn parse_seed_field_garbage_is_none() {
    // W8.2: non-numeric text falls back to None so a typo doesn't
    // panic + the user can still create a world (with a fresh
    // random seed).
    assert!(parse_seed_field("not a number").is_none());
    assert!(parse_seed_field("3.14").is_none());
}

#[test]
fn parse_seed_field_trims_whitespace() {
    assert_eq!(parse_seed_field("  17  "), Some(17));
}

#[test]
fn esc_from_new_world_returns_to_singleplayer() {
    // W8.2: ESC on the new-world screen should NOT drop the user
    // all the way back to Main - they want to keep browsing the
    // existing slot list.
    let mut menu = Menu::new(fresh_options());
    menu.apply_switch(default_new_world());
    let handled = menu.on_escape();
    assert!(handled);
    assert!(matches!(menu.screen, MenuScreen::Singleplayer { .. }));
}

#[test]
fn new_world_action_with_pinned_seed_round_trips() {
    let action = MenuAction::NewWorld {
        name: "Test".to_string(),
        worldgen: "realistic".to_string(),
        seed: Some(7),
        game_mode: GameMode::Creative,
    };
    match action {
        MenuAction::NewWorld { seed, game_mode, .. } => {
            assert_eq!(seed, Some(7));
            assert_eq!(game_mode, GameMode::Creative);
        }
        _ => panic!("wrong variant"),
    }
}

#[test]
fn delete_world_action_carries_slug() {
    let action = MenuAction::DeleteWorld {
        slug: "test-slot".to_string(),
    };
    if let MenuAction::DeleteWorld { slug } = action {
        assert_eq!(slug, "test-slot");
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn direct_connect_action_carries_addr() {
    let action = MenuAction::DirectConnect {
        addr: "1.2.3.4:7777".to_string(),
    };
    if let MenuAction::DirectConnect { addr } = action {
        assert_eq!(addr, "1.2.3.4:7777");
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn join_server_action_carries_addr() {
    let action = MenuAction::JoinServer {
        addr: "1.2.3.4:7777".to_string(),
    };
    if let MenuAction::JoinServer { addr } = action {
        assert_eq!(addr, "1.2.3.4:7777");
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn add_saved_server_action_carries_fields() {
    let action = MenuAction::AddSavedServer {
        name: "Home".to_string(),
        addr: "127.0.0.1:7777".to_string(),
    };
    if let MenuAction::AddSavedServer { name, addr } = action {
        assert_eq!(name, "Home");
        assert_eq!(addr, "127.0.0.1:7777");
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn options_apply_action_carries_snapshot() {
    let action = MenuAction::ApplyOptions {
        snapshot: fresh_options(),
    };
    if let MenuAction::ApplyOptions { snapshot } = action {
        assert_eq!(snapshot.player_name, "Tester");
        assert_eq!(snapshot.view_distance, 6);
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn browse_refresh_action_is_unit_variant() {
    // Trivial: assert the Refresh variant pattern-matches. Catches
    // a future signature change that adds payload fields.
    let action = MenuAction::BrowseServersRefresh;
    if !matches!(action, MenuAction::BrowseServersRefresh) {
        panic!("wrong variant");
    }
}

#[test]
fn switch_to_singleplayer_from_main_clears_form_state() {
    let mut menu = Menu::new(fresh_options());
    menu.apply_switch(default_singleplayer());
    if let MenuScreen::Singleplayer {
        selected_slug,
        search,
        new_world_open,
        confirm_delete,
    } = &menu.screen
    {
        assert!(selected_slug.is_none());
        assert!(search.is_empty());
        assert!(new_world_open.is_none());
        assert!(confirm_delete.is_none());
    } else {
        panic!("expected Singleplayer");
    }
}
