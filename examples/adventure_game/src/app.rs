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
    /// The music that is playing, held so the next PlayMusic replaces
    /// it instead of layering a second track over it.
    music: Option<infengine_audio::ActiveSound>,
    /// The line being spoken, so a new one can silence it and the
    /// music knows when to come back up.
    voice: Option<infengine_audio::ActiveSound>,
    /// How far into the walk cycle the actor is.
    ///
    /// Kept across rooms: a figure that restarts its stride every time
    /// it walks through a door looks like it stumbles.
    step: f32,
    /// Always-running clock for the room's own ambient animations.
    clock: f32,
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
    /// A138: seconds at which to write a screenshot, and where.
    /// Diagnostics: what the game really draws, without a person at
    /// the keyboard to catch the moment.
    shots: Vec<f32>,
    shot_dir: std::path::PathBuf,
    /// Seconds since the window opened, for the shot schedule.
    since_start: f32,
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
            music: None,
            voice: None,
            step: 0.0,
            clock: 0.0,
            cursor: Vec2::ZERO,
            status: String::new(),
            quit: false,
            shots: Vec::new(),
            shot_dir: std::path::PathBuf::from("."),
            since_start: 0.0,
        }
    }

    /// Write a PNG at each of these times, then quit after the last.
    pub fn with_shots(mut self, at: Vec<f32>, dir: std::path::PathBuf) -> Self {
        self.shots = at;
        self.shots.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        self.shot_dir = dir;
        self
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

        let mut images: Vec<String> = self
            .host
            .room
            .layers
            .iter()
            .map(|layer| layer.image.clone())
            .collect();
        // The room's own ambient animations draw too, so their sheets
        // load with the room rather than never.
        images.extend(
            self.host
                .project
                .animations
                .for_room(&self.host.room.id)
                .map(|animation| animation.image.clone()),
        );
        for image in images {
            self.upload(&image);
        }
        // The viewport as the surface really is, not as it was when the
        // view was made: a room loaded before the first resize event
        // centred itself in a window that no longer exists.
        if let Some(renderer) = self.renderer.as_mut() {
            let (w, h) = renderer.camera_mut().viewport();
            self.view.viewport = Vec2::new(w as f32, h as f32);
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

    /// Which pointer belongs over this spot.
    ///
    /// A way out shows the arrow for the direction it leads, which is
    /// how a player learns there is a way down at all; anything else
    /// keeps the plain pointer.
    fn cursor_art(&self) -> (&'static str, bool) {
        if self.host.screen.is_some() || !self.view.contains(self.cursor) {
            return ("pointer", false);
        }
        match self.host.resolve(self.view.to_room(self.cursor)) {
            infengine_adventure::game::Click::Exit { id, .. } => match id.as_str() {
                "north" => ("up", true),
                "south" => ("down", true),
                "east" => ("right", true),
                "west" => ("left", true),
                _ => ("pointer", false),
            },
            _ => ("pointer", false),
        }
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
            // A cut is a cut: a line still ringing from the last scene
            // does not follow the player through it.
            if let Some(mut voice) = self.voice.take() {
                voice.stop(0.1);
            }
            if let Some(music) = self.music.as_mut() {
                music.set_volume(1.0, 0.3);
            }
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
        // One voice at a time, and the music out of its way: a new
        // line stops the one still ringing, and while anyone speaks
        // the music ducks so the words carry over it.
        if let Some(file) = request.voice {
            tracing::info!(%file, "voice");
            if let Some(mut old) = self.voice.take() {
                old.stop(0.05);
            }
            self.voice = self.play_tracked(&file);
            if self.voice.is_some()
                && let Some(music) = self.music.as_mut()
            {
                music.set_volume(0.3, 0.15);
            }
        }
        if let Some(file) = request.sound {
            // Named out loud so a silent run can say whether the sound
            // was never asked for or asked for and unheard.
            tracing::info!(%file, "sfx");
            self.play(&file);
        }
        // Music replaces music: the outer Some is "a PlayMusic action
        // happened", the inner one is the track - None stops it. Left
        // untracked, every scene change layered a new track over the
        // old one still playing out.
        if let Some(music) = request.music {
            if let Some(mut old) = self.music.take() {
                old.stop(0.5);
            }
            if let Some(file) = music {
                tracing::info!(%file, "music");
                self.music = self.play_tracked(&file);
            }
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

    fn play_tracked(&mut self, relative: &str) -> Option<infengine_audio::ActiveSound> {
        let audio = self.audio.as_mut()?;
        let path = self.host.project.resolve(relative);
        match audio
            .load_sound(&path)
            .and_then(|s| audio.play_sound_tracked(&s))
        {
            Ok(handle) => Some(handle),
            Err(err) => {
                warn!(file = relative, %err, "could not play");
                None
            }
        }
    }
}

impl AppHandler for Game {
    fn on_start(&mut self, window: &EngineWindow) {
        // The game draws its own pointer; the system's arrow over it
        // reads as two cursors.
        window.raw().set_cursor_visible(false);
        // Vsync: a paced 2D game wants every frame on the display's
        // beat; unsynchronized presents made the intro judder.
        match Renderer::new_vsynced(window) {
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
                // Key auto-repeat delivers "pressed" again and again;
                // restarting the hold on each one made the bar fill
                // halfway, snap back and never finish.
                if self.skip_hold.is_none() {
                    self.skip_hold = Some(0.0);
                }
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
            // A100: Escape goes back, and the front door has no back.
            //
            // A title screen before the game has begun is not something
            // to dismiss: doing so left the player in a room nobody had
            // started, where Escape then offered to resume and save a
            // game that did not exist.
            Key::Escape => {
                if self.host.screen.is_some() {
                    if self.host.started {
                        let request = self.host.fire(
                            &infengine_adventure::screen::ScreenAction::Close,
                        );
                        self.perform(request);
                    }
                } else if self.host.started
                    && self.host.project.screen("pause").is_some()
                {
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
        self.clock += ctx.delta_time;
        // The shot schedule: one file per time asked for, and the run
        // ends with the last of them.
        if !self.shots.is_empty() {
            self.since_start += ctx.delta_time;
            if self.since_start >= self.shots[0] {
                let at = self.shots.remove(0);
                let path = self.shot_dir.join(format!("shot_{:05.1}s.png", at));
                let _ = std::fs::create_dir_all(&self.shot_dir);
                if let Some(renderer) = self.renderer.as_mut() {
                    renderer.grab_next_frame(path);
                }
                if self.shots.is_empty() {
                    self.quit = true;
                }
            }
        }

        if let Some((_, left)) = self.still.as_mut() {
            *left -= ctx.delta_time;
            if *left <= 0.0 {
                self.still = None;
            }
        }

        // The voice ending is when the music comes back up.
        if self.voice.as_ref().is_some_and(|voice| voice.is_stopped()) {
            self.voice = None;
            if let Some(music) = self.music.as_mut() {
                music.set_volume(1.0, 0.4);
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
        // The room's own looping ambients - the sea under the intro's
        // lightning - play on the global clock, under the script's one.
        let ambients: Vec<(Animation, u32, Option<Texture2dHandle>, Vec2)> = self
            .host
            .project
            .animations
            .for_room(&self.host.room.id)
            .filter(|animation| animation.looping)
            .map(|animation| {
                (
                    animation.clone(),
                    animation.frame_at(self.clock),
                    self.textures.get(&animation.image).copied(),
                    animation.position_in(room_size),
                )
            })
            .collect();
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
        let (cursor_name, cursor_centred) = self.cursor_art();
        let cursor_image = format!("cursors/{cursor_name}.png");
        self.upload(&cursor_image);

        let held = self.host.game.state.hand.clone();
        if let Some(item) = &held {
            item_texture(
                self.renderer.as_mut(),
                &self.host.project,
                &mut self.textures,
                item,
            );
        }

        // Snapshotted before the renderer borrow: the closure only
        // reads flags, but the borrow checker cannot see that.
        let flags = self.host.game.state.flags.clone();
        if let Some(renderer) = self.renderer.as_mut() {
            draw::sync_camera(renderer, &self.view);
            draw::layers(renderer, &self.host.room, &self.view, &self.textures, &|flag| {
                flags.get(flag).copied().unwrap_or(false)
            });
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
            if let Some(&handle) = self.textures.get(&cursor_image) {
                draw::cursor(renderer, &self.view, self.cursor, handle, cursor_centred);
            }
            for (animation, index, handle, at) in &ambients {
                if let Some(handle) = handle {
                    draw::animation_frame(renderer, &self.view, animation, *handle, *index, *at);
                }
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
