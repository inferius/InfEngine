//! W47.20 - app bootstrap (`on_start` body), relocated from `main.rs`.
//!
//! `on_start` runs once when the engine window comes up. It
//! discovers + loads plugins, restores persisted settings, builds
//! the renderer + atlas, loads sheep / mob textures, sets up the
//! audio bank, and (when launched as a CLI server) enters the
//! world immediately.
//!
//! Free function with `&mut Voxel` + the `&EngineWindow`. The
//! method on `Voxel` stays as a one-line thin wrapper so the
//! `AppHandler` impl keeps working.

use std::sync::Arc;

use infengine_render::Renderer;
use infengine_window::EngineWindow;
use infengine_editor::EditorUi;
use tracing::{error, info, warn};

use crate::{Voxel, load_block_atlas_from_dir, mob_meshes, plugin, resource_pack};
use crate::net::NetRole;

/// W47.20: drive the per-process startup sequence against `vx`.
pub(crate) fn on_start(vx: &mut Voxel, window: &EngineWindow) {
    let (w, h) = window.size();
    vx.viewport = (w, h);
    // Wave 7: discover + load plugins. Failures logged + skipped.
    let plugin_dir = plugin::PluginRegistry::discover_dir();
    if let Some(reg) = vx.plugins.as_mut() {
        reg.load_all(&plugin_dir);
    }
    // Load persisted settings before any user interaction.
    vx.load_config();
    // W9.7: honour any user-stored MC resource pack override path.
    if let Some(override_path) = vx.settings.mc_assets_path.clone() {
        vx.resource_pack = Arc::new(resource_pack::ResourcePack::load_with_override(
            Some(std::path::PathBuf::from(override_path)),
        ));
    }
    vx.session_started = Some(std::time::Instant::now());
    // Drop the MOTD lines into the local chat log on startup.
    let motd = Voxel::motd_lines();
    if !motd.is_empty() {
        let now = std::time::Instant::now();
        for line in motd {
            vx.chat_log.push((now, format!("[MOTD] {line}")));
        }
    }
    // Pick a default world seed for solo / offline play.
    if vx.base_seed == 0 {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0)
            .wrapping_mul(0x9E37_79B1)
            | 1;
        vx.base_seed = seed;
    }
    info!(
        width = w,
        height = h,
        seed = vx.base_seed,
        "voxel_demo started"
    );
    match Renderer::new(window) {
        Ok(r) => {
            let ui = EditorUi::new(r.wgpu_device(), r.surface_format(), window.raw(), 1);
            vx.ui = Some(ui);
            vx.renderer = Some(r);
        }
        Err(err) => {
            error!(error = %err, "renderer init failed");
            std::process::exit(2);
        }
    }
    // Build the block atlas + upload it to the renderer.
    if let Some(r) = vx.renderer.as_mut() {
        // W9.2: prefer the MC resource-pack atlas when available.
        let (aw, ah, atlas_rgba) = if vx.resource_pack.is_fallback {
            vx.texture_dir
                .as_ref()
                .and_then(|d| load_block_atlas_from_dir(d))
                .unwrap_or_else(|| {
                    let (w, h) = vx.resource_pack.atlas_size;
                    (w, h, vx.resource_pack.atlas_rgba.clone())
                })
        } else {
            let (w, h) = vx.resource_pack.atlas_size;
            (w, h, vx.resource_pack.atlas_rgba.clone())
        };
        vx.voxel_atlas = Some(r.make_mesh_albedo(aw, ah, &atlas_rgba));
        // W32: promote the atlas to the global slot for the uber chunk pass.
        r.set_mesh_albedo(aw, ah, &atlas_rgba);
        vx.atlas_rgba = Some((aw, ah, atlas_rgba));
        r.upload_tile_anims(&vx.resource_pack.build_tile_anims_uniform());
        // W18.2: push the particle sprite atlas to the GPU.
        if let Some(pa) = &vx.resource_pack.particle_atlas {
            r.load_particle_atlas_rgba(pa.width, pa.height, &pa.rgba);
        }
        // Sheep + mob textures, loaded from --textures dir.
        if let Some(dir) = vx.texture_dir.as_ref() {
            let path = dir.join("wool.png");
            if path.exists()
                && let Ok(mut img) = image::open(&path).map(|i| i.to_rgba8())
            {
                for px in img.chunks_exact_mut(4) {
                    px[3] = 255;
                }
                let mesh = r.create_cube_mesh("voxel_demo_mp::sheep_body");
                let albedo = r.make_mesh_albedo(img.width(), img.height(), &img);
                r.bind_mesh_albedo_arc(mesh, albedo);
                info!(path = %path.display(), "sheep body texture bound");
                vx.sheep_mesh = Some(mesh);
            }
            // Sheep head: MC sheep.tga or .png laid out as UV atlas.
            let sheep_root = dir
                .parent()
                .unwrap_or(dir.as_path())
                .join("entity")
                .join("sheep");
            let sheep_paths = [
                sheep_root.join("sheep.tga"),
                sheep_root.join("sheep.png"),
                dir.join("sheep.tga"),
                dir.join("sheep.png"),
            ];
            let sheep_src = sheep_paths.iter().find(|p| p.exists()).cloned();
            if let Some(path) = sheep_src
                && let Ok(mut img) = image::open(&path).map(|i| i.to_rgba8())
            {
                let w = img.width();
                let h = img.height();
                for px in img.chunks_exact_mut(4) {
                    px[3] = 255;
                }
                let (verts, indices) = mob_meshes::build_sheep_head_mesh(w, h);
                let mesh = r.create_mesh("voxel_demo_mp::sheep_head", &verts, &indices);
                let albedo = r.make_mesh_albedo(w, h, &img);
                r.bind_mesh_albedo_arc(mesh, albedo);
                info!(
                    path = %path.display(),
                    texture = format!("{w}x{h}"),
                    "sheep head per-face UV mesh bound"
                );
                vx.sheep_head_mesh = Some(mesh);
            }
            // W19.2 / W46.9: load remaining mob meshes.
            let mc_entity_dir = if !vx.resource_pack.is_fallback {
                Some(vx.resource_pack.source.join("textures").join("entity"))
            } else {
                None
            };
            vx.mob_meshes =
                mob_meshes::load_non_sheep_mob_meshes(dir, mc_entity_dir.as_deref(), r);
        }
    }
    // Best-effort audio: a missing output device disables SFX.
    match infengine_audio::AudioSystem::new() {
        Ok(a) => vx.audio = Some(a),
        Err(err) => {
            warn!(error = %err, "audio init failed - SFX disabled");
        }
    }
    // Load user-supplied sound assets from --sounds dir.
    if let (Some(a), Some(dir)) = (vx.audio.as_ref(), vx.sounds_dir.as_ref()) {
        let scans: &[(&str, &str, &[&str])] = &[
            ("dig:grass", "dig", &["grass1.ogg", "grass2.ogg", "grass3.ogg", "grass4.ogg"]),
            ("dig:stone", "dig", &["stone1.ogg", "stone2.ogg", "stone3.ogg", "stone4.ogg"]),
            ("dig:sand", "dig", &["sand1.ogg", "sand2.ogg", "sand3.ogg", "sand4.ogg"]),
            ("dig:wood", "dig", &["wood1.ogg", "wood2.ogg", "wood3.ogg", "wood4.ogg"]),
            ("dig:gravel", "dig", &["gravel1.ogg", "gravel2.ogg", "gravel3.ogg", "gravel4.ogg"]),
            ("dig:glass", "dig", &["glass1.ogg", "glass2.ogg", "glass3.ogg"]),
            ("step:grass", "step", &["grass1.ogg", "grass2.ogg", "grass3.ogg", "grass4.ogg"]),
            ("step:stone", "step", &["stone1.ogg", "stone2.ogg", "stone3.ogg", "stone4.ogg"]),
            ("step:sand", "step", &["sand1.ogg", "sand2.ogg", "sand3.ogg", "sand4.ogg"]),
            ("step:wood", "step", &["wood1.ogg", "wood2.ogg", "wood3.ogg", "wood4.ogg"]),
            ("step:gravel", "step", &["gravel1.ogg", "gravel2.ogg", "gravel3.ogg", "gravel4.ogg"]),
            ("mob/sheep/say", "mob/sheep", &["say1.ogg", "say2.ogg", "say3.ogg"]),
            (
                "mob/sheep/step",
                "mob/sheep",
                &["step1.ogg", "step2.ogg", "step3.ogg", "step4.ogg", "step5.ogg"],
            ),
            (
                "splash",
                "random",
                &["splash1.ogg", "splash2.ogg", "splash3.ogg", "splash.ogg"],
            ),
            (
                "splash",
                "liquid",
                &["splash.ogg", "splash1.ogg", "splash2.ogg"],
            ),
            (
                "splash",
                "water",
                &["splash.ogg", "splash1.ogg", "splash2.ogg"],
            ),
            (
                "splash",
                "splash",
                &["splash1.ogg", "splash2.ogg", "splash3.ogg", "splash.ogg"],
            ),
            ("splash_out", "liquid", &["water.ogg", "lava.ogg"]),
            ("splash_out", "random", &["splash.ogg"]),
            ("water_flow", "liquid", &["water.ogg", "stream.ogg", "flow.ogg"]),
            ("water_flow", "ambient", &["water.ogg", "stream.ogg"]),
        ];
        for (key, subdir, files) in scans {
            let paths: Vec<std::path::PathBuf> = files
                .iter()
                .map(|f| dir.join(subdir).join(f))
                .filter(|p| p.exists())
                .collect();
            if paths.is_empty() {
                continue;
            }
            let n = vx.audio_bank.load_variants(a, key, &paths);
            if n > 0 {
                info!(key = %key, count = n, "loaded sound asset variants");
            }
        }
    }
    // W9.6: extend the AudioBank with variants from the MC pack.
    if let Some(a) = vx.audio.as_ref()
        && !vx.resource_pack.sounds.is_empty()
    {
        for (engine_key, _) in resource_pack::sounds::ENGINE_TO_MC {
            if vx.audio_bank.has(engine_key) {
                continue;
            }
            let paths = vx.resource_pack.sound_paths(engine_key);
            if paths.is_empty() {
                continue;
            }
            let n = vx.audio_bank.load_variants(a, engine_key, paths);
            if n > 0 {
                info!(
                    key = %engine_key,
                    count = n,
                    "loaded MC sound variants from resource pack",
                );
            }
        }
    }
    // W8.1: CLI servers (`--server`) enter the world immediately;
    // pure offline + client launches defer until the menu pick.
    if matches!(vx.net, NetRole::Server { .. }) {
        vx.enter_world();
    }
}
