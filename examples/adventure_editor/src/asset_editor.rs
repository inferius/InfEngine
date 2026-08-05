//! A94: the animation editor - an asset opened in its own workspace.
//!
//! The composition modes edit a room: the picture is the room and
//! everything on it belongs to the room. An animation is not part of
//! any one room, and editing it over one was the confusion - the sheet
//! landed on whatever screen happened to be open. So an animation
//! opens the way an asset opens in Unreal: it takes over the centre,
//! with its own header saying which asset is being edited and a Close
//! that gives the room back.
//!
//! What it shows is the sheet itself with the cut drawn on it: the
//! grid of frames, the frame under the playhead lit, and the animation
//! playing beside it at the size it will draw in the game. Every number
//! that decides the cut - frame size, gap, columns, count - is here,
//! nudgeable a pixel at a time, and the position the animation takes in
//! a room is a pair of pixel values with a snap.

use glam::Vec2;
use infengine_adventure::Animation;

use crate::lang::t;

/// How many pixels of screen one sheet pixel takes, by default.
const DEFAULT_SCALE: f32 = 2.0;

/// The asset on the workbench.
#[derive(Debug, Clone, PartialEq)]
pub struct AssetEditor {
    /// The animation as it is being edited. Saved back on Save.
    pub animation: Animation,
    /// What it was called when it was opened, so a rename replaces the
    /// right entry rather than leaving both.
    pub original_id: String,
    /// Size of the sheet, so the cut cannot promise frames it lacks.
    pub sheet_size: Vec2,
    /// Zoom of the sheet view.
    pub scale: f32,
    /// Whether the preview is running.
    pub playing: bool,
    /// Seconds into the preview.
    pub time: f32,
    /// What the position nudge lands on, in pixels. Zero is free.
    pub snap: i32,
    pub dirty: bool,
}

impl AssetEditor {
    pub fn open(animation: Animation, sheet_size: Vec2) -> Self {
        Self {
            original_id: animation.id.clone(),
            animation,
            sheet_size,
            scale: DEFAULT_SCALE,
            playing: true,
            time: 0.0,
            snap: 1,
            dirty: false,
        }
    }

    /// How many frames of this size the sheet can actually hold.
    pub fn max_frames(&self) -> u32 {
        let size = self.animation.frame_size;
        if size.x <= 0.0 || size.y <= 0.0 {
            return 1;
        }
        // With a gap, N frames span N sizes and N-1 gaps.
        let fit = |space: f32, size: f32, gap: f32| {
            (((space + gap) / (size + gap)).floor() as u32).max(1)
        };
        let per_row = fit(self.sheet_size.x, size.x, self.animation.gap.x);
        if self.animation.columns == 0 {
            return per_row;
        }
        let columns = self.animation.columns.min(per_row);
        let rows = fit(self.sheet_size.y, size.y, self.animation.gap.y);
        (columns * rows).max(1)
    }

    /// Advance the preview clock.
    pub fn tick(&mut self, delta: f32) {
        if self.playing {
            self.time += delta.max(0.0);
        }
    }

    /// The frame showing right now.
    pub fn frame(&self) -> u32 {
        self.animation.frame_at(self.time)
    }
}

/// What the workspace asked the app to do.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct AssetActions {
    /// Write the animation back into the project's index.
    pub save: bool,
    /// Put the asset away and give the room back.
    pub close: bool,
}

/// Draw the workspace in the centre of the window.
pub fn panel(
    ctx: &egui::Context,
    state: &mut AssetEditor,
    texture: Option<egui::TextureHandle>,
    rooms: &[String],
) -> AssetActions {
    let mut actions = AssetActions::default();
    egui::CentralPanel::default().show(ctx, |ui| {
        header(ui, state, &mut actions);
        ui.separator();
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width((ui.available_width() - 280.0).max(200.0));
                egui::ScrollArea::both()
                    .id_salt("asset_sheet")
                    .auto_shrink([false, false])
                    .show(ui, |ui| sheet_view(ui, state, texture.as_ref()));
            });
            ui.separator();
            ui.vertical(|ui| {
                ui.set_width(264.0);
                egui::ScrollArea::vertical()
                    .id_salt("asset_fields")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        preview(ui, state, texture.as_ref());
                        ui.separator();
                        fields(ui, state, rooms);
                    });
            });
        });
    });
    actions
}

