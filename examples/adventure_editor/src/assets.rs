//! A22.1: getting the project's files onto the screen and into the ears.
//!
//! Free functions rather than methods, so the editor keeps them out of
//! its own state and the borrow checker can see that a texture cache and
//! a room are different things.

use std::collections::HashMap;

use infengine_audio::AudioSystem;
use infengine_render::{Renderer, Texture2dHandle, TextureFilter};
use infengine_adventure::Room;
use tracing::warn;

use infengine_adventure::Project;

/// Upload every layer image the room references.
///
/// Textures are keyed by the project-relative path, so two layers
/// sharing a sheet upload it once. Returns the images that would not
/// load, which the inspector shows rather than leaving a room silently
/// half-drawn.
pub fn upload_layers(
    renderer: &mut Renderer,
    project: &Project,
    cache: &mut HashMap<String, Texture2dHandle>,
    room: &Room,
) -> Vec<String> {
    for handle in cache.values() {
        renderer.destroy_texture(*handle);
    }
    cache.clear();

    let mut missing = Vec::new();
    for layer in &room.layers {
        if cache.contains_key(&layer.image) {
            continue;
        }
        match upload(renderer, project, &layer.image) {
            Some(handle) => {
                cache.insert(layer.image.clone(), handle);
            }
            None => missing.push(layer.image.clone()),
        }
    }
    missing
}

/// Upload an image if it is not already loaded, and return it.
pub fn texture_for(
    renderer: &mut Renderer,
    project: &Project,
    cache: &mut HashMap<String, Texture2dHandle>,
    image: &str,
) -> Option<Texture2dHandle> {
    if let Some(handle) = cache.get(image) {
        return Some(*handle);
    }
    let handle = upload(renderer, project, image)?;
    cache.insert(image.to_string(), handle);
    Some(handle)
}

/// Load one project-relative image, for pictures a scene shows over
/// itself rather than the room's own layers.
pub fn upload_one(
    renderer: &mut Renderer,
    project: &Project,
    image: &str,
) -> Option<Texture2dHandle> {
    upload(renderer, project, image)
}

fn upload(
    renderer: &mut Renderer,
    project: &Project,
    image: &str,
) -> Option<Texture2dHandle> {
    let path = project.resolve(image);
    match renderer.create_texture_from_file(&path, TextureFilter::Nearest) {
        Ok(handle) => Some(handle),
        Err(err) => {
            warn!(image, %err, "image did not load");
            None
        }
    }
}

/// Play a sound given as a project-relative path, keeping the handle.
///
/// The handle and the duration are what the player strip runs on:
/// where the sound is, how long it is, and the means to hold, seek
/// and stop it.
pub fn play_file(
    audio: Option<&mut AudioSystem>,
    project: &Project,
    relative: &str,
) -> Result<(infengine_audio::ActiveSound, f32), String> {
    let Some(audio) = audio else {
        return Err("no audio device".to_string());
    };
    let path = project.resolve(relative);
    let sound = audio.load_sound(&path).map_err(|err| {
        warn!(file = relative, %err, "could not load");
        err.to_string()
    })?;
    let handle = audio.play_sound_tracked(&sound).map_err(|err| {
        warn!(file = relative, %err, "could not play");
        err.to_string()
    })?;
    Ok((handle, sound.duration_secs()))
}
