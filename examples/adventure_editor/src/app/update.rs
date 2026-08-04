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
        let current = match self.sheet.as_ref() {
            // While a sheet is being cut, the cut is what plays: the
            // whole point is to see the frames land.
            Some(draft) => Some(draft.to_animation()),
            None => self
                .playback
                .selected
                .clone()
                .and_then(|id| self.room_animations.iter().find(|a| a.id == id).cloned()),
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
        // In Script mode the graph covers the centre; drawing the room
        // underneath it would be work nobody sees.
        if let Some(renderer) = self.renderer.as_mut()
            && self.mode != Mode::Script
        {
            draw::sync_camera(renderer, &self.view);
            draw::draw_layers(renderer, &self.room, &self.view, &self.textures);
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
            draw::draw_overlays(
                renderer,
                &self.room,
                &self.view,
                &self.overlays,
                &selected,
                &walk_line,
                self.play.game.state.camera,
            );
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
        if self.story_ui.open && self.story_cache.is_none() {
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
        let mode = self.mode;
        let cursor_room = self.view.window_to_room(self.cursor);
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
                sheet: &mut self.sheet,
                playing: self.play.active,
                now: if self.play.active {
                    self.play.doing()
                } else {
                    String::new()
                },
                camera: self.play.game.state.camera,
                cursor_room,
                timeline_open: &mut self.timeline_open,
                content_min: egui::Vec2::ZERO,
                content_max: egui::Vec2::ZERO,
                cards: &mut self.cards,
            };
            let view = &mut self.view;
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
            let library_ui = &mut self.library_ui;
            let story_cache = &self.story_cache;
            let timeline_selected = &self.timeline_selected;
            ui.run(window.raw(), |ctx| {
                // Applied before anything is laid out, so the whole
                // frame is measured at the size it will be drawn at.
                ctx.set_zoom_factor(*ui_scale);
                menu_actions =
                    crate::menu::bar(ctx, &menu_ctx, state.overlays, mode, ui_scale);
                crate::help::window(ctx, help_ui);
                game_actions = ui_game::window(ctx, game_ui, &room_names, &languages);
                screen_actions = ui_screen::window(ctx, screen_ui, &languages);
                if story_ui.open
                    && let Some(story) = story_cache.as_ref()
                {
                    story_actions =
                        crate::ui_story::window(ctx, story_ui, story, &story_start);
                }
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
                        extras.play.active,
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
                // What is left between the panels is the canvas; the
                // view frames the room against it.
                let rect = ctx.content_rect();
                state.content_min = egui::vec2(rect.left(), rect.top());
                state.content_max = egui::vec2(rect.right(), rect.bottom());
                if mode == Mode::Script {
                    graph_actions = ui_graph::panel(ctx, graph_ui, &script_ids);
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
        if let Some(id) = library_actions.preview {
            self.playback.select(id);
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
        if library_actions.edit_screens {
            self.screen_ui.open = true;
        }
        self.apply_timeline(timeline_actions);
        self.apply_menu(menu_actions);
        self.apply_play_actions(play_actions);
        self.apply_game_actions(game_actions);
        self.apply_screen_actions(screen_actions);
        self.apply_graph_actions(graph_actions);
    }
}
