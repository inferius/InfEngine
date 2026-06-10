//! W47.28 - new_construct impl block (relocated from app/mod.rs).
#![allow(unused_imports)]
use crate::*;
use super::Settings;
use crate::lighting::HaloLighting;

impl Voxel {

    pub(crate) fn new() -> Self {
        let mut registry = BlockRegistry::new();
        // Drive the runtime block registry from the BLOCK_TYPES
        // table so adding a new block kind is a one-place change.
        // W36.1: apply per-block PBR overrides from block_pbr().
        for bt in BLOCK_TYPES {
            let (metallic, roughness) = block::block_pbr(bt.id);
            let def = BlockDef::with_pbr(bt.name, bt.color, bt.transparent, metallic, roughness);
            registry.insert(BlockId(bt.id), def);
        }
        Self {
            state: AppState::MainMenu,
            settings: Settings::default(),
            requested_quit: false,
            app_start: std::time::Instant::now(),
            renderer: None,
            ui: None,
            registry,
            world: World::new(),
            current_dim: dim::OVERWORLD.to_string(),
            base_seed: 0,
            worlds: HashMap::new(),
            entities_by_dim: HashMap::new(),
            dim_worldgens: HashMap::new(),
            portal_in_ticks: 0,
            portal_cooldown_ticks: 0,
            chunk_meshes: HashMap::new(),
            pending_chunks: BinaryHeap::new(),
            pending_set: HashSet::new(),
            pending_remesh: VecDeque::new(),
            pending_remesh_set: HashSet::new(),
            workers: None,
            chunk_revisions: HashMap::new(),
            pending_lighting_jobs: 0,
            target: None,
            mine_target: None,
            player_pos: Vec3::new(0.0, 32.0, 0.0),
            player_vel: Vec3::ZERO,
            player_yaw: 0.0,
            player_pitch: -0.4,
            has_focus: true,
            last_mouse: None,
            mouse_dx: 0.0,
            mouse_dy: 0.0,
            esc_prev: false,
            e_prev: false,
            f_prev: false,
            // W10.3: double-tap-Space detector starts idle. The
            // first Space press kicks off the timer.
            space_prev: false,
            last_space_press: None,
            hud_visible: true,
            f1_prev: false,
            chat_open: false,
            chat_input: String::new(),
            chat_log: Vec::new(),
            chat_history: Vec::new(),
            chat_history_idx: None,
            chat_completion: None,
            t_prev: false,
            lmb_pending: false,
            rmb_pending: false,
            lmb_just_pressed: false,
            lmb_held: false,
            rmb_held: false,
            mining: None,
            player_health: PLAYER_MAX_HEALTH,
            fall_start_y: None,
            hurt_flash: 0.0,
            regen_accum: 0.0,
            kill_count: 0,
            blocks_broken: 0,
            blocks_placed: 0,
            distance_walked: 0.0,
            last_walk_xz: None,
            place_history: Vec::new(),
            z_prev: false,
            show_chunk_grid: false,
            g_prev: false,
            highlight_mode: HighlightMode::Auto,
            h_prev: false,
            fov_y_rad: 60_f32.to_radians(),
            sprinting: false,
            stamina: STAMINA_MAX,
            player_air: AIR_MAX_SEC,
            remote_interp: HashMap::new(),
            player_name: {
                // Pseudo-random 4-digit suffix so repeat-launch
                // sessions don't accidentally collide.
                let n = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.subsec_nanos())
                    .unwrap_or(0)
                    % 10000;
                format!("Player{n:04}")
            },
            player_names: HashMap::new(),
            tab_held: false,
            pickup_toasts: Vec::new(),
            inventory: {
                // Curated starter hotbar: slot 0 left empty so the
                // player has a free hand for picked-up items; slots
                // 1..8 hold the four wood tools + four basic block
                // bundles so the player can immediately mine across
                // every category without crafting first.
                let mut inv: [Option<(BlockId, u8)>; INVENTORY_SLOTS] =
                    [None; INVENTORY_SLOTS];
                let seed: &[(u16, u8)] = &[
                    (16, 1), // wood pickaxe
                    (23, 1), // wood sword
                    (29, 1), // wood shovel
                    (32, 1), // wood axe
                    (2, 16), // dirt
                    (7, 16), // planks
                    (8, 16), // cobble
                    (10, 4), // torch
                ];
                for (i, (bid, count)) in seed.iter().enumerate() {
                    let slot = i + 1;
                    if slot < HOTBAR_SLOTS {
                        inv[slot] = Some((BlockId(*bid), *count));
                    }
                }
                inv
            },
            // W5.2b: starter inventory has no flint+steel; map
            // stays empty until the first pickup / give of id 48.
            item_durability: HashMap::new(),
            selected_slot: 0,
            inventory_open: false,
            mp_menu_open: false,
            craft_grid: [None; 4],
            wheel_accum: 0.0,
            grounded: false,
            viewport: (1, 1),
            fps_ema: 60.0,
            // Start near mid-morning so the demo opens on a bright
            // sky rather than at sunrise.
            day_time: DAY_LENGTH_S * 0.3,
            net: NetRole::Offline,
            player_cube: None,
            liquid_mesh_handles: HashMap::new(),
            last_built_worldgen: None,
            dropped_items: Vec::new(),
            next_drop_id: 1,
            flow_sfx_accum: 0.0,
            liquid_mesh_dirty: true,
            liquid_chunk_sig: HashMap::new(),
            sheep_mesh: None,
            sheep_head_mesh: None,
            mob_meshes: Default::default(),
            voxel_atlas: None,
            atlas_rgba: None,
            egui_atlas: None,
            egui_panorama: None,
            panorama_scroll: 0.0,
            texture_dir: None,
            resource_pack: Arc::new(resource_pack::ResourcePack::load()),
            particles: {
                // Particle system starts with one "ambient" emitter
                // (rate=0, max=0) that only contributes its gravity
                // + interpolation curves to direct-burst particles.
                let mut s = infengine_particles::ParticleSystem::new();
                s.add_emitter(infengine_particles::Emitter {
                    rate: 0.0,
                    max_particles: 0,
                    gravity: [0.0, -10.0, 0.0],
                    color_start: [0.75, 0.55, 0.30, 1.0],
                    color_end: [0.50, 0.32, 0.18, 0.0],
                    size_start: 0.12,
                    size_end: 0.04,
                    lifetime: 0.5,
                    ..Default::default()
                });
                s
            },
            textured_particles: Vec::new(),
            edit_history: Vec::new(),
            known_client_ids: HashSet::new(),
            last_state_save: None,
            saved_inventories: HashMap::new(),
            // Default slug for the cold-start path. The legacy
            // migration step overrides this to "legacy" if a pre-A4
            // save is found; the Worlds picker rewrites it to the
            // user's pick on New / Load.
            current_world_slug: Some(world_save::DEFAULT_SLUG.to_string()),
            // W8.2: bootstrap defaults to Survival. The actual mode
            // is overridden by the player file on join.
            game_mode: game_mode::GameMode::Survival,
            // W10.1: cached ability bundle. Recomputed at world
            // load + on every `/gamemode` switch.
            abilities: game_mode::Abilities::for_mode(game_mode::GameMode::Survival),
            // W10.6: creative tab starts closed. Opens on E key
            // press in Creative mode.
            creative_tab: creative_tab::CreativeTabState::default(),
            // W20.4: recipe book search query. Starts empty
            // (show all recipes).
            recipe_book_query: String::new(),
            // W8.1: bootstrap state has NO world loaded. The user
            // opts into world simulation by picking Singleplayer >
            // Hrat / Vytvorit or Multiplayer > Connect.
            world_loaded: false,
            net_tick: 0,
            audio: None,
            audio_bank: infengine_audio::AudioBank::new(),
            sounds_dir: None,
            step_cooldown: 0.0,
            last_pickup_sfx: None,
            rain_sfx_accum: 0.0,
            underwater_was: false,
            underwater_sfx_accum: 0.0,
            // First cave sting is eligible a couple of minutes in;
            // re-rolls randomised after each fire.
            cave_sfx_in: 150.0,
            // First music track a few minutes after launch (MC's
            // music gap is 5-20 min; we sit at the friendly end).
            music_in: 180.0,
            heartbeat_accum: 0.0,
            cactus_damage_accum: 0.0,
            fly_mode: false,
            god_mode: false,
            invuln_remaining: 0.0,
            was_night: false,
            was_in_water: false,
            world_locked: false,
            mob_cap: MOB_CAP_DEFAULT,
            freeze_time: false,
            weather: 0,
            lightning_flash: 0.0,
            snow_accum: 0.0,
            snow_rate: 0.4,
            chicken_lay_accum: 0.0,
            debug_chicken_feathers: false,
            drop_tick_accum: 0.0,
            next_lightning_at: 6.0,
            muted_players: HashSet::new(),
            head_bob_phase: 0.0,
            camera_mode: 0,
            f5_prev: false,
            show_debug_panel: false,
            f6_prev: false,
            // Pathfinding enabled by default - matches MC's
            // `FlowingFluid.getSpread` which always biases toward
            // the closest hole. Toggle off via F6 for radial
            // spread (useful when debugging spread bugs).
            liquid_bias_drops: true,
            f4_prev: false,
            q_prev: false,
            home_pos: None,
            spawn_pos: None,
            session_started: None,
            last_day_broadcast: None,
            show_debug: false,
            f3_prev: false,
            torch_positions: HashSet::new(),
            emitter_positions: HashMap::new(),
            portal_positions: HashSet::new(),
            torch_orient: HashMap::new(),
            snow_positions: HashMap::new(),
            chests: HashMap::new(),
            chest_open: None,
            chest_viewers: HashMap::new(),
            furnace_viewers: HashMap::new(),
            furnace_state_last_sent_ms: HashMap::new(),
            chest_held: None,
            furnaces: HashMap::new(),
            furnace_runtimes: HashMap::new(),
            fires: HashMap::new(),
            fire_clock: 0,
            furnace_open: None,
            furnace_held: None,
            crafting_table_open: None,
            craft_grid_3x3: [None; 9],
            crafting_table_held: None,
            brewing_stands: HashMap::new(),
            brewing_runtimes: HashMap::new(),
            brewing_open: None,
            brewing_held: None,
            door_open: std::collections::HashSet::new(),
            liquid_schedule: LiquidSchedule::new(),
            liquid_clock: 0,
            liquid_tick_accum: 0.0,
            liquid_level: HashMap::new(),
            liquid_positions: HashSet::new(),
            falling_pending: HashSet::new(),
            persistent_leaves: HashSet::new(),
            lever_on: HashSet::new(),
            lighting: lighting::LightingState::new(),
            last_lighting_tick_s: 0.0,
            mobs: Vec::new(),
            remote_mobs: HashMap::new(),
            last_mob_spawn_time: 0.0,
            last_despawn_check_time: 0.0,
            next_mob_id: 1,
            mob_step_timers: HashMap::new(),
            mob_say_timers: HashMap::new(),
            mob_last_xz: HashMap::new(),
            mob_hurt_flash: HashMap::new(),
            mob_no_hit_time: HashMap::new(),
            mob_dying: HashMap::new(),
            mob_lunge: HashMap::new(),
            master_addr: format!(
                "91.232.214.139:{}",
                infengine_net::master::MASTER_PORT
            ),
            direct_addr_buf: "127.0.0.1:7777".into(),
            host_name_buf: "My InfEngine Server".into(),
            host_port_buf: "7777".into(),
            host_announce: true,
            browser_servers: Vec::new(),
            browser_error: None,
            browser_last_refresh: None,
            browser_query_rx: None,
            announce_query_rx: None,
            master_token: None,
            last_heartbeat: None,
            new_world_name_buf: String::new(),
            // The OptionsSnapshot here mirrors Voxel::settings + a
            // few fields outside `Settings`. Kept in sync with the
            // runtime by `sync_menu_options` before each render so
            // the screen always opens with the current values.
            menu: menu::Menu::new(menu::OptionsSnapshot {
                player_name: String::new(),
                master_addr: String::new(),
                view_distance: VIEW_DISTANCE_CHUNKS as u8,
                fov_deg: 60.0,
                mouse_sensitivity: 0.6,
                show_fps: true,
                head_bob: true,
                invert_mouse_y: false,
                invert_wheel: false,
                lock_cursor: true,
                hide_cursor: true,
                // W9.7: filled by `sync_menu_options` from the live
                // resource pack before each render.
                mc_assets_path: String::new(),
                mc_assets_resolved: String::new(),
                mc_assets_is_fallback: true,
            }),
            plugins: Some(plugin::PluginRegistry::new()),
            plugin_blocks: Vec::new(),
            // W28.9: cache starts empty; filled on first render_ui call.
            icon_uv_table: HashMap::new(),
            icon_uv_table_frame: u32::MAX,
            // W28.9: preallocated drain buffer for stream_chunks.
            stream_delivered: Vec::with_capacity(32),
        }
    }
}