fn header(ui: &mut egui::Ui, state: &mut AssetEditor, actions: &mut AssetActions) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(t("ANIMATION", "ANIMACE")).small().weak());
        ui.heading(&state.animation.id);
        ui.label(
            egui::RichText::new(&state.animation.image)
                .small()
                .weak(),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .button(t("Close", "Zavřít"))
                .on_hover_text(t(
                    "Put the asset away and go back to the room",
                    "Odlož asset a vrať se k místnosti",
                ))
                .clicked()
            {
                actions.close = true;
            }
            if ui
                .add_enabled(
                    state.dirty && !state.animation.id.trim().is_empty(),
                    egui::Button::new(t("Save", "Uložit")),
                )
                .on_hover_text(t(
                    "Write it into the project's animations",
                    "Zapiš ji do animací projektu",
                ))
                .on_disabled_hover_text(t("Nothing has changed", "Nic se nezměnilo"))
                .clicked()
            {
                actions.save = true;
            }
            if state.dirty {
                ui.label(
                    egui::RichText::new(t("● unsaved", "● neuloženo"))
                        .small()
                        .color(egui::Color32::from_rgb(255, 200, 120)),
                );
            }
        });
    });
}

/// The sheet with the cut drawn over it.
fn sheet_view(
    ui: &mut egui::Ui,
    state: &mut AssetEditor,
    texture: Option<&egui::TextureHandle>,
) {
    let Some(texture) = texture else {
        ui.label(
            egui::RichText::new(t(
                "The sheet did not load.",
                "Arch se nenačetl.",
            ))
            .weak(),
        );
        return;
    };
    let scale = state.scale;
    let shown = egui::vec2(state.sheet_size.x * scale, state.sheet_size.y * scale);
    let (rect, response) = ui.allocate_exact_size(shown, egui::Sense::hover());
    let painter = ui.painter_at(ui.clip_rect().intersect(rect));
    painter.rect_filled(rect, 0.0, egui::Color32::from_gray(20));
    egui::Image::new((texture.id(), shown)).paint_at(ui, rect);

    // The cut, drawn frame by frame: every cell the animation will
    // sample, with the one on screen lit. A cut that is wrong is
    // obvious here and nowhere else.
    let animation = &state.animation;
    let current = animation.frame_at(state.time);
    let step = animation.frame_size + animation.gap;
    let columns = if animation.columns == 0 {
        animation.frames.max(1)
    } else {
        animation.columns
    };
    for index in 0..animation.frames.max(1) {
        let (col, row) = (index % columns, index / columns);
        let at = egui::pos2(
            rect.min.x + col as f32 * step.x * scale,
            rect.min.y + row as f32 * step.y * scale,
        );
        let cell = egui::Rect::from_min_size(
            at,
            egui::vec2(animation.frame_size.x * scale, animation.frame_size.y * scale),
        );
        let lit = index == current;
        painter.rect_stroke(
            cell,
            0.0,
            egui::Stroke::new(
                if lit { 2.0 } else { 1.0 },
                if lit {
                    egui::Color32::from_rgb(255, 235, 140)
                } else {
                    egui::Color32::from_white_alpha(60)
                },
            ),
            egui::StrokeKind::Inside,
        );
    }
    response.on_hover_text(t(
        "The cut, frame by frame. The lit cell is the one playing.",
        "Řez, snímek po snímku. Rozsvícená buňka právě hraje.",
    ));
}

