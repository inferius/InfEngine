//! W134 - minecarts: rail-riding cart entities.
//!
//! MC mirror: `AbstractMinecart` movement, MVP cut. A cart is bound
//! to a rail CELL plus a parameter `t` along that cell's path; every
//! rail shape is one straight segment between two edge midpoints
//! (curves run the diagonal - exactly the vanilla cart corner-cut).
//! Slopes accelerate downhill (MC's 0.0078 blocks/tick^2), friction
//! bleeds speed (0.997/tick), powered rails boost - or brake hard
//! when unpowered - and a rider's W/S thrusts along the rider's look
//! direction. Carts live in `Voxel::carts`; the offline/host tick
//! simulates them and streams `vx.cart` snapshots to clients, which
//! only render. Riding pins the player eye to the cart each frame
//! (see the `riding` gate in `update_play`).
//!
//! Follow-ups (W134 commit notes): client-side place/mount in MP,
//! save/load persistence, detector-rail activation, cart-vs-cart
//! collision, the rolling-sound loop, mobs as passengers.

use glam::Vec3;
use infengine_voxel::World;

use crate::block::rail::{
    self, SHAPE_ASC_E, SHAPE_ASC_N, SHAPE_ASC_S, SHAPE_ASC_W, SHAPE_EW,
    SHAPE_NE, SHAPE_NW, SHAPE_SE, SHAPE_SW,
};

/// Item id of the minecart (item-only - `places: None`; RMB on a
/// rail routes through `RmbAction::PlaceMinecart` instead).
pub(crate) const MINECART_ITEM_ID: u16 = 657;

/// MC rail speed cap (8 blocks/s).
pub(crate) const CART_MAX_SPEED: f32 = 8.0;
/// Slope acceleration - MC's 0.0078 blocks/tick^2 at 20 tps.
const SLOPE_G: f32 = 3.1;
/// MC per-tick friction on a coasting cart.
const FRICTION_PER_TICK: f32 = 0.997;
/// Powered-rail boost (m/s^2).
const POWERED_ACCEL: f32 = 7.0;
/// Rider W/S thrust (m/s^2).
const RIDE_ACCEL: f32 = 5.0;
/// Rail plate height - the cart bottom rides on the rail top.
const RAIL_H: f32 = 1.0 / 16.0;
/// Rider eye height above the cart origin (seated).
const RIDER_EYE: f32 = 1.30;

#[derive(Debug, Clone)]
pub(crate) struct Minecart {
    pub(crate) id: u32,
    /// Rail cell the cart is currently bound to.
    pub(crate) cell: [i32; 3],
    /// Param along the cell's path, 0..1 (endpoint A -> B).
    pub(crate) t: f32,
    /// Signed speed (m/s) along +t.
    pub(crate) vel: f32,
    /// World position of the cart origin (bottom center) - derived
    /// from `cell` + `t` after each tick.
    pub(crate) pos: Vec3,
    pub(crate) yaw: f32,
}

/// Path endpoints (cell-local `[x, y, z]` in 0..1) for a rail shape.
/// Ascending paths raise the high end one full block; curves join
/// their two open edge midpoints with a straight diagonal.
pub(crate) fn shape_path(shape: u8) -> ([f32; 3], [f32; 3]) {
    let h = RAIL_H;
    match shape & 0x7F {
        SHAPE_EW => ([0.0, h, 0.5], [1.0, h, 0.5]),
        SHAPE_ASC_E => ([0.0, h, 0.5], [1.0, h + 1.0, 0.5]),
        SHAPE_ASC_W => ([0.0, h + 1.0, 0.5], [1.0, h, 0.5]),
        SHAPE_ASC_N => ([0.5, h + 1.0, 0.0], [0.5, h, 1.0]),
        SHAPE_ASC_S => ([0.5, h, 0.0], [0.5, h + 1.0, 1.0]),
        SHAPE_SE => ([1.0, h, 0.5], [0.5, h, 1.0]),
        SHAPE_SW => ([0.5, h, 1.0], [0.0, h, 0.5]),
        SHAPE_NW => ([0.0, h, 0.5], [0.5, h, 0.0]),
        SHAPE_NE => ([0.5, h, 0.0], [1.0, h, 0.5]),
        _ => ([0.5, h, 0.0], [0.5, h, 1.0]), // NS + fallback
    }
}

