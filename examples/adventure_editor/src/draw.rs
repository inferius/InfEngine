//! A6.3: drawing a room and its editing overlays.
//!
//! Free functions over borrowed state rather than methods on the app, so
//! the draw order is readable in one place and the borrow checker does
//! not have to reason about the whole application at once.

use std::collections::HashMap;

use glam::Vec2;
use infengine_adventure::{Animation, Room, Shape};

/// Height of the actor marker at scale 1, in room units. About what
/// a person occupies on a 200-line screen.
const ACTOR_HEIGHT: f32 = 60.0;
use infengine_render::{Renderer, SpriteInstance, Texture2dHandle};

use crate::view::View;

/// Colours, kept together so the overlay reads as one palette.
pub mod color {
    pub const FLOOR: [f32; 4] = [0.25, 1.0, 0.35, 0.9];
    pub const HOLE: [f32; 4] = [1.0, 0.3, 0.25, 0.9];
    pub const HOTSPOT: [f32; 4] = [0.35, 0.75, 1.0, 0.85];
    pub const EXIT: [f32; 4] = [1.0, 0.85, 0.2, 0.9];
    pub const ENTRY: [f32; 4] = [1.0, 0.4, 0.9, 0.95];
    pub const SELECTED: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
    pub const PATH: [f32; 4] = [1.0, 0.55, 0.1, 1.0];
    pub const BOUNDS: [f32; 4] = [0.45, 0.45, 0.55, 0.7];
    /// The frame of what the player sees. Brighter than the
    /// room's edge, because it is the one that moves.
    pub const CAMERA: [f32; 4] = [1.0, 1.0, 0.55, 0.95];
    pub const PREVIEW: [f32; 4] = [1.0, 0.9, 0.3, 0.8];
    pub const ACTOR: [f32; 4] = [0.4, 1.0, 0.6, 0.9];
}

/// Which overlays are switched on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Overlays {
    pub floor: bool,
    pub hotspots: bool,
    pub exits: bool,
    pub entries: bool,
    pub bounds: bool,
    /// The frame of what the player sees, for a room bigger than one
    /// screen.
    pub camera: bool,
}

impl Default for Overlays {
    fn default() -> Self {
        Self {
            floor: true,
            hotspots: true,
            exits: true,
            entries: true,
            bounds: true,
            camera: true,
        }
    }
}

/// Point the renderer's camera at what the view is framing.
pub fn sync_camera(renderer: &mut Renderer, view: &View) {
    let camera = renderer.camera_mut();
    let world = view.room_to_world(view.center);
    camera.position = [world.x, world.y];
    camera.zoom = view.zoom;
}

/// Draw the room's visible layers, back to front.
///
/// A layer whose image has not loaded is skipped rather than drawn as a
/// placeholder: a magenta box over a background reads as a broken room,
/// and the inspector already reports which images are missing.
pub fn draw_layers(
    renderer: &mut Renderer,
    room: &Room,
    view: &View,
    textures: &HashMap<String, Texture2dHandle>,
) {
    for layer in room.layers.iter().filter(|l| l.visible) {
        let Some(&handle) = textures.get(&layer.image) else {
            continue;
        };
        let Some((w, h)) = renderer.texture_size(handle) else {
            continue;
        };
        let size = Vec2::new(w as f32, h as f32);
        let center_room = layer.offset + size * 0.5;
        let center = view.room_to_world(center_room);
        renderer.draw_texture(
            handle,
            SpriteInstance::new([center.x, center.y], [size.x, size.y]),
        );
    }
}

/// Draw the actor as a marker, sized by the room's depth ramp.
///
/// A placeholder until character sprite sheets are decoded, but a
/// truthful one: it stands the right height for where it is, so a
/// mis-authored scale ramp is visible immediately.
pub fn draw_actor(renderer: &mut Renderer, view: &View, at: Vec2, scale: f32) {
    let height = ACTOR_HEIGHT * scale.max(0.05);
    let half = height * 0.25;
    let head = Vec2::new(at.x, at.y - height);
    outline(
        renderer,
        view,
        &[
            at,
            Vec2::new(at.x - half, at.y - height * 0.5),
            head,
            Vec2::new(at.x + half, at.y - height * 0.5),
        ],
        color::ACTOR,
    );
    // The feet, which is the position the game actually reasons about.
    outline(
        renderer,
        view,
        &[
            Vec2::new(at.x - 2.0, at.y),
            Vec2::new(at.x + 2.0, at.y),
        ],
        color::ACTOR,
    );
}