/// The animation playing at the size it will draw in the game.
fn preview(ui: &mut egui::Ui, state: &mut AssetEditor, texture: Option<&egui::TextureHandle>) {
    ui.horizontal(|ui| {
        if ui
            .button(if state.playing { "⏸" } else { "▶" })
            .clicked()
        {
            state.playing = !state.playing;
        }
        if ui
            .small_button("⏮")
            .on_hover_text(t("Back to the first frame", "Zpět na první snímek"))
            .clicked()
        {
            state.time = 0.0;
        }
        let frame = state.frame();
        ui.label(
            egui::RichText::new(if crate::lang::is_czech() {
                format!("snímek {} z {}", frame + 1, state.animation.frames)
            } else {
                format!("frame {} of {}", frame + 1, state.animation.frames)
            })
            .small()
            .weak(),
        );
    });

    let Some(texture) = texture else {
        return;
    };
    let animation = &state.animation;
    let frame = animation.frame_at(state.time);
    let (uv_min, uv_max) = animation.frame_uv(frame, state.sheet_size);
    let scale = (96.0 / animation.frame_size.x.max(1.0))
        .min(96.0 / animation.frame_size.y.max(1.0))
        .clamp(0.25, 6.0);
    let shown = egui::vec2(
        animation.frame_size.x * scale,
        animation.frame_size.y * scale,
    );
    let (rect, _) = ui.allocate_exact_size(shown, egui::Sense::hover());
    ui.painter().rect_filled(rect, 2.0, egui::Color32::from_gray(25));
    egui::Image::new((texture.id(), shown))
        .uv(egui::Rect::from_min_max(
            egui::pos2(uv_min.x, uv_min.y),
            egui::pos2(uv_max.x, uv_max.y),
        ))
        .paint_at(ui, rect);
    ui.ctx().request_repaint();
}