/// Rail shape at `pos`; `None` when the cell holds no rail. W143:
/// stored block state first (what the mesher renders), with the
/// stateless derivation as the legacy fallback - so the cart always
/// follows exactly what the player sees.
pub(crate) fn rail_shape_at(
    world: &World,
    shapes: &std::collections::HashMap<[i32; 3], u8>,
    pos: [i32; 3],
) -> Option<u8> {
    let id = world.get(pos).0;
    if !rail::is_rail_block(id) {
        return None;
    }
    Some(match shapes.get(&pos) {
        Some(s) => *s & 0x7F,
        None => {
            let rail_at = |p: [i32; 3]| rail::is_rail_block(world.get(p).0);
            rail::compute_shape(id, pos, rail_at)
        }
    })
}

fn cell_point(cell: [i32; 3], p: [f32; 3]) -> Vec3 {
    Vec3::new(
        cell[0] as f32 + p[0],
        cell[1] as f32 + p[1],
        cell[2] as f32 + p[2],
    )
}

fn path_len(a: [f32; 3], b: [f32; 3]) -> f32 {
    (Vec3::from(b) - Vec3::from(a)).length().max(1e-4)
}

/// Refresh `pos` + `yaw` from the bound cell/param. Keeps the last
/// pose when the rail vanished (the cart freezes in place).
pub(crate) fn snap_to_rail(
    world: &World,
    shapes: &std::collections::HashMap<[i32; 3], u8>,
    cart: &mut Minecart,
) {
    let Some(shape) = rail_shape_at(world, shapes, cart.cell) else {
        return;
    };
    let (a, b) = shape_path(shape);
    let pa = cell_point(cart.cell, a);
    let pb = cell_point(cart.cell, b);
    cart.pos = pa.lerp(pb, cart.t.clamp(0.0, 1.0));
    // W177: align yaw to the rail axis ALWAYS, not only while moving.
    // A freshly placed (stationary) cart kept its default yaw (0 =
    // east), so it sat sideways on a north-south rail. Moving uses the
    // travel direction (vel sign); parked aligns to the a->b chord.
    let d = pb - pa;
    if d.length_squared() > 1e-6 {
        let dir = if cart.vel.abs() > 0.1 { d * cart.vel.signum() } else { d };
        cart.yaw = (-dir.z).atan2(dir.x);
    }
}

/// Hop to the rail continuing past the endpoint the cart just hit.
/// Returns `false` when no rail continues there (caller stops the
/// cart). Leaving an ascending TOP connects one block up; level
/// exits continue level or drop onto a descending rail one below.
fn advance_cell(
    world: &World,
    shapes: &std::collections::HashMap<[i32; 3], u8>,
    cart: &mut Minecart,
    exit_b: bool,
) -> bool {
    let Some(shape) = rail_shape_at(world, shapes, cart.cell) else {
        return false;
    };
    let (a, b) = shape_path(shape);
    let e = if exit_b { b } else { a };
    // Outward step: away from the cell centre along the dominant axis.
    let (dx, dz) = (e[0] - 0.5, e[2] - 0.5);
    let step = if dx.abs() > dz.abs() {
        [if dx > 0.0 { 1 } else { -1 }, 0, 0]
    } else {
        [0, 0, if dz > 0.0 { 1 } else { -1 }]
    };
    let base = [
        cart.cell[0] + step[0],
        cart.cell[1],
        cart.cell[2] + step[2],
    ];
    let candidates: [[i32; 3]; 2] = if e[1] > 0.5 {
        [[base[0], base[1] + 1, base[2]], base]
    } else {
        [base, [base[0], base[1] - 1, base[2]]]
    };
    let exit_w = cell_point(cart.cell, e);
    for c in candidates {
        let Some(ns) = rail_shape_at(world, shapes, c) else {
            continue;
        };
        // Enter at whichever endpoint of the next path touches the
        // exit point; flip the param direction to keep moving inward.
        let (na, nb) = shape_path(ns);
        let da = exit_w.distance_squared(cell_point(c, na));
        let db = exit_w.distance_squared(cell_point(c, nb));
        let speed = cart.vel.abs();
        let enter_a = da <= db;
        cart.cell = c;
        cart.t = if enter_a { 0.0 } else { 1.0 };
        cart.vel = if enter_a { speed } else { -speed };
        return true;
    }
    false
}

