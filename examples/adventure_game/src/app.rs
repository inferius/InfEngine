//! A36.6: the game application - glue only.
//!
//! Owns the window, the renderer, the audio device and a [`Host`], and
//! does nothing else. Every decision about what happens next belongs to
//! the host, which is why the same decisions can be tested without any
//! of this.

use std::collections::HashMap;

use glam::Vec2;
use infengine_adventure::host::{Host, Request};
use infengine_adventure::{Animation, Project};
use infengine_audio::AudioSystem;
use infengine_editor::EditorUi;
use infengine_render::{Renderer, Texture2dHandle, TextureFilter};
use infengine_window::raw_winit::event::{ElementState, MouseButton, WindowEvent};
use infengine_window::{AppHandler, EngineWindow, FrameContext, Key};
use tracing::{info, warn};

use crate::draw;
use crate::ui;
use crate::view::View;

pub struct Game {
    renderer: Option<Renderer>,
    ui: Option<EditorUi>,
    audio: Option<AudioSystem>,

    host: Host,
    view: View,
    /// Layer and sheet textures, keyed by their project-relative path.
    textures: HashMap<String, Texture2dHandle>,
    /// The animation playing in this room, how far into it we are,
    /// and where the script put it if it said.
    animation: Option<(Animation, f32, Option<Vec2>)>,
    /// A picture over the room, and the seconds it has left.
    /// Infinity means until the room changes.
    still: Option<(String, f32)>,
    /// How far into the walk cycle the actor is.
    ///
    /// Kept across rooms: a figure that restarts its stride every time
    /// it walks through a door looks like it stumbles.
    step: f32,
    cursor: Vec2,
    status: String,
    /// Screen art and appearance animation state.
    screen_fx: ui::ScreenFx,
    /// How long the skip key has been held during the intro, and the
    /// seconds the hint stays up after a tap.
    skip_hold: Option<f32>,
    skip_hint: f32,
    /// Set when a screen asks to quit; the window closes next frame.
    quit: bool,
}

impl std::fmt::Debug for Game {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Game").field("host", &self.host).finish()
    }
}

impl Game {
    pub fn new(project: Project) -> Self {
        Self {
            renderer: None,
            ui: None,
            screen_fx: ui::ScreenFx::default(),
            skip_hold: None,
            skip_hint: 0.0,
            audio: AudioSystem::new().ok(),
            host: Host::new(project),
            view: View::default(),
            textures: HashMap::new(),
            animation: None,
            still: None,
            step: 0.0,
            cursor: Vec2::ZERO,
            status: String::new(),
            quit: false,
        }
    }

    /// Upload what the current room draws, dropping the last room's.
    fn load_room_textures(&mut self) {
        // Worked out before the renderer is borrowed: it reads the
        // project, and the borrow checker cannot see that the two do not
        // overlap.
        let keep = self.actor_images();
        let Some(renderer) = self.renderer.as_mut() else {
            return;
        };
        // The actor's own sheets are not the room's and stay loaded;
        // dropping and re-uploading them on every doorway would be work
        // done once per step of the game for no reason.
        let dropped: Vec<String> = self
            .textures
            .keys()
            .filter(|image| !keep.contains(*image))
            .cloned()
            .collect();
        for image in dropped {
            if let Some(handle) = self.textures.remove(&image) {
                renderer.destroy_texture(handle);
            }
        }
        self.animation = None;
        self.still = None;

        let images: Vec<String> = self
            .host
            .room
            .layers
            .iter()
            .map(|layer| layer.image.clone())
            .collect();
        for image in images {
            self.upload(&image);
        }
        self.view.room = self.host.room.size;
        self.view.visible = self.host.room.camera.view_in(self.host.room.size);
        self.view.camera = self.host.game.state.camera;
        self.view.fit();
    }

    /// Images the actor is drawn from, which outlive a room.
    fn actor_images(&self) -> std::collections::HashSet<String> {
        let Some(look) = self.host.project.manifest.as_ref().and_then(|m| m.actor.as_ref())
        else {
            return Default::default();
        };
        [
            &look.walk_left,
            &look.walk_right,
            &look.walk_back,
            &look.walk_front,
            &look.idle,
        ]
        .into_iter()
        .filter(|id| !id.is_empty())
        .filter_map(|id| self.host.project.animations.get(id))
        .map(|animation| animation.image.clone())
        .collect()
    }

