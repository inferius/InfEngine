//! W47.14 - mining tick submodule (split from W47.13 monolith).
//!
//! Contains `tick_mining` (W47.13j), the LMB-held damage tick that
//! starts / restarts a `MiningState`, advances its timer via
//! `mining::step_mining_session`, plays the periodic crack SFX +
//! dust puff, and returns `Some(voxel)` when the session completes
//! so the caller can route it through the break pipeline.

/// W47.13j: drive one frame of the LMB-held mining damage path.
///
/// `lmb_active` is the gate the caller derived from
/// `lmb_held && !any_panel_open && combat-target-clear && may_build`.
/// `dt` is the frame delta.
///
/// Returns `Some(voxel)` when the session completes this frame -
/// the caller routes that into `apply_block_break`. Returns `None`
/// otherwise (no LMB / aim away / mid-session).
pub(crate) fn tick_mining(vx: &mut crate::Voxel, lmb_active: bool, dt: f32) -> Option<[i32; 3]> {
    use crate::mining::{self, MiningState};
    use crate::{BlockId, block_material, submerged_liquid_eye};
    let mut mine_break: Option<[i32; 3]> = None;
    if lmb_active && let Some(hit) = vx.mine_target {
        let block = vx.world.get(hit.voxel);
        if block != BlockId::AIR {
            if vx.abilities.instabuild {
                if vx.lmb_just_pressed {
                    vx.lmb_just_pressed = false;
                    vx.mining = None;
                    mine_break = Some(hit.voxel);
                }
            } else {
                // Restart the session if the target voxel changed or
                // the block was replaced by another player.
                let restart = match vx.mining {
                    Some(s) => s.voxel != hit.voxel || s.block != block,
                    None => true,
                };
                if restart {
                    vx.mining = Some(MiningState {
                        voxel: hit.voxel,
                        progress: 0.0,
                        // W473: MC progress units - done at 1.0.
                        total: 1.0,
                        block,
                        fx_cooldown: 0.0,
                    });
                }
                if let Some(mut m) = vx.mining.take() {
                    // W473: getDestroyProgress port. The rate is
                    // re-evaluated every frame like MC's per-tick
                    // check, so tool swaps keep accumulated
                    // progress but change speed, and the water /
                    // airborne penalties apply mid-session. MC
                    // only slows EYES-IN-WATER mining (the
                    // SUBMERGED_MINING_SPEED attribute is scoped
                    // to FluidTags.WATER) - lava does not slow it.
                    let held = vx
                        .inventory
                        .get(vx.selected_slot)
                        .and_then(|s| s.as_ref())
                        .map(|st| st.id)
                        .unwrap_or(0);
                    let eye_in_water =
                        submerged_liquid_eye(&vx.world, vx.player_pos)
                            .is_some_and(|l| {
                                l.block_id == crate::block::WATER_ID
                            });
                    let rate =
                        crate::block::mining_math::destroy_progress_per_second(
                            held,
                            block.0,
                            eye_in_water,
                            vx.grounded,
                        );
                    let mining::MiningStep { fire_fx, done } =
                        mining::step_mining_session(&mut m, dt, rate);
                    let voxel_center = [
                        m.voxel[0] as f32 + 0.5,
                        m.voxel[1] as f32 + 0.5,
                        m.voxel[2] as f32 + 0.5,
                    ];
                    let key = format!("dig:{}", block_material(block));
                    let freq = 220.0 + (block.0 as f32) * 18.0;
                    let total = m.total;
                    let progress = m.progress;
                    if fire_fx {
                        vx.sfx_at(&key, voxel_center, move |a, g| {
                            let _ = a.play_thud(
                                freq,
                                0.10 * g * (0.5 + 0.5 * progress / total),
                            );
                        });
                        let color = vx
                            .registry
                            .get(block)
                            .map(|d| d.color)
                            .unwrap_or([0.7, 0.7, 0.7]);
                        vx.spawn_break_particles_tinted(m.voxel, color);
                    }
                    if done {
                        // W175/W471: a finished dig wears the held
                        // tool by its MC `Tool.damage_per_block`
                        // (diggers + shears 1, swords 2, everything
                        // else 0). Source gate (`Item.mineBlock`):
                        // no wear when the block's destroy speed is
                        // 0 - W473: read from the real hardness
                        // table (instabreak blocks cost 0 wear;
                        // liquid scoops stay exempt).
                        if crate::block::mining_math::mine_wear_applies(
                            m.block.0,
                        ) {
                            let wear = vx
                                .inventory
                                .get(vx.selected_slot)
                                .and_then(|s| s.as_ref())
                                .map(|st| {
                                    crate::item::durability::mine_wear_amount(
                                        st.id,
                                    )
                                })
                                .unwrap_or(0);
                            vx.wear_selected_stack(wear);
                        }
                        mine_break = Some(m.voxel);
                    } else {
                        vx.mining = Some(m);
                    }
                }
            }
        } else {
            vx.mining = None;
        }
    } else {
        vx.mining = None;
    }
    mine_break
}