/// One simulation step. `input` = rider thrust in -1..1 already
/// projected onto +t (see `rider_thrust`); `powered` = `Some(on)`
/// when the cell holds a powered/activator rail.
pub(crate) fn tick_cart(
    world: &World,
    shapes: &std::collections::HashMap<[i32; 3], u8>,
    cart: &mut Minecart,
    dt: f32,
    input: f32,
    powered: Option<bool>,
) {
    let Some(shape) = rail_shape_at(world, shapes, cart.cell) else {
        // Rail mined / washed away under the cart: freeze in place
        // (no free ground rolling in the MVP).
        cart.vel = 0.0;
        return;
    };
    let (a, b) = shape_path(shape);
    let len = path_len(a, b);
    // Gravity pulls toward the low end of a slope.
    cart.vel -= SLOPE_G * ((b[1] - a[1]) / len) * dt;
    cart.vel += RIDE_ACCEL * input * dt;
    match powered {
        Some(true) => {
            // Boost along the current motion. A stopped cart needs a
            // rider kick or a slope first (MC's launch rules need an
            // adjacent solid block; the rider kick covers the MVP).
            if cart.vel.abs() > 0.05 {
                cart.vel += POWERED_ACCEL * cart.vel.signum() * dt;
            }
        }
        Some(false) => {
            // Unpowered booster brakes hard (MC halves per tick).
            cart.vel *= 0.5_f32.powf(dt * 20.0);
        }
        None => {}
    }
    cart.vel *= FRICTION_PER_TICK.powf(dt * 20.0);
    cart.vel = cart.vel.clamp(-CART_MAX_SPEED, CART_MAX_SPEED);
    if cart.vel.abs() < 0.02 && input == 0.0 {
        cart.vel = 0.0;
    }
    // Advance along the path, hopping cells as endpoints are hit.
    // The hop bound (8) caps one tick's traversal; at 8 m/s and
    // 60 fps a cart crosses well under one cell per frame anyway.
    let mut remaining = cart.vel.abs() * dt;
    for _ in 0..8 {
        if remaining <= 1e-6 || cart.vel == 0.0 {
            break;
        }
        let Some(shape) = rail_shape_at(world, shapes, cart.cell) else {
            cart.vel = 0.0;
            break;
        };
        let (a, b) = shape_path(shape);
        let len = path_len(a, b);
        let fwd = cart.vel >= 0.0;
        let room = if fwd {
            (1.0 - cart.t) * len
        } else {
            cart.t * len
        };
        if remaining < room {
            let dpar = remaining / len;
            cart.t += if fwd { dpar } else { -dpar };
            break;
        }
        remaining -= room;
        cart.t = if fwd { 1.0 } else { 0.0 };
        if !advance_cell(world, shapes, cart, fwd) {
            cart.vel = 0.0;
            break;
        }
    }
    snap_to_rail(world, shapes, cart);
}

/// Project the rider's raw W/S input (-1..1) onto the cart's +t
/// tangent using the rider's horizontal look, so W always means
/// "the way I'm facing" regardless of the cart's param direction.
pub(crate) fn rider_thrust(
    world: &World,
    shapes: &std::collections::HashMap<[i32; 3], u8>,
    cart: &Minecart,
    raw: f32,
    yaw: f32,
) -> f32 {
    if raw == 0.0 {
        return 0.0;
    }
    let Some(shape) = rail_shape_at(world, shapes, cart.cell) else {
        return 0.0;
    };
    let (a, b) = shape_path(shape);
    let tan = Vec3::from(b) - Vec3::from(a);
    let look = Vec3::new(yaw.cos(), 0.0, -yaw.sin());
    let d = look.x * tan.x + look.z * tan.z;
    if d.abs() < 0.05 { 0.0 } else { raw * d.signum() }
}

/// Ray pick against every cart's MC-sized box (0.98 x 0.7 x 0.98).
/// Returns the closest `(id, distance)` within `max_d`.
pub(crate) fn raycast_cart(
    carts: &[Minecart],
    origin: Vec3,
    dir: Vec3,
    max_d: f32,
) -> Option<(u32, f32)> {
    let dir = dir.normalize_or_zero();
    if dir == Vec3::ZERO {
        return None;
    }
    let mut best: Option<(u32, f32)> = None;
    for c in carts {
        let bmin = c.pos - Vec3::new(0.49, 0.0, 0.49);
        let bmax = c.pos + Vec3::new(0.49, 0.7, 0.49);
        if let Some((t_in, t_out)) =
            crate::world::targeting::ray_aabb(origin, dir, bmin, bmax)
            && t_out >= 0.0
        {
            let t = t_in.max(0.0);
            if t <= max_d && best.is_none_or(|(_, bt)| t < bt) {
                best = Some((c.id, t));
            }
        }
    }
    best
}

// ---------------------------------------------------------------
// Voxel-coupled glue (spawn / break / mount / per-frame tick).
// ---------------------------------------------------------------