    /// The frame of the walk cycle to draw, and whether to mirror it.
    ///
    /// A standing figure shows the first frame rather than freezing
    /// wherever the cycle happened to stop, which would leave it with
    /// one foot in the air.
    fn actor_frame(&self) -> Option<(Animation, u32, bool, Texture2dHandle)> {
        let look = self.host.project.manifest.as_ref()?.actor.as_ref()?;
        let actor = &self.host.game.state.actor;
        let walking = actor.is_walking();
        let id = if walking || look.idle.is_empty() {
            look.walking(actor.facing)
        } else {
            look.idle.as_str()
        };
        let animation = self.host.project.animations.get(id)?.clone();
        let handle = *self.textures.get(&animation.image)?;
        let frame = if walking {
            animation.frame_at(self.step)
        } else {
            0
        };
        Some((animation, frame, look.mirrored(actor.facing), handle))
    }

    fn upload(&mut self, image: &str) -> Option<Texture2dHandle> {
        if let Some(handle) = self.textures.get(image) {
            return Some(*handle);
        }
        let renderer = self.renderer.as_mut()?;
        // By the text language rather than the voice one: this picks a
        // picture, and a picture of handwriting is something you read.
        let path = self
            .host
            .project
            .resolve_in(image, &self.host.language.text);
        match renderer.create_texture_from_file(&path, TextureFilter::Nearest) {
            Ok(handle) => {
                self.textures.insert(image.to_string(), handle);
                Some(handle)
            }
            Err(err) => {
                warn!(image, %err, "image did not load");
                None
            }
        }
    }

    /// Do what the host asked for.
    fn perform(&mut self, request: Request) {
        if request.room_changed {
            self.load_room_textures();
        }
        if request.quit {
            self.quit = true;
        }
        if let Some(id) = request.animation {
            // Playing nothing takes the last one off. A finished
            // animation now holds its last frame, which is what a title
            // card wants - so there has to be a way to say "and now it
            // is gone" that is not a room change.
            if id.is_empty() {
                self.animation = None;
            } else if let Some(animation) = self.host.project.animations.get(&id).cloned() {
                self.upload(&animation.image);
                self.animation = Some((animation, 0.0, request.animation_at));
            } else {
                warn!(id, "no such animation");
            }
        }
        if let Some((image, seconds)) = request.still {
            self.upload(&image);
            let left = if seconds > 0.0 { seconds } else { f32::INFINITY };
            self.still = Some((image, left));
        }
        for file in [request.voice, request.sound]
            .into_iter()
            .flatten()
            .chain(request.music.flatten())
        {
            self.play(&file);
        }
    }

    fn play(&mut self, relative: &str) {
        let Some(audio) = self.audio.as_mut() else {
            return;
        };
        let path = self.host.project.resolve(relative);
        if let Err(err) = audio.load_sound(&path).and_then(|s| audio.play_sound(&s)) {
            warn!(file = relative, %err, "could not play");
        }
    }
}

impl AppHandler for Game {
    fn on_start(&mut self, window: &EngineWindow) {
        match Renderer::new(window) {
            Ok(mut renderer) => {
                renderer.set_clear_color(0.0, 0.0, 0.0, 1.0);
                self.ui = Some(EditorUi::new(
                    renderer.wgpu_device(),
                    renderer.surface_format(),
                    window.raw(),
                    1,
                ));
                self.renderer = Some(renderer);
            }
            Err(err) => warn!(%err, "renderer failed to start"),
        }
        let (w, h) = window.size();
        self.view.viewport = Vec2::new(w as f32, h as f32);

        // Said out loud at startup because a figure that silently does
        // not draw is indistinguishable from one drawn behind the
        // scenery, and the two have nothing in common to fix.
        match self.host.project.manifest.as_ref().and_then(|m| m.actor.as_ref()) {
            Some(look) => {
                let images = self.actor_images();
                info!(
                    sheets = images.len(),
                    left = look.walking(infengine_adventure::interact::Facing::Left),
                    right = look.walking(infengine_adventure::interact::Facing::Right),
                    "actor"
                );
                for image in images {
                    self.upload(&image);
                }
            }
            None => info!("no actor in the manifest; drawing an outline"),
        }
        let request = self.host.start();
        self.perform(request);
        info!(
            rooms = self.host.project.rooms.len(),
            room = %self.host.room.id,
            "game started"
        );
    }

