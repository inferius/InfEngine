//! A83: one frame of the editor.
//!
//! Moved out of `mod.rs` whole: the frame is the editor's longest
//! stretch of glue, and the struct file should read as state plus small
//! verbs. The order panels are laid out in is the design's stacking -
//! menu bar, side panels, timeline at the very bottom with the status
//! strip above it, and in Script mode the graph filling the centre.

use glam::Vec2;

use infengine_adventure::Project;
use infengine_window::{EngineWindow, FrameContext};

use crate::mode::Mode;
use crate::play;
use crate::ui::{self, UiActions, UiState};
use crate::ui_game;
use crate::ui_graph;
use crate::ui_play;
use crate::ui_screen;
use crate::draw;

use super::Editor;

/// A sheet as an interface texture, cached per image in egui's own
/// store - the workbench and the cutter both draw through egui, and
/// re-decoding a sheet every frame would be a visible stutter.
fn sheet_texture(
    ctx: &egui::Context,
    project: &Project,
    image: &str,
) -> Option<egui::TextureHandle> {
    let key = egui::Id::new(("sheet_preview", image));
    if let Some(handle) = ctx.data(|d| d.get_temp::<egui::TextureHandle>(key)) {
        return Some(handle);
    }
    let loaded = image::open(project.resolve(image)).ok()?;
    let rgba = loaded.to_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    let pixels = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
    let handle =
        ctx.load_texture(format!("cutter:{image}"), pixels, egui::TextureOptions::NEAREST);
    ctx.data_mut(|d| d.insert_temp(key, handle.clone()));
    Some(handle)
}