/// RMB with the minecart item aimed at a rail: spawn a cart mid-cell
/// and consume one item (host/offline; client placement is a
/// follow-up - the server owns the cart list).
pub(crate) fn place_cart_from_click(vx: &mut crate::Voxel, cell: [i32; 3]) {
    use crate::net::NetRole;
    if matches!(vx.net, NetRole::Client { .. }) {
        tracing::info!("minecart place ignored on client (host-authoritative MVP)");
        return;
    }
    if rail_shape_at(&vx.world, &vx.rail_shapes, cell).is_none() {
        return;
    }
    let id = vx.next_cart_id;
    vx.next_cart_id = vx.next_cart_id.wrapping_add(1).max(1);
    let mut cart = Minecart {
        id,
        cell,
        t: 0.5,
        vel: 0.0,
        pos: Vec3::ZERO,
        yaw: 0.0,
    };
    snap_to_rail(&vx.world, &vx.rail_shapes, &mut cart);
    vx.carts.push(cart);
    if !vx.abilities.instabuild
        && let Some((_, count)) = vx.inventory[vx.selected_slot].as_mut()
    {
        *count = count.saturating_sub(1);
        if *count == 0 {
            vx.inventory[vx.selected_slot] = None;
        }
    }
}

/// LMB on a cart: pop it back to the item (MC instant cart break).
pub(crate) fn break_cart(vx: &mut crate::Voxel, id: u32) {
    use crate::net::NetRole;
    let Some(idx) = vx.carts.iter().position(|c| c.id == id) else {
        return;
    };
    if vx.riding_cart == Some(id) {
        dismount(vx);
    }
    let cart = vx.carts.remove(idx);
    crate::drops::spawn(
        vx,
        infengine_voxel::BlockId(MINECART_ITEM_ID),
        cart.cell,
    );
    if let NetRole::Server { server, .. } = &vx.net {
        let cfg = bincode::config::standard();
        if let Ok(buf) = bincode::serde::encode_to_vec(id, cfg) {
            server.broadcast_bytes("vx.cart_remove", buf);
        }
    }
}

pub(crate) fn mount(vx: &mut crate::Voxel, id: u32) {
    if vx.carts.iter().any(|c| c.id == id) {
        vx.riding_cart = Some(id);
        vx.cart_input = 0.0;
        vx.cart_dismount_pending = false;
    }
}

/// Step off: stand the player up at the cart's cell (gravity settles
/// them onto the rail/floor next tick).
pub(crate) fn dismount(vx: &mut crate::Voxel) {
    let Some(id) = vx.riding_cart.take() else {
        return;
    };
    if let Some(c) = vx.carts.iter().find(|c| c.id == id) {
        vx.player_pos =
            c.pos + Vec3::new(0.0, crate::PLAYER_HEIGHT + 0.2, 0.0);
        vx.player_vel = Vec3::ZERO;
    }
    vx.cart_input = 0.0;
    vx.cart_dismount_pending = false;
}