    fn on_window_event(&mut self, window: &EngineWindow, event: &WindowEvent) {
        if let Some(ui) = self.ui.as_mut() {
            ui.handle_window_event(window.raw(), event);
        }
        let over_ui = self
            .ui
            .as_ref()
            .is_some_and(|ui| ui.context().wants_pointer_input());

        match event {
            WindowEvent::CursorMoved { position, .. } => {
                let scale = window.scale_factor() as f32;
                self.cursor = Vec2::new(position.x as f32, position.y as f32) / scale;
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } if !over_ui && self.view.contains(self.cursor) => {
                if self.host.intro {
                    // Holding through the intro is the request to get
                    // on with it; the corner notice fills as it holds.
                    self.skip_hold = Some(0.0);
                    self.skip_hint = 2.5;
                } else {
                    let point = self.view.to_room(self.cursor);
                    self.host.click(point);
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Released,
                button: MouseButton::Left,
                ..
            } => {
                self.skip_hold = None;
            }
            _ => {}
        }
    }

    fn on_key(&mut self, key: Key, pressed: bool) {
        if self.host.intro {
            // Any key skips the intro - held, not tapped, so a stray
            // press cannot cost the scene. The tap leaves a hint.
            if pressed {
                self.skip_hold = Some(0.0);
                self.skip_hint = 2.5;
            } else {
                self.skip_hold = None;
            }
            return;
        }
        if !pressed {
            return;
        }
        match key {
            // Escape opens the pause menu if the game has one, and
            // closes whatever is open otherwise.
            Key::Escape => {
                if self.host.screen.is_some() {
                    let request = self.host.fire(
                        &infengine_adventure::screen::ScreenAction::Close,
                    );
                    self.perform(request);
                } else if self.host.project.screen("pause").is_some() {
                    self.host.screen = Some("pause".into());
                }
            }
            Key::Space => self.host.skip_subtitle(),
            _ => {}
        }
    }

    fn on_resize(&mut self, width: u32, height: u32) {
        if let Some(renderer) = self.renderer.as_mut() {
            renderer.resize(width, height);
            let (w, h) = renderer.camera_mut().viewport();
            self.view.viewport = Vec2::new(w as f32, h as f32);
            self.view.fit();
        }
    }

    fn on_update(&mut self, window: &EngineWindow, ctx: &FrameContext<'_>) {
        if self.renderer.is_none() {
            return;
        }
        if self.quit {
            window.request_close();
            return;
        }

        // The skip hold: filled by keeping the key or button down
        // through the intro, and completing it cuts to the title.
        self.skip_hint = (self.skip_hint - ctx.delta_time).max(0.0);
        if let Some(held) = self.skip_hold.as_mut() {
            *held += ctx.delta_time;
            if *held >= ui::SKIP_HOLD {
                self.skip_hold = None;
                let request = self.host.skip_intro();
                self.perform(request);
            }
        }
        if !self.host.intro {
            self.skip_hold = None;
        }

        let request = self.host.tick(ctx.delta_time);
        // The view is wherever the game state says, every frame: a pan
        // moves it, and so does a camera that follows the actor.
        self.view.camera = self.host.game.state.camera;
        if !request.is_empty() {
            self.perform(request);
        }

        // What the cursor is over, for the status line.
        self.status = if self.view.contains(self.cursor) && self.host.screen.is_none() {
            self.host.resolve(self.view.to_room(self.cursor)).label()
        } else {
            String::new()
        };

        if self.host.game.state.actor.is_walking() {
            self.step += ctx.delta_time;
        }

        if let Some((_, left)) = self.still.as_mut() {
            *left -= ctx.delta_time;
            if *left <= 0.0 {
                self.still = None;
            }
        }

        if let Some((animation, time, _)) = self.animation.as_mut() {
            let was = *time;
            *time += ctx.delta_time;
            // A finished animation releases the script but stays on
            // screen. A title card is an animation that plays once and
            // then holds, and clearing it the moment it ended left
            // Kyrandia's title on screen for four seconds where the
            // original holds it for eleven - the script's own waits had
            // nothing to hold.
            if !animation.looping
                && animation.finished_at(*time)
                && !animation.finished_at(was)
            {
                let id = animation.id.clone();
                self.host.game.runner.finish_animation(&id);
            }
        }

        let room_size = self.host.room.size;
        let actor = {
            let position = self.host.game.state.actor.position;
            (position, self.host.room.scale.scale_at(position.y))
        };
        let sprite = self.actor_frame();
        let frame = self.animation.as_ref().map(|(animation, time, at)| {
            (
                animation.clone(),
                animation.frame_at(*time),
                self.textures.get(&animation.image).copied(),
                *at,
            )
        });

        // Items lying in this room, with their art fetched on first
        // sight. The icon convention is items/<id>.png, project-wide.
        let ground: Vec<(String, glam::Vec2)> = self
            .host
            .game
            .state
            .ground_in(&self.host.room.id)
            .iter()
            .map(|g| (g.item.clone(), g.at))
            .collect();
        for (item, _) in &ground {
            item_texture(
                self.renderer.as_mut(),
                &self.host.project,
                &mut self.textures,
                item,
            );
        }
        let held = self.host.game.state.hand.clone();
        if let Some(item) = &held {
            item_texture(
                self.renderer.as_mut(),
                &self.host.project,
                &mut self.textures,
                item,
            );
        }

        if let Some(renderer) = self.renderer.as_mut() {
            draw::sync_camera(renderer, &self.view);
            draw::layers(renderer, &self.host.room, &self.view, &self.textures);
            for (item, at) in &ground {
                if let Some(&handle) = self.textures.get(&icon_path(item)) {
                    draw::ground_item(renderer, &self.view, *at, handle);
                }
            }
            if let Some(item) = &held
                && let Some(&handle) = self.textures.get(&icon_path(item))
            {
                draw::held_item(renderer, &self.view, self.cursor, handle);
            }
            if let Some((animation, index, Some(handle), placed)) = frame {
                // Where the script said, or where the animation says
                // about itself - a script placing one is the only
                // thing that knows about this particular playing.
                let at = placed.unwrap_or_else(|| animation.position_in(room_size));
                draw::animation_frame(renderer, &self.view, &animation, handle, index, at);
            }
            if let Some(handle) = self
                .still
                .as_ref()
                .and_then(|(image, _)| self.textures.get(image).copied())
            {
                draw::still(renderer, &self.view, handle);
            }
            // No floor means nowhere to stand: a cutscene is a room
            // the player is not in, and drawing a figure on it would be
            // a figure standing in a film.
            if self.host.screen.is_none() && !self.host.room.walk.is_empty() {
                match &sprite {
                    Some((animation, frame, mirrored, handle)) => draw::actor_sprite(
                        renderer,
                        &self.view,
                        &draw::ActorFrame {
                            animation,
                            handle: *handle,
                            frame: *frame,
                            at: actor.0,
                            scale: actor.1,
                            mirrored: *mirrored,
                        },
                    ),
                    None => draw::actor(renderer, &self.view, actor.0, actor.1),
                }
            }
        }

        let mut events = ui::UiEvents::default();
        if let Some(ui_state) = self.ui.as_mut() {
            let host = &self.host;
            let status = self.status.clone();
            let fx = &mut self.screen_fx;
            let skip = ui::SkipNotice {
                held: self.skip_hold,
                hint: self.skip_hint,
                showing: self.host.intro,
            };
            ui_state.run(window.raw(), |ctx| {
                events = ui::build(ctx, host, &status, fx, skip);
            });
        }
        if let Some(action) = events.fired {
            let request = self.host.fire(&action);
            self.perform(request);
        }

        if let (Some(renderer), Some(ui)) = (self.renderer.as_mut(), self.ui.as_mut()) {
            let _ = renderer.render(|device, queue, encoder, view, size| {
                ui.render(device, queue, encoder, view, size);
            });
        }
    }
}


/// Project-relative path of an item's icon.
fn icon_path(item: &str) -> String {
    format!("items/{item}.png")
}

/// Fetch an item's icon into the texture cache, on first sight.
fn item_texture(
    renderer: Option<&mut Renderer>,
    project: &infengine_adventure::Project,
    textures: &mut HashMap<String, Texture2dHandle>,
    item: &str,
) {
    let path = icon_path(item);
    if textures.contains_key(&path) {
        return;
    }
    let Some(renderer) = renderer else { return };
    let file = project.resolve(&path);
    if let Ok(handle) = renderer.create_texture_from_file(&file, TextureFilter::Nearest) {
        textures.insert(path, handle);
    }
}
