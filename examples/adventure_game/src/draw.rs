//! A36.4: drawing a room for a player rather than an author.
//!
//! No overlays, no handles, no grid - the layers, whatever animation is
//! running, and the actor. What the editor draws on top of this is the
//! difference between the two programs.

use std::collections::HashMap;

use glam::Vec2;
use infengine_adventure::{Animation, Room};
use infengine_render::{Renderer, SpriteInstance, Texture2dHandle};

use crate::view::View;

/// Height of the actor marker at scale 1, in room units.
const ACTOR_HEIGHT: f32 = 60.0;

/// Point the camera at the window and clear it.
pub fn sync_camera(renderer: &mut Renderer, view: &View) {
    let camera = renderer.camera_mut();
    camera.position = [view.viewport.x * 0.5, view.viewport.y * 0.5];
    camera.zoom = 1.0;
}

/// Draw every visible layer, back to front.
pub fn layers(
    renderer: &mut Renderer,
    room: &Room,
    view: &View,
    textures: &HashMap<String, Texture2dHandle>,
) {
    for layer in &room.layers {
        if !layer.visible {
            continue;
        }
        let Some(handle) = textures.get(&layer.image) else {
            continue;
        };
        let Some((w, h)) = renderer.texture_size(*handle) else {
            continue;
        };
        let size = Vec2::new(w as f32, h as f32) * view.scale;
        let centre = view.to_window(layer.offset) + size * 0.5;
        renderer.draw_texture(
            *handle,
            SpriteInstance::new([centre.x, flip(view, centre.y)], [size.x, size.y]),
        );
    }
}

/// Draw one frame of an animation over the room.
pub fn animation_frame(
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
    let size = animation.frame_size * view.scale;
    let centre = view.to_window(at) + size * 0.5;
    let mut instance = SpriteInstance::new([centre.x, flip(view, centre.y)], [size.x, size.y]);
    instance.uv_min = [uv_min.x, uv_min.y];
    instance.uv_max = [uv_max.x, uv_max.y];
    renderer.draw_texture(handle, instance);
}

/// One frame of the player character, ready to draw.
#[derive(Debug, Clone, Copy)]
pub struct ActorFrame<'a> {
    pub animation: &'a Animation,
    pub handle: Texture2dHandle,
    pub frame: u32,
    /// Where the feet are, in room space.
    pub at: Vec2,
    /// How large the figure is here, from the room's depth ramp.
    pub scale: f32,
    /// Flip it horizontally, for a sheet with one side view.
    pub mirrored: bool,
}

/// Draw the player character from its walk sheet.
///
/// `at` is where the feet are, so the sprite hangs above the point the
/// walk logic works with rather than being centred on it. A figure
/// centred on its position stands with its waist on the floor and its
/// feet through it, which reads as the depth ramp being wrong when it is
/// not.
///
/// `mirrored` flips the frame horizontally, which is how a sheet with
/// one side view faces the other way.
pub fn actor_sprite(renderer: &mut Renderer, view: &View, look: &ActorFrame<'_>) {
    let ActorFrame {
        animation,
        handle,
        frame,
        at,
        scale,
        mirrored,
    } = *look;
    let Some((sheet_w, sheet_h)) = renderer.texture_size(handle) else {
        return;
    };
    let (uv_min, uv_max) =
        animation.frame_uv(frame, Vec2::new(sheet_w as f32, sheet_h as f32));
    let size = animation.frame_size * scale.max(0.05) * view.scale;
    let feet = view.to_window(at);
    let centre = Vec2::new(feet.x, feet.y - size.y * 0.5);
    let mut instance = SpriteInstance::new([centre.x, flip(view, centre.y)], [size.x, size.y]);
    // Swapping the horizontal ends of the frame's window into the sheet
    // mirrors it without a second copy of the art.
    let (left, right) = if mirrored {
        (uv_max.x, uv_min.x)
    } else {
        (uv_min.x, uv_max.x)
    };
    instance.uv_min = [left, uv_min.y];
    instance.uv_max = [right, uv_max.y];
    renderer.draw_texture(handle, instance);
}

/// Draw the actor as an outline.
///
/// What a project gets before its character sheets are cut: a marker
/// that stands the right height for where it is, so a wrong depth ramp
/// is visible rather than merely wrong.
pub fn actor(renderer: &mut Renderer, view: &View, at: Vec2, scale: f32) {
    let height = ACTOR_HEIGHT * scale.max(0.05) * view.scale;
    let half = height * 0.22;
    let feet = view.to_window(at);
    let colour = [0.95, 0.9, 0.75, 1.0];
    let corners = [
        Vec2::new(feet.x - half, feet.y),
        Vec2::new(feet.x - half * 0.6, feet.y - height),
        Vec2::new(feet.x + half * 0.6, feet.y - height),
        Vec2::new(feet.x + half, feet.y),
    ];
    for i in 0..corners.len() {
        let a = corners[i];
        let b = corners[(i + 1) % corners.len()];
        renderer.debug_line([a.x, flip(view, a.y)], [b.x, flip(view, b.y)], colour);
    }
}

/// Room space runs y down and the renderer's world runs y up.
fn flip(view: &View, y: f32) -> f32 {
    view.viewport.y - y
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view() -> View {
        let mut v = View {
            viewport: Vec2::new(640.0, 400.0),
            room: Vec2::new(320.0, 200.0),
            ..Default::default()
        };
        v.fit();
        v
    }

    #[test]
    fn flipping_twice_returns_the_same_point() {
        let v = view();
        assert_eq!(flip(&v, flip(&v, 123.0)), 123.0);
    }

    #[test]
    fn the_top_of_the_window_flips_to_its_height() {
        let v = view();
        assert_eq!(flip(&v, 0.0), 400.0);
        assert_eq!(flip(&v, 400.0), 0.0);
    }
}