/// Every number that decides the cut, and where the result stands.
fn fields(ui: &mut egui::Ui, state: &mut AssetEditor, rooms: &[String]) {
    let most = state.max_frames();
    let mut changed = false;

    ui.horizontal(|ui| {
        ui.label(t("name", "název"));
        changed |= ui.text_edit_singleline(&mut state.animation.id).changed();
    });

    ui.add_space(6.0);
    ui.label(egui::RichText::new(t("The cut", "Řez")).small().weak());
    ui.horizontal(|ui| {
        ui.label(t("one frame", "snímek"));
        changed |= ui
            .add(
                egui::DragValue::new(&mut state.animation.frame_size.x)
                    .range(1.0..=state.sheet_size.x.max(1.0))
                    .speed(1.0),
            )
            .changed();
        ui.label("x");
        changed |= ui
            .add(
                egui::DragValue::new(&mut state.animation.frame_size.y)
                    .range(1.0..=state.sheet_size.y.max(1.0))
                    .speed(1.0),
            )
            .changed();
    });
    ui.horizontal(|ui| {
        ui.label(t("gap", "mezera"))
            .on_hover_text(t(
                "Empty pixels between neighbouring frames",
                "Prázdné pixely mezi sousedními snímky",
            ));
        changed |= ui
            .add(egui::DragValue::new(&mut state.animation.gap.x).range(0.0..=64.0))
            .changed();
        changed |= ui
            .add(egui::DragValue::new(&mut state.animation.gap.y).range(0.0..=64.0))
            .changed();
        ui.label(t("columns", "sloupců"))
            .on_hover_text(t(
                "Frames per row; 0 keeps one long row",
                "Snímků na řádek; 0 znamená jednu dlouhou řadu",
            ));
        changed |= ui
            .add(egui::DragValue::new(&mut state.animation.columns).range(0..=64))
            .changed();
    });
    state.animation.frames = state.animation.frames.clamp(1, most);
    changed |= ui
        .add(egui::Slider::new(&mut state.animation.frames, 1..=most).text(t("frames", "snímků")))
        .changed();

    ui.add_space(6.0);
    ui.label(egui::RichText::new(t("Playback", "Přehrávání")).small().weak());
    changed |= ui
        .add(
            egui::Slider::new(&mut state.animation.fps, 1.0..=30.0)
                .text(t("per second", "za sekundu")),
        )
        .changed();
    changed |= ui
        .checkbox(&mut state.animation.looping, t("repeats", "opakuje se"))
        .on_hover_text(t(
            "Otherwise it plays once and stops on the last frame.",
            "Jinak přehraje jednou a zůstane na posledním snímku.",
        ))
        .changed();
    ui.label(
        egui::RichText::new(if crate::lang::is_czech() {
            format!("trvá {:.2}s", state.animation.duration())
        } else {
            format!("runs {:.2}s", state.animation.duration())
        })
        .small()
        .weak(),
    );

    ui.add_space(6.0);
    ui.label(
        egui::RichText::new(t("Where it stands", "Kde stojí"))
            .small()
            .weak(),
    );
    let mut placed = state.animation.at.is_some();
    if ui
        .checkbox(&mut placed, t("placed by hand", "umístit ručně"))
        .on_hover_text(t(
            "Otherwise it is centred in the room, which is what a full-screen cutscene wants.",
            "Jinak se vystředí v místnosti, což chce celoobrazovková scéna.",
        ))
        .changed()
    {
        state.animation.at = placed.then_some(Vec2::ZERO);
        changed = true;
    }
    if let Some(at) = state.animation.at.as_mut() {
        ui.horizontal(|ui| {
            ui.label(t("at", "na"));
            changed |= ui.add(egui::DragValue::new(&mut at.x).speed(1.0)).changed();
            changed |= ui.add(egui::DragValue::new(&mut at.y).speed(1.0)).changed();
            ui.label(t("snap", "krok"))
                .on_hover_text(t(
                    "Nudges land on multiples of this. 1 is pixel by pixel.",
                    "Posuny dosednou na násobky tohoto čísla. 1 je pixel po pixelu.",
                ));
            ui.add(egui::DragValue::new(&mut state.snap).range(1..=64));
        });
        // Nudging: what a pixel-perfect placement is actually made of.
        let step = state.snap.max(1) as f32;
        ui.horizontal(|ui| {
            for (label, delta) in [
                ("←", Vec2::new(-step, 0.0)),
                ("→", Vec2::new(step, 0.0)),
                ("↑", Vec2::new(0.0, -step)),
                ("↓", Vec2::new(0.0, step)),
            ] {
                if ui.small_button(label).clicked() {
                    *at += delta;
                    // Land on the grid, not merely move by it.
                    at.x = (at.x / step).round() * step;
                    at.y = (at.y / step).round() * step;
                    changed = true;
                }
            }
        });
    }

    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label(t("room", "místnost"))
            .on_hover_text(t(
                "Which room owns it. None means it can play anywhere.",
                "Které místnosti patří. Žádná znamená, že jde hrát kdekoli.",
            ));
        let mut owned = state.animation.room.clone().unwrap_or_default();
        egui::ComboBox::from_id_salt("asset_room")
            .selected_text(if owned.is_empty() {
                t("anywhere", "kdekoli")
            } else {
                owned.as_str()
            })
            .width(150.0)
            .show_ui(ui, |ui| {
                if ui.selectable_label(owned.is_empty(), t("anywhere", "kdekoli")).clicked() {
                    owned.clear();
                    state.animation.room = None;
                    changed = true;
                }
                for room in rooms {
                    if ui.selectable_label(&owned == room, room).clicked() {
                        state.animation.room = Some(room.clone());
                        changed = true;
                    }
                }
            });
    });

    ui.add_space(6.0);
    ui.add(
        egui::Slider::new(&mut state.scale, 1.0..=8.0)
            .text(t("sheet zoom", "lupa archu")),
    );

    if changed {
        state.dirty = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn editor() -> AssetEditor {
        let animation =
            Animation::new("walk", "sprites/walk.png", 4, Vec2::new(32.0, 32.0));
        AssetEditor::open(animation, Vec2::new(128.0, 32.0))
    }

    #[test]
    fn opening_remembers_what_it_was_called() {
        let mut state = editor();
        state.animation.id = "walk_left".into();
        assert_eq!(state.original_id, "walk", "the rename has to replace the old entry");
    }

    #[test]
    fn the_cut_cannot_promise_frames_the_sheet_lacks() {
        let mut state = editor();
        assert_eq!(state.max_frames(), 4);
        state.animation.frame_size = Vec2::new(64.0, 32.0);
        assert_eq!(state.max_frames(), 2);
        state.animation.gap = Vec2::new(8.0, 0.0);
        // Two 64-wide frames with 8 between span 136 > 128.
        assert_eq!(state.max_frames(), 1);
    }

    #[test]
    fn a_grid_counts_rows_as_well_as_columns() {
        let mut state = editor();
        state.sheet_size = Vec2::new(128.0, 64.0);
        state.animation.columns = 4;
        assert_eq!(state.max_frames(), 8);
    }

    #[test]
    fn a_paused_preview_does_not_advance() {
        let mut state = editor();
        state.playing = false;
        state.tick(1.0);
        assert_eq!(state.time, 0.0);
        state.playing = true;
        state.tick(0.5);
        assert!(state.time > 0.0);
    }

    #[test]
    fn actions_default_to_doing_nothing() {
        let actions = AssetActions::default();
        assert!(!actions.save && !actions.close);
    }
}