impl Editor {
    pub(super) fn frame(&mut self, window: &EngineWindow, ctx: &FrameContext<'_>) {
        if self.renderer.is_none() {
            return;
        }
        let request = play::tick(
            &mut self.play,
            &self.room,
            &self.project.animations,
            &self.project.locales,
            &self.language,
            ctx.delta_time,
        );
        if !request.is_empty() {
            self.perform(request);
        }
        if self.play.active {
            let p = self.cursor_room();
            let verb = self.play.verb;
            self.play.hover = self.play.game.resolve(&self.room, p, verb);
        }

        // Advance the preview before anything is drawn, so the frame
        // shown is the one for this instant.
        //
        // Only where previews belong: the canvas in Room mode is the
        // authored room and nothing else - a stage with a clear frame,
        // not a place things land on because a button was pressed.
        // Animate composes on it and Test plays on it; those two show
        // guests.
        let preview_allowed = matches!(self.mode, Mode::Animate | Mode::Test) || self.play.active;
        let current = match self.sheet.as_ref() {
            // While a sheet is being cut, the cut is what plays: the
            // whole point is to see the frames land.
            Some(draft) if preview_allowed => Some(draft.to_animation()),
            Some(_) => None,
            None if preview_allowed => self
                .playback
                .selected
                .clone()
                .and_then(|id| self.room_animations.iter().find(|a| a.id == id).cloned()),
            None => None,
        };
        self.playback.tick(ctx.delta_time, current.as_ref());
        let preview = current.and_then(|animation| {
            let frame = self.playback.frame(&animation);
            let handle = self.animation_texture(&animation.image)?;
            Some((animation, handle, frame))
        });

        // Compute what depends on &self before taking the renderer out
        // mutably; the borrow checker cannot see that they are disjoint.
        let selected = self.selected_points();
        let cursor_room = self.cursor_room();
        let rubber = match self.drag {
            super::Drag::HotspotRect(from) => Some(from),
            _ => None,
        };
        let actor = self.play.active.then(|| {
            let position = self.play.game.state.actor.position;
            (position, self.room.scale.scale_at(position.y))
        });
        let walk_line: Vec<Vec2> = if self.play.active {
            std::iter::once(self.play.game.state.actor.position)
                .chain(self.play.game.state.actor.path().iter().copied())
                .collect()
        } else {
            self.path.clone()
        };
        // The workbench advances its own clock, whatever the modes do.
        if let Some(bench) = self.bench.as_mut() {
            bench.tick(ctx.delta_time);
        }

        // Story and Script fill the centre with their own thing, and so
        // does the workbench and the page that opens games; drawing the
        // room under any of them is work nobody sees.
        let centre_taken =
            !self.mode.has_room_canvas() || self.bench.is_some() || self.browser.open;
        if let Some(renderer) = self.renderer.as_mut()
            && !centre_taken
        {
            draw::sync_camera(renderer, &self.view);
            // While a test run is going the room wears the state the
            // game is in - the table with the letter, or without it.
            // While editing, every switched-on layer shows.
            let staged_flags = self
                .play
                .active
                .then(|| self.play.game.state.flags.clone());
            draw::draw_layers(
                renderer,
                &self.room,
                &self.view,
                &self.textures,
                staged_flags.as_ref(),
            );
            if let Some((animation, handle, frame)) = &preview {
                draw::draw_animation_frame(
                    renderer,
                    &self.view,
                    animation,
                    *handle,
                    *frame,
                    self.playback.position,
                );
            }
            // While the game runs, the stage is the game: the editing
            // overlays would sit on top of every scene like scaffolding
            // left up on opening night. The camera frame survives - it
            // is the one line that describes the play, not the edit.
            let staged = if self.play.active {
                draw::Overlays {
                    floor: false,
                    hotspots: false,
                    exits: false,
                    entries: false,
                    bounds: false,
                    camera: self.overlays.camera,
                }
            } else {
                self.overlays
            };
            draw::draw_overlays(
                renderer,
                &self.room,
                &self.view,
                &staged,
                &selected,
                &walk_line,
                self.play.game.state.camera,
            );
            // What the hand is in the middle of, live: the outline
            // from its first corner, the rectangle from its first
            // pixel.
            if self.tool == crate::edit::Tool::DrawFloor && !self.floor_draft.is_empty() {
                draw::draw_floor_draft(renderer, &self.view, &self.floor_draft, cursor_room);
            }
            if let Some(from) = rubber {
                draw::draw_rubber_rect(renderer, &self.view, from, cursor_room);
            }
            if let Some((position, scale)) = actor {
                draw::draw_actor(renderer, &self.view, position, scale);
            }
            // The scene's still goes over everything the room drew,
            // which is where a script put it.
            if self.play.active
                && self.play.still.is_some()
                && let Some((_, handle)) = self.play_still
            {
                draw::draw_still(renderer, &self.room, &self.view, handle);
            }
        }
        if self.play.still.is_none() {
            self.play_still = None;
        }

        // Build the UI, then apply what it asked for after the borrow of
        // the room ends.
        let mut actions = UiActions::default();
        // Snapshotted before the UI borrows the language pickers: the
        // screen preview only reads it, but the borrow checker cannot
        // see that through the closure.
        let language = self.language.clone();
        let locales = self.project.locales.clone();
        // For the screen preview: which flags the test run has set, so
        // conditional widgets show as the game would show them.
        let play_flags = self.play.game.state.flags.clone();
        let mut play_actions = ui_play::PlayActions::default();
        let mut game_actions = ui_game::GameActions::default();
        let mut screen_actions = ui_screen::ScreenActions::default();
        let mut graph_actions = ui_graph::GraphActions::default();
        let mut menu_actions = crate::menu::MenuActions::default();
        let mut timeline_actions = crate::timeline::TimelineActions::default();
        let mut story_actions = crate::ui_story::StoryActions::default();
        let mut library_actions = crate::library::LibraryActions::default();
        let mut asset_actions = crate::bench::BenchActions::default();
        let mut browser_actions = crate::project_browser::BrowserActions::default();
        let screen_names: Vec<String> =
            self.screen_ui.screens.iter().map(|s| s.id.clone()).collect();
        if self.mode == Mode::Story && self.story_cache.is_none() {
            self.story_cache =
                Some(infengine_adventure::story::Story::of(&self.project));
        }
        let story_start = self
            .project
            .manifest
            .as_ref()
            .map(|manifest| manifest.start_room.clone())
            .unwrap_or_default();
        // Only laid out in Animate mode with the panel open: the
        // timeline is a mode's worth of screen, and a room being traced
        // does not want one.
        let scene_line = (self.mode == Mode::Animate && self.timeline_open)
            .then(|| self.scene_timeline())
            .flatten();
        // The same steps sorted onto tracks: who does what when.
        let scene_tracks: Vec<crate::tracks::Track> = scene_line
            .as_ref()
            .zip(self.project.scripts.get(&self.scene_script_id()))
            .map(|(line, script)| crate::tracks::assign(line, &script.actions))
            .unwrap_or_default();
        // The selected step's current values, for the details column.
        let scene_detail: Option<infengine_adventure::Action> = self
            .timeline_selected
            .as_ref()
            .zip(self.project.scripts.get(&self.scene_script_id()))
            .and_then(|(path, script)| {
                crate::timeline::action_at(&script.actions, path).cloned()
            });
        let picker_animations: Vec<String> =
            self.project.animations.iter().map(|a| a.id.clone()).collect();
        let picker_sounds: Vec<String> = self.project.sounds.ids().map(str::to_string).collect();
        let picker_rooms: Vec<String> = self.project.room_ids();
        let picker_images = self.pictures.clone();
        // Every flag the project's scripts mention, for the layer
        // conditions: naming one from memory is how a variant ends up
        // gated on a flag nothing ever sets.
        let known_flags: Vec<String> = {
            let mut flags: std::collections::BTreeSet<String> = self
                .project
                .scripts
                .values()
                .flat_map(|script| script.flags())
                .collect();
            flags.extend(self.play.game.state.flags.keys().cloned());
            flags.into_iter().collect()
        };
        let mode = self.mode;
        let game_view = self.game_ui.manifest.view;
        let asset_image = self.bench.as_ref().and_then(|b| b.image().map(str::to_string));
        let scene_id = self.scene_script_id();
        // The room's scenes, for the timeline's picker.
        let scene_ids: Vec<String> = {
            let prefix = format!("{}.", self.room.script_prefix());
            let mut ids: Vec<String> = self
                .project
                .scripts
                .keys()
                .filter(|id| id.starts_with(&prefix))
                .cloned()
                .collect();
            ids.sort();
            ids
        };
        let mut content = None;
        if let Some(ui) = self.ui.as_mut() {
            let mut state = UiState {
                project: &self.project,
                current_room: self.current_room,
                room: &mut self.room,
                mode,
                tool: &mut self.tool,
                overlays: &mut self.overlays,
                selection: self.selection,
                dirty: self.dirty,
                missing_images: &self.missing_images,
                status: &self.status,
                playback: &mut self.playback,
                animations: &self.room_animations,
                languages: &self.languages,
                text_language: &mut self.language.text,
                voice_language: &mut self.language.voice,
                lines: &self.lines,
                pictures: &picker_images,
                known_flags: &known_flags,
                sheet: &mut self.sheet,
                playing: self.play.active,
                now: if self.play.active {
                    self.play.doing()
                } else {
                    String::new()
                },
                bench: self
                    .bench
                    .as_ref()
                    .map(|open| (open.what(), open.dirty())),
                camera: self.play.game.state.camera,
                cursor_room,
                game_view,
                timeline_open: &mut self.timeline_open,
                content_min: egui::Vec2::ZERO,
                content_max: egui::Vec2::ZERO,
                cards: &mut self.cards,
            };
            let view = &mut self.view;
            let player = &mut self.player;
            let bench = &mut self.bench;
            let browser = &mut self.browser;
            let recent = &self.recent;
            let play = &mut self.play;
            let play_draft = &mut self.play_draft;
            let room_id = state.room.id.clone();
            let slots = &self.slots;
            let game_ui = &mut self.game_ui;
            let room_names: Vec<String> = state
                .project
                .rooms
                .iter()
                .map(|path| Project::room_label(path))
                .collect();
            let languages = self.languages.clone();
            let screen_ui = &mut self.screen_ui;
            let graph_ui = &mut self.graph_ui;
            let script_ids: Vec<String> = {
                let mut ids: Vec<String> = state.project.scripts.keys().cloned().collect();
                ids.sort();
                ids
            };
            let crumb = {
                let project = state
                    .project
                    .root
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("project");
                if state.current_room.is_some() {
                    format!("{project} · {room_id}")
                } else {
                    project.to_string()
                }
            };
            let menu_ctx = crate::menu::MenuContext {
                has_room: state.current_room.is_some(),
                dirty: state.dirty,
                playing: play.active,
                crumb,
            };
            self.help.offer_once();
            let help_ui = &mut self.help;
            let ui_scale = &mut self.ui_scale;
            let timeline_zoom = &mut self.timeline_zoom;
            let timeline_snap = &mut self.timeline_snap;
            let timeline_reveal = self.timeline_reveal.take();
            let story_ui = &mut self.story_ui;
            let story_layout = &mut self.story_layout;
            let library_ui = &mut self.library_ui;
            let story_cache = &self.story_cache;
            let timeline_selected = &self.timeline_selected;
            ui.run(window.raw(), |ctx| {
                // Applied before anything is laid out, so the whole
                // frame is measured at the size it will be drawn at.
                ctx.set_zoom_factor(*ui_scale);
                // The workbench draws the sheet through egui rather
                // than the room renderer, so it needs its own texture.
                let asset_texture = asset_image
                    .as_deref()
                    .and_then(|image| sheet_texture(ctx, state.project, image));
                menu_actions =
                    crate::menu::bar(ctx, &menu_ctx, state.overlays, mode, ui_scale);
                crate::help::window(ctx, help_ui);
                game_actions = ui_game::window(ctx, game_ui, &room_names, &languages);
                screen_actions = ui_screen::window(ctx, screen_ui, &languages);
                if let Some(screen) = screen_ui.preview_screen() {
                    screen_actions.fired =
                        ui_screen::preview(ctx, screen, &locales, &language, &play_flags);
                }

                // The side panels, then the bottom of the window from
                // the bottom up: the timeline sits at the very bottom,
                // the status strip above it, the way the design stacks
                // them.
                library_actions = crate::library::panel(
                    ctx,
                    library_ui,
                    &mut state,
                    &mut actions,
                    &picker_images,
                );
                let mut extras = crate::inspector::Extras {
                    play,
                    play_draft,
                    slots,
                    room_id: room_id.clone(),
                    on_canvas: graph_ui.script_id.clone(),
                };
                play_actions =
                    crate::inspector::panel(ctx, &mut state, &mut actions, &mut extras);
                crate::toolbar::panel(ctx, &mut state, view);
                if let Some(line) = scene_line.as_ref() {
                    timeline_actions = crate::timeline::panel(
                        ctx,
                        line,
                        &scene_tracks,
                        &scene_id,
                        &scene_ids,
                        extras.play.active,
                        extras.play.paused,
                        extras.play.elapsed,
                        timeline_zoom,
                        timeline_snap,
                        timeline_selected,
                        timeline_reveal.as_deref(),
                        scene_detail.as_ref(),
                        &crate::timeline::Pickers {
                            animations: &picker_animations,
                            sounds: &picker_sounds,
                            rooms: &picker_rooms,
                            images: &picker_images,
                        },
                    );
                }
                ui::status_bar(ctx, &mut state);
                // The player strip sits above the status bar, present
                // only while something is playing.
                crate::player::panel(ctx, player);
                // What is left between the panels is the canvas; the
                // view frames the room against it. available_rect, not
                // content_rect: the latter is the whole screen, and
                // measuring against it put the frame chip under the
                // menu and made Fit centre the room beneath the panels.
                let rect = ctx.available_rect();
                state.content_min = egui::vec2(rect.left(), rect.top());
                state.content_max = egui::vec2(rect.right(), rect.bottom());
                // An asset on the workbench owns the centre - it is
                // not a room, and editing it over one was the
                // confusion this replaces.
                if browser.open {
                    // The page that opens games owns the centre: there
                    // may be no game to draw behind it.
                    browser_actions =
                        crate::project_browser::panel(ctx, browser, recent);
                } else if let Some(open) = bench.as_mut() {
                    asset_actions = crate::bench::panel(
                        ctx,
                        open,
                        asset_texture.clone(),
                        &room_names,
                        &crate::screen_desk::Context {
                            locales: &locales,
                            language: &language,
                            pictures: &picker_images,
                            screens: &screen_names,
                            scripts: &script_ids,
                            languages: &languages,
                            texture: &|image| sheet_texture(ctx, state.project, image),
                        },
                    );
                } else {
                    ui::stage_chips(ctx, &mut state, &scene_id);
                    if mode == Mode::Story {
                        // The map owns the centre in Story mode: it is
                        // the whole game, not a window over one room.
                        if let Some(story) = story_cache.as_ref() {
                            story_actions = crate::ui_story::panel(
                                ctx,
                                story_ui,
                                story,
                                story_layout,
                                &story_start,
                            );
                        }
                    }
                    if mode == Mode::Script {
                        graph_actions = ui_graph::panel(
                            ctx,
                            graph_ui,
                            &script_ids,
                            ui_graph::GraphOptions {
                                animations: &picker_animations,
                                sounds: &picker_sounds,
                                rooms: &picker_rooms,
                                images: &picker_images,
                            },
                        );
                    }
                }
                if extras.play.active {
                    ui_play::subtitle(ctx, extras.play);
                }
            });
            content = Some((state.content_min, state.content_max));
        }
        if let Some((min, max)) = content {
            self.view
                .set_content_rect(Vec2::new(min.x, min.y), Vec2::new(max.x, max.y));
        }

        if let (Some(renderer), Some(ui)) = (self.renderer.as_mut(), self.ui.as_mut()) {
            let _ = renderer.render(|device, queue, encoder, view, size| {
                ui.render(device, queue, encoder, view, size);
            });
        }

        // The picker writes straight into the choice, so notice the
        // change here rather than routing it through an action.
        let chosen = self.language.clone();
        if chosen != self.last_language {
            self.last_language = chosen;
            self.collect_lines();
        }

        self.apply_actions(actions);
        if let Some(id) = story_actions.open_room
            && let Some(index) = self
                .project
                .rooms
                .iter()
                .position(|path| infengine_adventure::Project::room_label(path) == id)
        {
            self.open_room(index);
            // Opening a place from the map means going there, so the
            // editor follows.
            self.set_mode(Mode::Room);
        }
        if let Some(id) = story_actions.set_start {
            self.game_ui.manifest.start_room = id.clone();
            self.game_ui.status = match self.game_ui.manifest.save(&self.project.root) {
                Ok(()) => format!("the game now starts in {id}"),
                Err(err) => err.to_string(),
            };
            self.status = self.game_ui.status.clone();
            self.story_cache = None;
        }
        if story_actions.arrange {
            self.story_layout.at.clear();
            self.save_story_layout();
            self.status = "arranged".into();
        }
        if story_actions.save_layout {
            self.save_story_layout();
        }
        if let Some(action) = library_actions.add {
            // Land it in the scene's timeline and turn the editor
            // there, so the step arrives somewhere visible.
            self.set_mode(Mode::Animate);
            self.apply_timeline(crate::timeline::TimelineActions {
                insert: Some((self.timeline_selected.clone(), action)),
                ..Default::default()
            });
            self.status = "step added - it is on the scene's timeline".into();
        }
        if let Some(id) = library_actions.play {
            match self.project.sounds.get(&id).map(str::to_string) {
                Some(file) => self.play_file(&file),
                None => self.status = format!("{id} has no file behind it"),
            }
        }
        if let Some(id) = library_actions.open_script {
            // The canvas lives in Script mode; go where it is.
            self.set_mode(Mode::Script);
            self.open_graph(&id);
        }
        if let Some(name) = library_actions.new_room {
            self.create_room(&name);
        }
        if let Some(id) = library_actions.edit_animation {
            self.open_asset(&id);
        }
        if let Some(id) = library_actions.edit_screen {
            self.open_screen_desk(&id);
        }
        if let Some(id) = library_actions.edit_font {
            self.open_font_desk(&id);
        }
        if let Some(image) = library_actions.cut_sheet {
            self.cut_new_asset(&image);
        }
        if asset_actions.save {
            self.save_bench();
        }
        if let Some(root) = browser_actions.open {
            self.open_project(&root);
        }
        if let Some(root) = browser_actions.create {
            self.create_project(&root);
        }
        if let Some(root) = browser_actions.forget {
            self.forget_recent(&root);
        }
        if menu_actions.open_browser {
            self.show_browser();
        }
        if asset_actions.close {
            self.bench = None;
            self.status = "closed the workbench".into();
        }
        self.apply_timeline(timeline_actions);
        self.apply_menu(menu_actions);
        self.apply_play_actions(play_actions);
        self.apply_game_actions(game_actions);
        self.apply_screen_actions(screen_actions);
        self.apply_graph_actions(graph_actions);
    }
}