/// Draw one frame of an animation over the room.
///
/// The sheet is one image with the frames side by side, so the frame is
/// selected by texture coordinates rather than by holding one texture
/// per frame - a room with six three-frame loops would otherwise mean
/// eighteen uploads for no gain.
pub fn draw_animation_frame(
    renderer: &mut Renderer,
    view: &View,
    animation: &Animation,
    handle: Texture2dHandle,
    frame: u32,
    at: Vec2,
) {
    let Some((sheet_w, sheet_h)) = renderer.texture_size(handle) else {
        return;
    };
    let (uv_min, uv_max) =
        animation.frame_uv(frame, Vec2::new(sheet_w as f32, sheet_h as f32));
    let size = animation.frame_size;
    let center = view.room_to_world(at + size * 0.5);
    let mut instance = SpriteInstance::new([center.x, center.y], [size.x, size.y]);
    instance.uv_min = [uv_min.x, uv_min.y];
    instance.uv_max = [uv_max.x, uv_max.y];
    renderer.draw_texture(handle, instance);

    // A thin box, because a frame that is entirely transparent is
    // otherwise indistinguishable from one that failed to load.
    outline(
        renderer,
        view,
        &[
            at,
            Vec2::new(at.x + size.x, at.y),
            at + size,
            Vec2::new(at.x, at.y + size.y),
        ],
        color::PREVIEW,
    );
}

/// Draw a scene's still: a picture over everything, centred on the
/// room, at its own size - what the game does when a script shows one.
pub fn draw_still(renderer: &mut Renderer, room: &Room, view: &View, handle: Texture2dHandle) {
    let Some((w, h)) = renderer.texture_size(handle) else {
        return;
    };
    let size = Vec2::new(w as f32, h as f32);
    let center = view.room_to_world(room.size * 0.5);
    renderer.draw_texture(
        handle,
        SpriteInstance::new([center.x, center.y], [size.x, size.y]),
    );
}

/// Draw one closed polygon outline in room coordinates.
fn outline(renderer: &mut Renderer, view: &View, points: &[Vec2], color: [f32; 4]) {
    if points.len() < 2 {
        return;
    }
    for i in 0..points.len() {
        let a = view.room_to_world(points[i]);
        let b = view.room_to_world(points[(i + 1) % points.len()]);
        renderer.debug_line([a.x, a.y], [b.x, b.y], color);
    }
}

/// Draw a small square centred on a room point, sized in screen pixels
/// so handles stay grabbable at any zoom.
fn handle(renderer: &mut Renderer, view: &View, at: Vec2, color: [f32; 4], pixels: f32) {
    let r = view.screen_pixels_in_room(pixels);
    let c = view.room_to_world(at);
    renderer.debug_rect([c.x - r, c.y - r], [c.x + r, c.y + r], color);
}

fn shape_outline(renderer: &mut Renderer, view: &View, shape: &Shape, color: [f32; 4]) {
    match shape {
        Shape::Rect { min, max } => {
            let lo = min.min(*max);
            let hi = min.max(*max);
            outline(
                renderer,
                view,
                &[
                    lo,
                    Vec2::new(hi.x, lo.y),
                    hi,
                    Vec2::new(lo.x, hi.y),
                ],
                color,
            );
        }
        Shape::Poly(poly) => outline(renderer, view, &poly.points, color),
    }
}