/// Per-frame cart pass: simulate every cart (offline/host), pin the
/// rider, stream snapshots to clients at ~10 Hz.
pub(crate) fn tick_all(vx: &mut crate::Voxel, dt: f32) {
    use crate::net::NetRole;
    if matches!(vx.net, NetRole::Client { .. }) {
        return; // clients render `vx.cart` snapshots only
    }
    if vx.cart_dismount_pending {
        dismount(vx);
    }
    // Pre-compute booster power + rider thrust per cart (immutable
    // pass; `rail_is_powered` is an `&self` helper so it can't run
    // inside the mutable tick loop).
    let mut params: Vec<(f32, Option<bool>)> =
        Vec::with_capacity(vx.carts.len());
    for c in &vx.carts {
        let id = vx.world.get(c.cell).0;
        let powered = if crate::block::rail::is_powerable(id) {
            Some(vx.rail_is_powered(c.cell, id))
        } else {
            None
        };
        let thrust = if vx.riding_cart == Some(c.id) {
            rider_thrust(&vx.world, &vx.rail_shapes, c, vx.cart_input, vx.player_yaw)
        } else {
            0.0
        };
        params.push((thrust, powered));
    }
    let world = &vx.world;
    let shapes = &vx.rail_shapes;
    for (cart, (thrust, powered)) in vx.carts.iter_mut().zip(params) {
        tick_cart(world, shapes, cart, dt, thrust, powered);
    }
    // Pin the rider to the cart (after physics so the camera follows
    // the fresh pose; update_play zeroed the walk velocity already).
    if let Some(id) = vx.riding_cart {
        match vx.carts.iter().find(|c| c.id == id) {
            Some(c) => {
                vx.player_pos = c.pos + Vec3::new(0.0, RIDER_EYE, 0.0);
                vx.player_vel = Vec3::ZERO;
                vx.grounded = true;
                vx.fall_start_y = None;
            }
            None => vx.riding_cart = None,
        }
    }
    // Host: stream cart states at ~10 Hz.
    vx.cart_net_accum += dt;
    if vx.cart_net_accum >= 0.1 {
        vx.cart_net_accum = 0.0;
        if let NetRole::Server { server, .. } = &vx.net {
            let cfg = bincode::config::standard();
            for c in &vx.carts {
                let msg = crate::net::CartMsg {
                    id: c.id,
                    pos: [c.pos.x, c.pos.y, c.pos.z],
                    yaw: c.yaw,
                    vel: c.vel,
                };
                if let Ok(buf) = bincode::serde::encode_to_vec(msg, cfg) {
                    server.broadcast_bytes("vx.cart", buf);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use infengine_voxel::{BlockId, Chunk};

    const RAIL: u16 = crate::block::RAIL_ID;

    fn world_with_rails(cells: &[[i32; 3]]) -> World {
        let mut w = World::default();
        w.insert_chunk([0, 0, 0], Chunk::empty());
        for &c in cells {
            w.set(c, BlockId(RAIL));
        }
        w
    }

    fn cart_at(cell: [i32; 3], vel: f32) -> Minecart {
        Minecart {
            id: 1,
            cell,
            t: 0.5,
            vel,
            pos: Vec3::ZERO,
            yaw: 0.0,
        }
    }

    #[test]
    fn cart_rolls_along_a_straight_line_and_hops_cells() {
        let cells: Vec<[i32; 3]> =
            (4..=12).map(|x| [x, 4, 4]).collect();
        let w = world_with_rails(&cells);
        let mut c = cart_at([5, 4, 4], 2.0);
        tick_cart(&w, &std::collections::HashMap::new(), &mut c, 1.0, 0.0, None);
        assert!(c.cell[0] > 5, "cart should hop east, at {:?}", c.cell);
        assert!(c.vel > 0.0, "still rolling");
        assert!(
            (c.pos.y - (4.0 + 1.0 / 16.0)).abs() < 1e-4,
            "rides on the rail plate, y={}",
            c.pos.y,
        );
        assert_eq!(c.cell[2], 4, "stays on the line");
    }

    #[test]
    fn cart_stops_at_the_end_of_the_track() {
        let w = world_with_rails(&[[4, 4, 4], [5, 4, 4], [6, 4, 4]]);
        let mut c = cart_at([5, 4, 4], 6.0);
        for _ in 0..40 {
            tick_cart(&w, &std::collections::HashMap::new(), &mut c, 0.05, 0.0, None);
        }
        assert_eq!(c.vel, 0.0, "track end stops the cart");
        assert_eq!(c.cell, [6, 4, 4], "parked on the last rail");
        assert!((c.t - 1.0).abs() < 1e-4, "at the far endpoint");
    }

    #[test]
    fn curve_turns_the_cart_into_the_side_branch() {
        // West-east run into an L turning south:
        //   [4,4,4]-[5,4,4] with [5,4,5] below the corner.
        let w = world_with_rails(&[[4, 4, 4], [5, 4, 4], [5, 4, 5], [5, 4, 6]]);
        let mut c = cart_at([4, 4, 4], 3.0);
        for _ in 0..40 {
            tick_cart(&w, &std::collections::HashMap::new(), &mut c, 0.05, 0.0, None);
        }
        assert!(
            c.cell[2] > 4,
            "curve should route the cart south, at {:?}",
            c.cell,
        );
    }

    #[test]
    fn ascending_rail_climbs_one_block() {
        let w = world_with_rails(&[[4, 4, 4], [5, 5, 4], [6, 5, 4]]);
        let mut c = cart_at([4, 4, 4], 5.0);
        for _ in 0..40 {
            tick_cart(&w, &std::collections::HashMap::new(), &mut c, 0.05, 0.0, None);
        }
        assert_eq!(c.cell[1], 5, "cart climbed the slope, at {:?}", c.cell);
    }

    #[test]
    fn unpowered_booster_brakes_hard() {
        let cells: Vec<[i32; 3]> = (4..=12).map(|x| [x, 4, 4]).collect();
        let w = world_with_rails(&cells);
        let mut c = cart_at([5, 4, 4], 6.0);
        tick_cart(&w, &std::collections::HashMap::new(), &mut c, 0.5, 0.0, Some(false));
        assert!(
            c.vel < 0.1,
            "unpowered booster must kill the speed, vel={}",
            c.vel,
        );
    }
}
