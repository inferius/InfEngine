//! A24.3: what the mouse and the keyboard do to a room.
//!
//! Split out of the editor's glue because it is the one part with real
//! behaviour in it - picking, dragging, drawing an outline - and it was
//! the reason `app` kept growing.

use glam::Vec2;

use infengine_adventure::find_path;

use crate::edit::{self, Selection, Tool};

use super::{Drag, Editor, GRAB_PIXELS, MIN_HOTSPOT};

impl Editor {
    /// Room-space position of the cursor.
    pub(super) fn cursor_room(&self) -> Vec2 {
        self.view.window_to_room(self.cursor)
    }

    pub(super) fn grab_radius(&self) -> f32 {
        self.view.screen_pixels_in_room(GRAB_PIXELS)
    }

    pub(super) fn on_left_press(&mut self) {
        let p = self.cursor_room();
        if self.play.active {
            // A click through a line reads it away; only then does it
            // mean something in the room.
            if self.play.subtitle.is_some() {
                self.play.skip_subtitle();
                return;
            }
            let verb = self.play.verb;
            let click = self.play.game.click(&self.room, p, verb);
            self.status = click.label();
            self.play.hover = click;
            return;
        }
        match self.tool {
            Tool::Select => {
                if let Some(at) = edit::vertex_at(&self.room, p, self.grab_radius()) {
                    self.selection = Selection::Vertex(at);
                    self.drag = Drag::Vertex(at);
                } else if let Some(index) = edit::hotspot_at(&self.room, p) {
                    self.selection = Selection::Hotspot(index);
                    self.drag = Drag::Hotspot(index);
                } else {
                    self.selection = Selection::None;
                    self.drag = Drag::Pan;
                }
            }
            Tool::DrawFloor => {
                edit::extend_floor(&mut self.room, &mut self.floor_draft, p);
                self.dirty = true;
            }
            Tool::DrawHotspot => self.drag = Drag::HotspotRect(p),
            Tool::WalkPreview => self.preview_walk(p),
        }
    }

    /// Route the actor to `goal` and keep the result for drawing.
    pub(super) fn preview_walk(&mut self, goal: Vec2) {
        let start = self.walk_from.or_else(|| {
            self.room
                .entries
                .first()
                .map(|e| e.position)
                .or_else(|| self.room.walk.clamp(goal))
        });
        let Some(start) = start else {
            self.status = "this room has no floor to walk on".into();
            return;
        };
        match find_path(&self.room.walk, start, goal) {
            Ok(path) => {
                self.status = format!(
                    "{} legs, {:.0} px",
                    path.len().saturating_sub(1),
                    infengine_adventure::path_length(&path)
                );
                self.walk_from = path.last().copied();
                self.path = path;
            }
            Err(err) => {
                self.path.clear();
                self.status = format!("no route: {err:?}");
            }
        }
    }

    pub(super) fn on_left_release(&mut self) {
        if let Drag::HotspotRect(from) = self.drag {
            let to = self.cursor_room();
            if let Some(index) =
                edit::finish_hotspot_drag(&mut self.room, from, to, MIN_HOTSPOT)
            {
                self.selection = Selection::Hotspot(index);
                self.dirty = true;
            }
        }
        self.drag = Drag::None;
    }

    pub(super) fn on_right_press(&mut self) {
        match self.tool {
            Tool::DrawFloor => {
                self.floor_draft.clear();
                self.status = "outline finished".into();
            }
            Tool::Select => {
                let p = self.cursor_room();
                let radius = self.grab_radius();
                if let Some(at) = edit::insert_vertex_on_edge(&mut self.room, p, radius) {
                    self.selection = Selection::Vertex(at);
                    self.dirty = true;
                }
            }
            _ => {}
        }
    }

    pub(super) fn on_cursor_move(&mut self) {
        let delta = self.cursor - self.last_cursor;
        match self.drag {
            Drag::Pan => self.view.pan(delta),
            Drag::Vertex(at) => {
                let target = self.cursor_room();
                if edit::move_vertex(&mut self.room, at, target) {
                    self.dirty = true;
                }
            }
            Drag::Hotspot(index) => {
                let room_delta = delta / self.view.zoom.max(f32::EPSILON);
                if edit::move_hotspot(&mut self.room, index, room_delta) {
                    self.dirty = true;
                }
            }
            Drag::None | Drag::HotspotRect(_) => {}
        }
    }

    /// Points drawn with the selection highlight.
    pub(super) fn selected_points(&self) -> Vec<Vec2> {
        match self.selection {
            Selection::Vertex(at) => {
                let list = match at.kind {
                    edit::PolyKind::Solid => &self.room.walk.solids,
                    edit::PolyKind::Hole => &self.room.walk.holes,
                };
                list.get(at.polygon)
                    .and_then(|p| p.points.get(at.vertex))
                    .copied()
                    .into_iter()
                    .collect()
            }
            _ => Vec::new(),
        }
    }
}