/// Draw every enabled overlay.
pub fn draw_overlays(
    renderer: &mut Renderer,
    room: &Room,
    view: &View,
    overlays: &Overlays,
    selected_points: &[Vec2],
    path: &[Vec2],
    camera_at: Vec2,
) {
    if overlays.bounds {
        outline(
            renderer,
            view,
            &[
                Vec2::ZERO,
                Vec2::new(room.size.x, 0.0),
                room.size,
                Vec2::new(0.0, room.size.y),
            ],
            color::BOUNDS,
        );
    }

    // What the player will actually see, when that is less than the
    // room. Drawn as a frame over the whole room rather than by cropping
    // the editor's own view: the point of editing a shot is seeing what
    // is outside it as well as what is in.
    if overlays.camera && room.camera.scrolls(room.size) {
        let size = room.camera.view_in(room.size);
        let at = room.camera.clamp(room.size, camera_at);
        outline(
            renderer,
            view,
            &[
                at,
                at + Vec2::new(size.x, 0.0),
                at + size,
                at + Vec2::new(0.0, size.y),
            ],
            color::CAMERA,
        );
    }

    if overlays.floor {
        for poly in &room.walk.solids {
            outline(renderer, view, &poly.points, color::FLOOR);
            for point in &poly.points {
                handle(renderer, view, *point, color::FLOOR, 3.0);
            }
        }
        for poly in &room.walk.holes {
            outline(renderer, view, &poly.points, color::HOLE);
            for point in &poly.points {
                handle(renderer, view, *point, color::HOLE, 3.0);
            }
        }
    }

    if overlays.hotspots {
        for hotspot in &room.hotspots {
            let tint = if hotspot.enabled {
                color::HOTSPOT
            } else {
                dim(color::HOTSPOT)
            };
            shape_outline(renderer, view, &hotspot.shape, tint);
            if let Some(walk_to) = hotspot.walk_to {
                handle(renderer, view, walk_to, tint, 4.0);
            }
        }
    }

    if overlays.exits {
        for exit in &room.exits {
            shape_outline(renderer, view, &exit.shape, color::EXIT);
        }
    }

    if overlays.entries {
        for entry in &room.entries {
            handle(renderer, view, entry.position, color::ENTRY, 5.0);
        }
    }

    for point in selected_points {
        handle(renderer, view, *point, color::SELECTED, 5.0);
    }

    for pair in path.windows(2) {
        let a = view.room_to_world(pair[0]);
        let b = view.room_to_world(pair[1]);
        renderer.debug_line([a.x, a.y], [b.x, b.y], color::PATH);
    }
    for point in path {
        handle(renderer, view, *point, color::PATH, 2.5);
    }
}

/// Half-strength colour, for things switched off.
fn dim(c: [f32; 4]) -> [f32; 4] {
    [c[0] * 0.4, c[1] * 0.4, c[2] * 0.4, c[3] * 0.6]
}

/// The floor outline being drawn, from its very first click.
///
/// A closed polygon only exists from three points, and until this the
/// draft drew as nothing - so the first two clicks of every outline
/// looked like the tool was broken. Now each placed corner shows, the
/// placed run is a polyline, and a rubber line runs from the last
/// corner to the cursor: the next click is always visible before it is
/// made.
pub fn draw_floor_draft(renderer: &mut Renderer, view: &View, draft: &[Vec2], cursor: Vec2) {
    for pair in draft.windows(2) {
        let a = view.room_to_world(pair[0]);
        let b = view.room_to_world(pair[1]);
        renderer.debug_line([a.x, a.y], [b.x, b.y], color::FLOOR);
    }
    for point in draft {
        handle(renderer, view, *point, color::FLOOR, 3.0);
    }
    if let Some(last) = draft.last() {
        let a = view.room_to_world(*last);
        let b = view.room_to_world(cursor);
        renderer.debug_line([a.x, a.y], [b.x, b.y], dim(color::FLOOR));
    }
}

/// The rectangle being dragged out, live under the hand.
pub fn draw_rubber_rect(renderer: &mut Renderer, view: &View, from: Vec2, to: Vec2) {
    let lo = from.min(to);
    let hi = from.max(to);
    outline(
        renderer,
        view,
        &[
            lo,
            Vec2::new(hi.x, lo.y),
            hi,
            Vec2::new(lo.x, hi.y),
        ],
        color::HOTSPOT,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlays_default_to_everything_visible() {
        let o = Overlays::default();
        assert!(o.floor && o.hotspots && o.exits && o.entries && o.bounds);
    }

    #[test]
    fn dimming_lowers_brightness_without_going_negative() {
        let d = dim(color::HOTSPOT);
        assert!(d.iter().all(|c| *c >= 0.0));
        assert!(d[2] < color::HOTSPOT[2]);
    }

    #[test]
    fn overlay_colours_are_distinct() {
        // A floor and a hole are drawn on top of each other often enough
        // that sharing a colour would be a real usability bug.
        assert_ne!(color::FLOOR, color::HOLE);
        assert_ne!(color::HOTSPOT, color::EXIT);
        assert_ne!(color::SELECTED, color::PATH);
    }
}
