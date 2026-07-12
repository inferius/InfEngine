//! W47.23 - update_play body relocated from main.rs.
//!
//! Movement / camera / mouse-look / WASD / jump / sneak / sprint /
//! swim float / head bob. Drains pending mouse + wheel input,
//! advances physics, updates the network broadcast pose.

#![allow(clippy::too_many_lines)]

use crate::*;
use infengine_window::FrameContext;

pub(crate) fn update_play(app: &mut Voxel, ctx: &FrameContext<'_>, dt: f32) {
        // While the inventory is open we still tick physics (so the
        // player keeps falling / settling on the ground + the server
        // sees a live character) but freeze all player-driven inputs:
        // mouse-look, WASD, jump, hotbar wheel. This matches the
        // Minecraft convention - opening the inventory in multiplayer
        // never pauses the world for anyone.
        let input_active = !app.any_panel_open();

        // W10.3: double-tap Space toggles fly in Creative. Tick
        // the detector EVERY frame so the window timer expires
        // even when the player is idle. The helper itself
        // short-circuits when `abilities.mayfly = false` so the
        // Survival path is unchanged.
        let space_now = input_active && ctx.input.is_pressed(Key::Space);
        app.tick_double_tap_fly(space_now, dt);

        if input_active {
            let dx = std::mem::take(&mut app.mouse_dx);
            let dy = std::mem::take(&mut app.mouse_dy);
            let sens = app.settings.mouse_sensitivity * 0.0035;
            app.player_yaw -= dx * sens;
            let dy_signed = if app.settings.invert_mouse_y {
                -dy
            } else {
                dy
            };
            app.player_pitch -= dy_signed * sens;
            let limit = std::f32::consts::FRAC_PI_2 - 0.05;
            app.player_pitch = app.player_pitch.clamp(-limit, limit);

            // Consume mouse wheel ticks - cycle hotbar slots (wheel up =
            // previous slot, matches Minecraft convention).
            let wheel = std::mem::take(&mut app.wheel_accum);
            let ticks = wheel.round() as i32;
            if ticks != 0 {
                let dir = if app.settings.invert_wheel { 1 } else { -1 };
                let mut slot = app.selected_slot as i32 + dir * ticks;
                slot = slot.rem_euclid(HOTBAR_SLOTS as i32);
                app.selected_slot = slot as usize;
            }
        } else {
            // Discard captured deltas so they don't snap the camera
            // when the menu closes.
            app.mouse_dx = 0.0;
            app.mouse_dy = 0.0;
            app.wheel_accum = 0.0;
        }

        let forward_horizontal =
            Vec3::new(app.player_yaw.cos(), 0.0, -app.player_yaw.sin()).normalize();
        // Right vector via the standard 90-deg horizontal rotation of
        // forward. For yaw=0 (forward = +X), right = +Z which matches
        // a player facing east strafing south.
        let right = Vec3::new(-forward_horizontal.z, 0.0, forward_horizontal.x);

        let mut move_axis: f32 = 0.0;
        let mut strafe: f32 = 0.0;
        // Movement speed multiplier: Shift = sneak (0.4x), Ctrl =
        // sprint (1.7x), no modifier = normal walk. Modifiers are
        // also gated on `input_active` so the menu doesn't sprint
        // the player while idle.
        let mut speed_mul = 1.0_f32;
        let mut sneaking = false;
        if input_active {
            if ctx.input.is_pressed(Key::W) {
                move_axis += 1.0;
            }
            if ctx.input.is_pressed(Key::S) {
                move_axis -= 1.0;
            }
            if ctx.input.is_pressed(Key::A) {
                strafe -= 1.0;
            }
            if ctx.input.is_pressed(Key::D) {
                strafe += 1.0;
            }
            if ctx.input.is_pressed(Key::ShiftLeft)
                || ctx.input.is_pressed(Key::ShiftRight)
            {
                speed_mul *= 0.4;
                sneaking = true;
            }
            if ctx.input.is_pressed(Key::ControlLeft)
                || ctx.input.is_pressed(Key::ControlRight)
            {
                // Stamina gate: sprint refuses to engage while
                // the player is below the reactivation
                // threshold so a fully-drained bar enforces a
                // brief walking penalty.
                let want_sprint = move_axis.abs() + strafe.abs() > 0.0;
                let stamina_ok = if app.sprinting {
                    app.stamina > 0.0
                } else {
                    app.stamina >= STAMINA_REACTIVATE
                };
                if want_sprint && stamina_ok {
                    speed_mul *= 1.7;
                    app.sprinting = true;
                } else {
                    app.sprinting = false;
                }
            } else {
                app.sprinting = false;
            }
        } else {
            app.sprinting = false;
        }
        // Stamina accounting: drain while sprinting, regen
        // otherwise. Clamped to [0, STAMINA_MAX] each tick.
        // W15.3: Creative / invulnerable players never drain stamina -
        // mirrors MC's Player::serverAiStep which skips the food/
        // exhaustion deduction in Creative (GameType.isCreative()).
        if app.abilities.invulnerable {
            // Snap to full so a mode-switch mid-sprint refills
            // immediately rather than waiting for the regen ramp.
            app.stamina = STAMINA_MAX;
        } else if app.sprinting {
            app.stamina = (app.stamina - STAMINA_DRAIN * dt).max(0.0);
        } else {
            app.stamina =
                (app.stamina + STAMINA_REGEN * dt).min(STAMINA_MAX);
        }
        // Smooth FOV lerp toward the per-state target. 60 deg
        // baseline, 70 deg while sprinting. The exponential lerp
        // factor `1 - exp(-rate * dt)` keeps the easing
        // framerate-independent.
        let target_fov = if app.sprinting {
            70_f32.to_radians()
        } else {
            60_f32.to_radians()
        };
        let lerp_rate = 8.0_f32;
        let alpha = 1.0 - (-lerp_rate * dt).exp();
        app.fov_y_rad += (target_fov - app.fov_y_rad) * alpha;
        // W134: riding a minecart suspends self-propelled motion -
        // W/S become cart thrust (projected onto the rail by the
        // cart tick), Shift requests a dismount, and the cart tick
        // in app_on_update pins the player each frame. Everything
        // past the motion block (ambience, breath, splash, walk
        // distance) still runs.
        let riding = app.riding_cart.is_some();
        if riding {
            let mut thrust = 0.0_f32;
            if input_active {
                if ctx.input.is_pressed(Key::W) {
                    thrust += 1.0;
                }
                if ctx.input.is_pressed(Key::S) {
                    thrust -= 1.0;
                }
                if ctx.input.is_pressed(Key::ShiftLeft)
                    || ctx.input.is_pressed(Key::ShiftRight)
                {
                    app.cart_dismount_pending = true;
                }
            }
            app.cart_input = thrust;
            app.player_vel = Vec3::ZERO;
            app.grounded = true;
            app.fall_start_y = None;
            app.sprinting = false;
        }
        let horizontal = if riding {
            Vec3::ZERO
        } else {
            (forward_horizontal * move_axis + right * strafe)
                .normalize_or_zero()
                * (PLAYER_SPEED * speed_mul)
        };
        let mut hvel_x = horizontal.x;
        let mut hvel_z = horizontal.z;
        // Sneak edge protection: a grounded sneaking player can't
        // walk off a block. We probe per-axis along the candidate
        // delta and zero the component that would land the feet
        // over thin air. Both axes blocked = no horizontal move
        // at all (lets the player creep up to the edge safely).
        if sneaking && app.grounded {
            let test_step = 0.4;
            let test_x = app.player_pos.x + hvel_x.signum() * test_step;
            if hvel_x.abs() > 1e-3 && !app.has_floor_under(test_x, app.player_pos.z) {
                hvel_x = 0.0;
            }
            let test_z = app.player_pos.z + hvel_z.signum() * test_step;
            if hvel_z.abs() > 1e-3 && !app.has_floor_under(app.player_pos.x, test_z) {
                hvel_z = 0.0;
            }
        }
        app.player_vel.x = hvel_x;
        app.player_vel.z = hvel_z;

        if riding {
            // W134: the cart carries the player - no jump, no
            // gravity (vertical motion comes from the rail path).
        } else if app.fly_mode {
            // Free-flight: Space = up, Shift = down. Velocity is
            // replaced (not integrated) so the player stops the
            // instant the key is released, the way Minecraft's
            // creative fly works. Ctrl held while flying doubles
            // the vertical thrust + horizontal speed multiplier
            // for fast traversal.
            let boost = input_active
                && (ctx.input.is_pressed(Key::ControlLeft)
                    || ctx.input.is_pressed(Key::ControlRight));
            let up = input_active && ctx.input.is_pressed(Key::Space);
            let down = input_active
                && (ctx.input.is_pressed(Key::ShiftLeft)
                    || ctx.input.is_pressed(Key::ShiftRight));
            let thrust = if boost { 16.0 } else { 8.0 };
            let v = if up && !down {
                thrust
            } else if down && !up {
                -thrust
            } else {
                0.0
            };
            app.player_vel.y = v;
            if boost {
                app.player_vel.x *= 1.7;
                app.player_vel.z *= 1.7;
            }
            app.grounded = false;
        } else {
            if input_active && ctx.input.is_pressed(Key::Space) && app.grounded {
                app.player_vel.y = JUMP_VELOCITY;
                app.grounded = false;
                // Jumping costs a flat 5 stamina so spamming the
                // bunny-hop drains the pool. Floor at 0.
                // W15.3: Creative / invulnerable players never pay
                // the jump cost (mirrors MC's exhaustion skip).
                if !app.abilities.invulnerable {
                    app.stamina = (app.stamina - 5.0).max(0.0);
                }
                // Sprint-jump boost: add forward kick so a moving
                // jump covers more ground than a stand-still jump.
                if app.sprinting {
                    let fwd_h = Vec3::new(
                        app.player_yaw.cos(),
                        0.0,
                        -app.player_yaw.sin(),
                    )
                    .normalize_or_zero();
                    app.player_vel.x += fwd_h.x * 3.0;
                    app.player_vel.z += fwd_h.z * 3.0;
                }
            }
            app.player_vel.y += GRAVITY * dt;
        }

        // Footstep audio: fire a short click every ~0.4 s of
        // grounded horizontal motion. Sprint shortens the cadence
        // proportionally so it stays in sync with the leg.
        let speed = (app.player_vel.x * app.player_vel.x
            + app.player_vel.z * app.player_vel.z)
            .sqrt();
        // Advance the head-bob phase proportionally to horizontal
        // speed while grounded. Off the ground we let it ease
        // back to a quiet rest so a jump doesn't freeze the bob
        // mid-cycle.
        if app.grounded && speed > 0.5 {
            app.head_bob_phase += dt * (speed * 1.6 + 1.0);
        } else {
            // Bleed phase back to zero so the camera settles flat.
            app.head_bob_phase = (app.head_bob_phase
                - dt * std::f32::consts::PI)
                .max(0.0);
        }
        if input_active && app.grounded && speed > 0.5 {
            app.step_cooldown -= dt * (speed / PLAYER_SPEED);
            if app.step_cooldown <= 0.0 {
                const STEP_PERIOD: f32 = 0.42;
                app.step_cooldown = STEP_PERIOD;
                // Sample the voxel just below the player's feet so
                // the footstep sound matches the surface they're
                // actually standing on.
                let foot = [
                    app.player_pos.x.floor() as i32,
                    (app.player_pos.y - PLAYER_HEIGHT - 0.05).floor() as i32,
                    app.player_pos.z.floor() as i32,
                ];
                let under = app.world.get(foot);
                let mat = block_material(under);
                let key = format!("step:{mat}");
                let pos = [app.player_pos.x, app.player_pos.y, app.player_pos.z];
                // W200.1: NO synth fallback for footsteps. The old
                // play_swish fallback produced a synthetic CLICK every
                // 0.42s of walking whenever the step:<mat> bank entry
                // didn't load - the persistent "clicking" while moving.
                // MC plays nothing when a sample is missing; the bank
                // miss is logged once per key by sfx_at (W200.1) so a
                // silent material is diagnosable instead of audible.
                app.sfx_at(&key, pos, |_, _| {});
                // Sprint dust: a small puff per step matching the
                // surface colour. Walking sneak / normal speed
                // skips this so dust is a sprint-only flourish.
                if app.sprinting {
                    let color = app
                        .registry
                        .get(under)
                        .map(|d| d.color)
                        .unwrap_or([0.6, 0.55, 0.45]);
                    let foot_voxel = foot;
                    // Tiny burst that fades fast; reuses the
                    // existing particle system rather than adding
                    // a dedicated sprint emitter.
                    let base = [
                        app.player_pos.x,
                        app.player_pos.y - PLAYER_HEIGHT,
                        app.player_pos.z,
                    ];
                    let mut rng: u32 = 0xD15D_15D1
                        ^ ((foot_voxel[0] as u32).wrapping_mul(7))
                        ^ ((foot_voxel[2] as u32).wrapping_mul(13))
                        ^ (app.net_tick as u32);
                    let mut rand01 = || -> f32 {
                        rng ^= rng << 13;
                        rng ^= rng >> 17;
                        rng ^= rng << 5;
                        (rng as f32 / u32::MAX as f32).clamp(0.0, 1.0)
                    };
                    for _ in 0..4 {
                        let vx = (rand01() * 2.0 - 1.0) * 1.2;
                        let vy = rand01() * 0.6 + 0.2;
                        let vz = (rand01() * 2.0 - 1.0) * 1.2;
                        app.particles.particles.push(
                            infengine_particles::Particle {
                                position: base,
                                velocity: [vx, vy, vz],
                                color: [color[0], color[1], color[2], 0.85],
                                size: 0.08,
                                age: 0.0,
                                lifetime: 0.30,
                                gravity_scale: 1.0,
                            },
                        );
                    }
                }
            }
        } else {
            // Reset cooldown so a step doesn't fire instantly the
            // moment we re-enter motion (keeps cadence natural).
            app.step_cooldown = 0.0;
        }

        // Distance-walked tracker. Uses XZ deltas only so
        // fall/jump motion doesn't inflate the counter.
        let cur_xz = (app.player_pos.x, app.player_pos.z);
        if let Some(prev) = app.last_walk_xz {
            let dx = cur_xz.0 - prev.0;
            let dz = cur_xz.1 - prev.1;
            let step = (dx * dx + dz * dz).sqrt();
            if step.is_finite() && step < 10.0 {
                app.distance_walked += step;
            }
        }
        app.last_walk_xz = Some(cur_xz);
        // W123: rain ambience loop. MC plays weather.rain segments
        // back-to-back while it rains; our bank variants are short
        // clips, so refire on a ~0.45 s cadence (clips overlap a
        // touch, which reads as a continuous shower). No synth
        // fallback - silence is better than a noise bed.
        if app.weather == 1 {
            app.rain_sfx_accum -= dt;
            if app.rain_sfx_accum <= 0.0 {
                app.rain_sfx_accum = 0.45;
                let pos = [app.player_pos.x, app.player_pos.y, app.player_pos.z];
                app.sfx_at("ambient/rain", pos, |_, _| {});
            }
        } else {
            app.rain_sfx_accum = 0.0;
        }
        // Lightning state machine. Only fires while raining.
        // `next_lightning_at` counts down; on zero we trigger a
        // 0.18s flash + a thud and seed the next interval at a
        // randomized 10..=30 s window so strikes feel sporadic.
        if app.weather == 1 {
            app.next_lightning_at -= dt;
            if app.next_lightning_at <= 0.0 {
                app.lightning_flash = 0.18;
                let mut rng: u32 = ((app.net_tick as u32)
                    .wrapping_mul(0xC2B2_AE35))
                    ^ 0xDEAD_BEEF;
                rng ^= rng << 13;
                rng ^= rng >> 17;
                rng ^= rng << 5;
                let bias = (rng as f32 / u32::MAX as f32).clamp(0.0, 1.0);
                app.next_lightning_at = 10.0 + bias * 20.0;
                // Distant thunder + chat note so peers know it
                // wasn't a graphics glitch.
                let pos = [
                    app.player_pos.x,
                    app.player_pos.y,
                    app.player_pos.z,
                ];
                app.sfx_at("thunder", pos, |a, g| {
                    let _ = a.play_thud(55.0, 0.7 * g);
                });
                app.chat_log.push((
                    std::time::Instant::now(),
                    "* lightning flashes overhead".to_string(),
                ));
            }
        }
        if app.lightning_flash > 0.0 {
            app.lightning_flash = (app.lightning_flash - dt).max(0.0);
        }
        // Precipitation spawner. weather=1 = rain (fast falling
        // blue streaks); weather=2 = snow (slow drifting white
        // flakes). Both stay purely visual. Skips spawning while
        // the player has solid ceiling overhead so being indoors
        // doesn't fill the room with rain.
        // Snow accumulation: while snowing, periodically blanket
        // a grass / dirt / sand voxel within ~12 blocks of the
        // player with a snow layer. Host-only so the edit goes
        // through the authoritative replication path. Limits to
        // one cell per fire so the world doesn't snap to a
        // uniform white sheet instantly.
        // Chicken passive laying (Q.3): every ~12 s the host
        // would drop one feather into its own inventory while a
        // live chicken existed. The loop ran unconditionally,
        // which is the "feathers spawn periodically in inventory
        // mid-air" bug the user reported. Now gated behind the
        // `/debug feathers on` toggle (defaults off); the
        // canonical way to get feathers is to kill a chicken (see
        // the mob death drop in `mob_ai`).
        let host_role = matches!(app.net, NetRole::Server { .. } | NetRole::Offline);
        let chicken_alive = app.mobs.iter().any(|m| m.kind == KIND_CHICKEN);
        if mob_meshes::tick_chicken_feather_lay(
            app.debug_chicken_feathers,
            host_role,
            chicken_alive,
            &mut app.chicken_lay_accum,
            dt,
        ) {
            app.pickup(BlockId(19));
        }
        if app.weather == 2
            && matches!(app.net, NetRole::Server { .. } | NetRole::Offline)
        {
            app.snow_accum += dt;
            if app.snow_accum >= app.snow_rate.max(0.05) {
                app.snow_accum = 0.0;
                let r = 12i32;
                let mut rng = (app.net_tick as u32)
                    .wrapping_mul(0x9E37_79B1);
                let mut rand = || -> u32 {
                    rng ^= rng << 13;
                    rng ^= rng >> 17;
                    rng ^= rng << 5;
                    rng
                };
                let dx = (rand() % (2 * r as u32 + 1)) as i32 - r;
                let dz = (rand() % (2 * r as u32 + 1)) as i32 - r;
                let cx = app.player_pos.x.floor() as i32 + dx;
                let cz = app.player_pos.z.floor() as i32 + dz;
                let ceiling = crate::WORLD_MAX_Y;
                if let Some(sy) =
                    app.surface_y_below(cx, cz, ceiling)
                {
                    let surface = app.world.get([cx, sy, cz]).0;
                    // Walk upward looking for either an existing
                    // snow voxel to thicken or the first air cell
                    // to paint a fresh layer on. Cap stacking at
                    // 4 voxels per column so deep drifts don't
                    // pile up into the sky.
                    let mut target_y = sy + 1;
                    let mut stacked = 0u32;
                    while stacked < 16 {
                        let here = app
                            .world
                            .get([cx, target_y, cz])
                            .0;
                        if here == 0 {
                            break;
                        }
                        if here == 15 {
                            let snow_voxel = [cx, target_y, cz];
                            let bumped = if let Some(layers) = app
                                .snow_positions
                                .get_mut(&snow_voxel)
                                && *layers < 8
                            {
                                *layers += 1;
                                Some(*layers)
                            } else {
                                None
                            };
                            if let Some(new_layers) = bumped {
                                // Broadcast layer increment so
                                // MP peers grow the slab too.
                                let cfg =
                                    bincode::config::standard();
                                if let NetRole::Server {
                                    server, ..
                                } = &mut app.net
                                    && let Ok(buf) =
                                        bincode::serde::encode_to_vec(
                                            (snow_voxel, new_layers),
                                            cfg,
                                        )
                                {
                                    server.broadcast_bytes(
                                        "vx.snow", buf,
                                    );
                                }
                                target_y = i32::MIN;
                                break;
                            }
                            target_y += 1;
                            stacked += 1;
                            continue;
                        }
                        target_y = i32::MIN;
                        break;
                    }
                    let needs_solid_base = matches!(surface, 2 | 3 | 4 | 8 | 15);
                    if target_y != i32::MIN && needs_solid_base {
                        let voxel = [cx, target_y, cz];
                        let edit = EditMsg {
                            x: voxel[0],
                            y: voxel[1],
                            z: voxel[2],
                            block: 15,
                        };
                        app.world_set_tracked(voxel, BlockId(15));
                        let coords = Voxel::affected_chunks(voxel);
                        app.rebuild_chunks(&coords);
                        let cfg = bincode::config::standard();
                        if let NetRole::Server { server, .. } = &mut app.net
                        {
                            app.edit_history.push(edit);
                            if let Ok(buf) =
                                bincode::serde::encode_to_vec(edit, cfg)
                            {
                                server.broadcast_bytes("vx.edit", buf);
                            }
                        }
                    }
                }
            }
        }
        let mut sheltered = false;
        if app.weather == 1 || app.weather == 2 {
            let px = app.player_pos.x.floor() as i32;
            let pz = app.player_pos.z.floor() as i32;
            let py = app.player_pos.y.floor() as i32;
            for dy in 1..15 {
                if app.world.get([px, py + dy, pz]) != BlockId::AIR {
                    sheltered = true;
                    break;
                }
            }
        }
        if (app.weather == 1 || app.weather == 2) && !sheltered {
            const FALL_PER_FRAME: u32 = 20;
            let mut rng: u32 = ((app.net_tick as u32)
                .wrapping_mul(0x9E37_79B1))
                ^ 0xCAFE_F00D;
            let mut rand01 = || -> f32 {
                rng ^= rng << 13;
                rng ^= rng >> 17;
                rng ^= rng << 5;
                (rng as f32 / u32::MAX as f32).clamp(0.0, 1.0)
            };
            let base = [
                app.player_pos.x,
                app.player_pos.y + 14.0,
                app.player_pos.z,
            ];
            for _ in 0..FALL_PER_FRAME {
                let dx = (rand01() * 2.0 - 1.0) * 22.0;
                let dz = (rand01() * 2.0 - 1.0) * 22.0;
                let (vel, color, size, life) = if app.weather == 1 {
                    (
                        [0.0_f32, -16.0, 0.0_f32],
                        [0.55, 0.65, 0.85, 0.65],
                        0.05,
                        1.2,
                    )
                } else {
                    // Snow: small lateral drift + slow descent.
                    let drift_x = (rand01() * 2.0 - 1.0) * 0.6;
                    let drift_z = (rand01() * 2.0 - 1.0) * 0.6;
                    (
                        [drift_x, -1.6, drift_z],
                        [0.92, 0.95, 1.00, 0.85],
                        0.08,
                        7.0,
                    )
                };
                app.particles.particles.push(
                    infengine_particles::Particle {
                        position: [base[0] + dx, base[1], base[2] + dz],
                        velocity: vel,
                        color,
                        size,
                        age: 0.0,
                        lifetime: life,
                        gravity_scale: 1.0,
                    },
                );
            }
        }
        // Low-HP heartbeat thump. Interval shrinks from ~1.2 s
        // at HP=5 down to ~0.5 s at HP=1. Above HP=5 we leave the
        // accumulator at 0 so re-entering the danger zone fires
        // the first beat promptly.
        if app.player_health > 0.0 && app.player_health < 5.0 {
            app.heartbeat_accum += dt;
            let interval = (0.5 + 0.2 * app.player_health).clamp(0.5, 1.4);
            if app.heartbeat_accum >= interval {
                app.heartbeat_accum = 0.0;
                let pos = [app.player_pos.x, app.player_pos.y, app.player_pos.z];
                let vol_scale = (1.0 - (app.player_health / 5.0)).clamp(0.2, 1.0);
                app.sfx_at("heartbeat", pos, move |a, _g| {
                    let _ = a.play_thud(70.0, 0.28 * vol_scale);
                });
            }
        } else {
            app.heartbeat_accum = 0.0;
        }

        // Swim mechanics. While the player's AABB overlaps a
        // water voxel: cap fall speed, scale horizontal velocity
        // (drag), and let Space push them upward (buoyancy +
        // swim-up). Skipped in fly mode so /fly through water
        // still glides freely.
        let liquid_under = if app.fly_mode {
            None
        } else {
            submerged_liquid(&app.world, app.player_pos)
        };
        if let Some(liquid) = liquid_under {
            // Every swim parameter comes from the `Liquid` row -
            // water reads light + smooth, lava drags + barely
            // floats. Adding a new liquid means filling these
            // fields; no fluid-specific code in the swim loop.
            if app.player_vel.y < liquid.swim_fall_cap {
                app.player_vel.y = liquid.swim_fall_cap;
            }
            app.player_vel.x *= liquid.swim_horiz_drag;
            app.player_vel.z *= liquid.swim_horiz_drag;
            // Current: flowing cells push the player downstream
            // at the liquid's `current_strength`. Source cells
            // (level 0 with no lower-level neighbour) return a
            // zero vector so a pond doesn't drag the player.
            let foot = [
                app.player_pos.x.floor() as i32,
                (app.player_pos.y - PLAYER_HEIGHT * 0.5).floor() as i32,
                app.player_pos.z.floor() as i32,
            ];
            let foot_block = app.world.get(foot);
            let mut foot_in_flow = false;
            if foot_block.0 == liquid.block_id
                && liquid.current_strength > 0.0
            {
                let dir = app.liquid_flow_dir(foot, liquid.block_id);
                if dir[0] != 0.0 || dir[1] != 0.0 {
                    let push = liquid.current_strength * dt * 6.0;
                    app.player_vel.x += dir[0] * push;
                    app.player_vel.z += dir[1] * push;
                    foot_in_flow = true;
                }
            }
            // Flowing-water ambience: when standing in a flow
            // voxel, periodically play a soft running-water
            // sting near the foot position. Source ponds stay
            // silent (no flow direction).
            app.flow_sfx_accum -= dt;
            if foot_in_flow && app.flow_sfx_accum <= 0.0 {
                app.flow_sfx_accum = 0.55;
                let center = [
                    foot[0] as f32 + 0.5,
                    foot[1] as f32 + 0.5,
                    foot[2] as f32 + 0.5,
                ];
                app.sfx_at("water_flow", center, |a, g| {
                    let _ = a.play_swish(0.12 * g);
                });
            }
            if input_active && ctx.input.is_pressed(Key::Space) {
                app.player_vel.y =
                    app.player_vel.y.max(liquid.swim_jump_vel);
            } else {
                app.player_vel.y += liquid.swim_buoyancy * dt;
                if app.player_vel.y > 1.0 {
                    app.player_vel.y = 1.0;
                }
            }
            // Reset fall-damage tracker so falling into the
            // liquid doesn't punish on the next grounded
            // transition.
            app.fall_start_y = None;
            // Submerged damage tick (lava burns, future acid).
            if liquid.submerged_damage_per_sec > 0.0 {
                app.apply_player_damage_kind(
                    liquid.submerged_damage_per_sec * dt,
                    DamageKind::Burn,
                );
            }
        }
        // Breath: only the eye-submerged check drains air -
        // mirrors MC's `getEyeInFluid() == WATER`. Wading
        // ankle- or waist-deep doesn't suffocate the player;
        // only fully-under does. Refill 4x as fast on the
        // surface so a quick breath buys plenty of dive time.
        // Independent of `liquid_under` so the surfacing
        // transition fires even when feet are still wet.
        //
        // W15.3: Creative / invulnerable players never lose air.
        // Mirrors MC's LocalPlayer::aiStep which skips the
        // `decreaseAirSupply` path when `isCreative()`. Snap to
        // AIR_MAX_SEC so a mode switch mid-dive refills instantly.
        if app.abilities.invulnerable {
            app.player_air = AIR_MAX_SEC;
        } else {
            let eye_liquid = if app.fly_mode {
                None
            } else {
                submerged_liquid_eye(&app.world, app.player_pos)
            };
            if let Some(eye_liq) = eye_liquid
                && eye_liq.drains_air
            {
                app.player_air = (app.player_air - dt).max(0.0);
                if app.player_air <= 0.0 {
                    app.apply_player_damage_kind(
                        DROWN_DAMAGE_PER_SEC * dt,
                        DamageKind::Drown,
                    );
                }
            } else {
                app.player_air =
                    (app.player_air + dt * 4.0).min(AIR_MAX_SEC);
            }
        }
        let in_water = liquid_under.is_some();
        // Splash on the air -> water transition: short droplet
        // burst + click SFX. Quieter cue going the other way.
        if in_water != app.was_in_water {
            let base = [
                app.player_pos.x,
                app.player_pos.y - PLAYER_HEIGHT * 0.5,
                app.player_pos.z,
            ];
            if in_water {
                // Splash synth: a low thud (mass hitting water)
                // layered with a high noise swish (the spray).
                // Much more water-like than a single click.
                app.sfx_at("splash", base, |a, g| {
                    let _ = a.play_thud(140.0, 0.20 * g);
                    let _ = a.play_swish(0.18 * g);
                });
                let mut rng: u32 = ((app.net_tick as u32)
                    .wrapping_mul(0xBEEF_0BAD))
                    ^ 0x5EA_5EA;
                let mut rand01 = || -> f32 {
                    rng ^= rng << 13;
                    rng ^= rng >> 17;
                    rng ^= rng << 5;
                    (rng as f32 / u32::MAX as f32).clamp(0.0, 1.0)
                };
                for _ in 0..18 {
                    let vx = (rand01() * 2.0 - 1.0) * 2.4;
                    let vy = rand01() * 2.6 + 1.5;
                    let vz = (rand01() * 2.0 - 1.0) * 2.4;
                    app.particles.particles.push(
                        infengine_particles::Particle {
                            position: base,
                            velocity: [vx, vy, vz],
                            color: [0.65, 0.80, 0.95, 0.95],
                            size: 0.07,
                            age: 0.0,
                            lifetime: 0.45,
                            gravity_scale: 1.0,
                        },
                    );
                }
            } else {
                app.sfx_at("splash_out", base, |a, g| {
                    let _ = a.play_swish(0.14 * g);
                });
            }
        }
        app.was_in_water = in_water;
        // Axis-resolved voxel AABB collision.
        let was_grounded = app.grounded;
        let delta = app.player_vel * dt;
        let (new_pos, hit_floor, hit_ceiling) = player::sweep_aabb_with_doors(
            &app.world,
            &app.snow_positions,
            &app.door_open,
            app.player_pos,
            delta,
        );
        app.player_pos = new_pos;
        if hit_floor {
            app.player_vel.y = 0.0;
            app.grounded = true;
        } else {
            app.grounded = false;
        }
        if hit_ceiling {
            app.player_vel.y = 0.0;
        }
        // Fall-damage tracking. Note the world-Y the moment we
        // first leave the ground, then on the landing transition
        // compute the drop height + apply damage.
        if was_grounded && !app.grounded {
            app.fall_start_y = Some(app.player_pos.y);
        }
        if !was_grounded
            && app.grounded
            && let Some(start_y) = app.fall_start_y.take()
        {
            let drop = (start_y - app.player_pos.y).max(0.0);
            // Fly mode disables fall damage so toggling /fly off
            // mid-air doesn't immediately murder the player.
            if drop > SAFE_FALL && !app.fly_mode {
                let landing = [
                    app.player_pos.x.floor() as i32,
                    (app.player_pos.y - PLAYER_HEIGHT - 0.05).floor() as i32,
                    app.player_pos.z.floor() as i32,
                ];
                // Landing on snow halves the impact - snow
                // cushion mechanic.
                let on_snow = app.world.get(landing).0 == 15;
                let mut dmg = drop - SAFE_FALL;
                if on_snow {
                    dmg *= 0.5;
                }
                app.apply_player_damage(dmg);
                app.spawn_break_particles(landing);
            }
        }
        // Cactus contact damage. Check the 8 voxels around the
        // player body for cactus id 27; if any overlap, apply 1
        // HP every 0.6 s.
        let cactus_touch = {
            let pos = app.player_pos;
            let foot_y = (pos.y - PLAYER_HEIGHT - 0.05).floor() as i32;
            let mid_y = (pos.y - PLAYER_HEIGHT * 0.5).floor() as i32;
            let px = pos.x.floor() as i32;
            let pz = pos.z.floor() as i32;
            (-1..=1).any(|dx| {
                (-1..=1).any(|dz| {
                    if dx == 0 && dz == 0 {
                        return false;
                    }
                    let v_lo = [px + dx, foot_y, pz + dz];
                    let v_hi = [px + dx, mid_y, pz + dz];
                    app.world.get(v_lo).0 == 27 || app.world.get(v_hi).0 == 27
                })
            })
        };
        if cactus_touch {
            app.cactus_damage_accum += dt;
            if app.cactus_damage_accum >= 0.6 {
                app.cactus_damage_accum = 0.0;
                app.apply_player_damage(1.0);
            }
        } else {
            app.cactus_damage_accum = 0.0;
        }
        // Hurt-flash decay.
        if app.hurt_flash > 0.0 {
            app.hurt_flash = (app.hurt_flash - dt).max(0.0);
        }
        if app.invuln_remaining > 0.0 {
            app.invuln_remaining = (app.invuln_remaining - dt).max(0.0);
        }
        // W10.4: Creative players' HP snaps back to max
        // immediately (no regen delay) when below. Catches the
        // "mode switched mid-combat" case where a Survival player
        // dropped to 4 HP, then ran /gamemode creative.
        if app.abilities.invulnerable
            && app.player_health > 0.0
            && app.player_health < PLAYER_MAX_HEALTH
        {
            app.player_health = PLAYER_MAX_HEALTH;
            app.regen_accum = 0.0;
        }
        // HP regen: tick the grace timer; once the player has gone
        // `REGEN_DELAY` seconds without damage, drip HP back in at
        // `REGEN_RATE` HP/s until full.
        if app.player_health > 0.0 && app.player_health < PLAYER_MAX_HEALTH {
            app.regen_accum += dt;
            if app.regen_accum > REGEN_DELAY {
                let gain = REGEN_RATE * dt;
                app.player_health =
                    (app.player_health + gain).min(PLAYER_MAX_HEALTH);
            }
        }

        // W10.4: Creative players take continuous Void damage below
        // the world surface so /gamemode creative isn't a free pass
        // against the void. `DamageKind::Void` is the only kind that
        // bypasses `abilities.invulnerable`. Survival players keep
        // using the legacy hard-respawn path below.
        if app.player_pos.y < 0.0
            && app.abilities.invulnerable
            && !matches!(app.net, NetRole::Client { .. })
        {
            // 4 HP/s = drains a full bar in ~5 s, fast enough to
            // matter but slow enough that a single rebound off a
            // platform doesn't kill outright.
            player::apply_damage_kind(app, 4.0 * dt, DamageKind::Void);
        }
        if app.player_pos.y < -20.0 {
            // Fell through the world - respawn above origin. Acts
            // as a soft "death": storage rows wipe so falling has
            // real cost, hotbar is kept so the player isn't reduced
            // to bare hands. Locally we always wipe (optimistic);
            // in MP client mode we also fire a `vx.fellout` so the
            // server's authoritative inventory mirrors the penalty.
            tracing::warn!(
                pos = ?[app.player_pos.x, app.player_pos.y, app.player_pos.z],
                "player fell through the world - respawning"
            );
            app.player_pos = app.safe_spawn();
            app.player_vel = Vec3::ZERO;
            let mut dropped = 0usize;
            for slot in app.inventory[HOTBAR_SLOTS..].iter_mut() {
                if slot.take().is_some() {
                    dropped += 1;
                }
            }
            for slot in app.craft_grid.iter_mut() {
                if slot.take().is_some() {
                    dropped += 1;
                }
            }
            if dropped > 0 {
                tracing::info!(dropped, "fall-out penalty: cleared storage stacks");
                app.spawn_break_particles([0, 32, 0]);
            }
            if let NetRole::Client { client, .. } = &app.net {
                let _ = client.send_bytes("vx.fellout", Vec::new());
            }
        }
    }
