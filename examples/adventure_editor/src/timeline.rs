//! A59.2 / A86: the scene as tracks of time you can drag.
//!
//! The script stays a sequence with waits - the right model to run -
//! and this panel is a view of it laid on a clock. Since A86 the view
//! reads like a film sheet: each step is a clip on the track of the
//! thing it drives - camera, pictures, each animation, each speaker,
//! sounds, music, and a Steps row for the waits and branches that
//! steer. Who does what when, in one glance.
//!
//! A playhead runs across it while the scene plays. Clips whose length
//! is a number in the script - a wait, a pan, a still - can be dragged
//! by their right edge, and the rest cannot, because offering to
//! stretch a line whose length comes from its own text would be
//! offering an edit the editor cannot make. Dragging a clip's body
//! moves the step to the moment under the cursor - among its siblings,
//! since order in the sequence is what "when" means here.
//!
//! It is also where steps are added. The Add menu drops a sound, an
//! image with a duration, an animation, a wait or a camera move after
//! whatever is selected; dragging from the library lands the step at
//! the moment it was dropped. The selected step's details sit in a
//! column beside the tracks where they can be changed and the step
//! removed.

use glam::Vec2;
use infengine_adventure::action::Action;
use infengine_adventure::schedule::{Slot, Timeline};

use crate::tracks::Track;

/// Height of one track row, in points.
const ROW: f32 = 22.0;
/// Pixels a second takes, before zoom.
const PER_SECOND: f32 = 40.0;
/// Shortest a clip is drawn, so a step that takes no time is still a
/// thing you can see and click rather than a line of zero width.
const MIN_BAR: f32 = 6.0;
/// Width of the details column.
const DETAILS: f32 = 250.0;
/// Width of the track-name column.
const NAMES: f32 = 130.0;

/// What the timeline asked for.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct TimelineActions {
    /// A step's length was dragged to a new number of seconds.
    pub set_seconds: Option<(Vec<usize>, f32)>,
    /// A step was clicked.
    pub selected: Option<Vec<usize>>,
    /// A new step, to go after the path - or at the end for `None`.
    pub insert: Option<(Option<Vec<usize>>, Action)>,
    /// A new step to go before everything: a drop ahead of the scene.
    pub insert_first: Option<Action>,
    /// The selected step, edited in the details column.
    pub edit: Option<(Vec<usize>, Action)>,
    /// The selected step, removed.
    pub remove: Option<Vec<usize>>,
    /// A step dragged to a new place among its siblings.
    pub reorder: Option<(Vec<usize>, usize)>,
    /// The play button: run this scene, and stop it.
    pub play: bool,
    pub stop: bool,
    /// Freeze the scene mid-run, and let it go again.
    pub pause: Option<bool>,
    /// The scrubber: put the scene at this many seconds, sight only.
    pub seek: Option<f32>,
    /// A different scene of this room to lay out and play.
    pub pick_scene: Option<String>,
    /// A sound or track the details column asked to hear.
    pub hear: Option<String>,
    /// An animation the details column asked to open on the workbench.
    pub edit_animation: Option<String>,
}

/// What a dragged bar or edge snaps to. Both are the author's to turn
/// off - snapping that cannot be escaped is a ruler taped to the hand.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Snap {
    /// Other steps' starts and ends.
    pub parts: bool,
    /// The time grid: tenths when zoomed in, halves otherwise.
    pub time: bool,
}

impl Default for Snap {
    fn default() -> Self {
        Self { parts: true, time: true }
    }
}

/// An asset dragged out of the library, on its way to becoming a step.
#[derive(Debug, Clone, PartialEq)]
pub enum DragAsset {
    Image(String),
    Sound(String),
    Animation(String),
}

impl DragAsset {
    /// The step dropping this creates, with a usable default.
    pub fn action(&self) -> Action {
        match self {
            DragAsset::Image(image) => Action::ShowImage {
                image: image.clone(),
                seconds: 2.0,
            },
            DragAsset::Sound(sound) => Action::PlaySound {
                sound: sound.clone(),
            },
            DragAsset::Animation(animation) => Action::PlayAnim {
                animation: animation.clone(),
                wait: true,
                at: None,
            },
        }
    }

}

/// The ids a detail field can offer instead of a blank box.
///
/// Typing an animation's name from memory is how a scene ends up
/// pointing at nothing; picking it from what the project has is not.
#[derive(Debug, Clone, Copy, Default)]
pub struct Pickers<'a> {
    pub animations: &'a [String],
    pub sounds: &'a [String],
    pub rooms: &'a [String],
    pub images: &'a [String],
}

/// Draw the tracks. Returns what was asked for.
#[allow(clippy::too_many_arguments)]
pub fn panel(
    ctx: &egui::Context,
    line: &Timeline,
    tracks: &[Track],
    scene: &str,
    scenes: &[String],
    playing: bool,
    paused: bool,
    elapsed: f32,
    zoom: &mut f32,
    snap: &mut Snap,
    selected: &Option<Vec<usize>>,
    reveal: Option<&[usize]>,
    detail: Option<&Action>,
    pickers: &Pickers<'_>,
) -> TimelineActions {
    let mut actions = TimelineActions::default();
    egui::TopBottomPanel::bottom("timeline")
        .resizable(true)
        .default_height(200.0)
        .show(ctx, |ui| {
            header(
                ui, line, scene, scenes, playing, paused, elapsed, zoom, snap, selected,
                pickers, &mut actions,
            );
            ui.separator();
            if line.slots.is_empty() {
                ui.label(
                    egui::RichText::new(
                        "Nothing to lay out. Play a scene, add a step, or drag \
                         something in from the Library.",
                    )
                    .weak(),
                );
                return;
            }
            ui.horizontal_top(|ui| {
                let strip_width = if detail.is_some() {
                    (ui.available_width() - DETAILS).max(120.0)
                } else {
                    ui.available_width()
                };
                ui.scope(|ui| {
                    ui.set_max_width(strip_width);
                    ui.horizontal_top(|ui| {
                        names_column(ui, tracks);
                        // The wheel belongs to zoom here, so the scroll
                        // area keeps its drag and gives the wheel up.
                        egui::ScrollArea::horizontal()
                            .id_salt("timeline_scroll")
                            .auto_shrink([false, false])
                            .scroll_source(egui::containers::scroll_area::ScrollSource {
                                mouse_wheel: false,
                                ..Default::default()
                            })
                            .show(ui, |ui| {
                                strip(
                                    ui, line, tracks, playing, elapsed, zoom, *snap,
                                    selected, reveal, &mut actions,
                                );
                            });
                    });
                });
                if let (Some(action), Some(path)) = (detail, selected.as_ref()) {
                    ui.separator();
                    details(ui, line, path, action, pickers, &mut actions);
                }
            });
        });
    actions
}

/// The fixed column naming each track, aligned with the rows beside it.
///
/// Outside the horizontal scroll on purpose: the names are the map, and
/// a map that scrolls away with the territory is no map.
fn names_column(ui: &mut egui::Ui, tracks: &[Track]) {
    ui.vertical(|ui| {
        ui.set_width(NAMES);
        ui.spacing_mut().item_spacing.y = 0.0;
        // Under the ruler row, so name N sits beside track N.
        ui.add_space(ROW);
        for track in tracks {
            ui.allocate_ui_with_layout(
                egui::vec2(NAMES, ROW),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.add_space(4.0);
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
                    ui.painter().rect_filled(rect, 2.0, track.kind.fill());
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new(&track.name)
                            .small()
                            .color(ui.visuals().strong_text_color()),
                    )
                    .on_hover_text(format!("{} step{}", track.slots.len(),
                        if track.slots.len() == 1 { "" } else { "s" }));
                },
            );
        }
    });
}

#[allow(clippy::too_many_arguments)]
fn header(
    ui: &mut egui::Ui,
    line: &Timeline,
    scene: &str,
    scenes: &[String],
    playing: bool,
    paused: bool,
    elapsed: f32,
    zoom: &mut f32,
    snap: &mut Snap,
    selected: &Option<Vec<usize>>,
    pickers: &Pickers<'_>,
    actions: &mut TimelineActions,
) {
    use crate::lang::t;
    ui.horizontal(|ui| {
        ui.heading(t("Timeline", "Časová osa"));
        // Which of the room's scenes is on the tracks. The answer to
        // "what am I editing here": one named scene of this room, and
        // this is where it is picked.
        egui::ComboBox::from_id_salt("timeline_scene")
            .selected_text(scene)
            .width(180.0)
            .show_ui(ui, |ui| {
                for id in scenes {
                    if ui
                        .selectable_label(id == scene, id)
                        .clicked()
                    {
                        actions.pick_scene = Some(id.clone());
                    }
                }
            })
            .response
            .on_hover_text(t(
                "The room's scenes - pick which one to lay out and play",
                "Scény této místnosti - vyber, kterou skládat a přehrávat",
            ));
        // The scene plays from here: the strip is where it is tuned,
        // so the transport lives on the strip - play, pause, stop, and
        // the scrubber that goes anywhere.
        if playing {
            if paused {
                if ui
                    .button("▶")
                    .on_hover_text(t("Let the scene run on", "Nech scénu běžet dál"))
                    .clicked()
                {
                    actions.pause = Some(false);
                }
            } else if ui
                .button("⏸")
                .on_hover_text(t("Freeze the scene here", "Zmraz scénu tady"))
                .clicked()
            {
                actions.pause = Some(true);
            }
            if ui.button("⏹").on_hover_text(t("Stop", "Zastavit")).clicked() {
                actions.stop = true;
            }
        } else if ui
            .button(t("▶ Play", "▶ Přehrát"))
            .on_hover_text(t("Run this scene from the start", "Spusť scénu od začátku"))
            .clicked()
        {
            actions.play = true;
        }

        // The scrubber. Dragging it puts the scene at that moment,
        // silently; with nothing running it starts the scene paused
        // there, so a scene can be inspected without pressing play.
        // While the hand is on it the knob follows the hand, not the
        // clock - the dragged value is parked in temp data and the
        // seek fires once, on release.
        let drag_id = egui::Id::new("timeline_scrub");
        let held = ui.ctx().data(|d| d.get_temp::<f32>(drag_id));
        let mut at = held.unwrap_or(if playing { elapsed } else { 0.0 });
        let end = line.seconds.max(0.1);
        let scrub = ui.add(
            egui::Slider::new(&mut at, 0.0..=end)
                .show_value(false)
                .trailing_fill(true),
        );
        if scrub.drag_stopped() {
            ui.ctx().data_mut(|d| d.remove::<f32>(drag_id));
            actions.seek = Some(at.min(line.seconds));
        } else if scrub.dragged() {
            ui.ctx().data_mut(|d| d.insert_temp(drag_id, at));
        } else if scrub.changed() {
            // A click on the rail rather than a drag of the knob.
            actions.seek = Some(at.min(line.seconds));
        }
        ui.label(
            egui::RichText::new(format!("{elapsed:.1}s / {:.1}s", line.seconds))
                .monospace()
                .weak(),
        );
        add_menu(ui, selected, pickers, actions);
        ui.add(
            egui::Slider::new(zoom, 0.25..=4.0)
                .show_value(false)
                .text(t("zoom", "lupa")),
        );
        ui.label(egui::RichText::new(t("snap", "přichytávat")).small().weak());
        ui.checkbox(&mut snap.parts, t("steps", "ke krokům"))
            .on_hover_text(t(
                "Dragged edges land on other steps' starts and ends",
                "Tažené hrany dosednou na začátky a konce jiných kroků",
            ));
        ui.checkbox(&mut snap.time, t("time", "k času"))
            .on_hover_text(t(
                "Dragged edges land on the time grid",
                "Tažené hrany dosednou na časovou mřížku",
            ));
        ui.label(
            egui::RichText::new(t(
                "wheel zooms · drag a clip to move it · drag from the Library to add",
                "kolečko přibližuje · klip přesuneš tažením · z Knihovny přidáš přetažením",
            ))
            .small()
            .weak(),
        );
    });
    if !line.untaken.is_empty() {
        // Said rather than hidden: a scene laid out down one side of a
        // choice is half the story, and tuning the half the player may
        // not see is worse than knowing you cannot see it.
        ui.label(
            egui::RichText::new(if crate::lang::is_czech() {
                format!(
                    "{} větví nevykresleno: osa ukazuje jen zvolenou stranu",
                    line.untaken.len()
                )
            } else {
                format!(
                    "{} branch{} not shown: only the taken side is laid out",
                    line.untaken.len(),
                    if line.untaken.len() == 1 { "" } else { "es" }
                )
            })
            .small()
            .color(egui::Color32::from_rgb(255, 200, 120)),
        );
    }
}

/// The menu that puts a new step on the strip.
///
/// Each entry arrives with a usable default - the first sound the
/// project has, two seconds of image - so the new bar does something
/// before anyone touches it, and the details column is for adjusting
/// rather than for rescuing.
fn add_menu(
    ui: &mut egui::Ui,
    selected: &Option<Vec<usize>>,
    pickers: &Pickers<'_>,
    actions: &mut TimelineActions,
) {
    use crate::lang::t;
    ui.menu_button(t("Add step", "Přidat krok"), |ui| {
        let first = |list: &[String]| list.first().cloned().unwrap_or_default();
        let entries: [(&str, &str, Action); 7] = [
            (
                t("Wait", "Čekání"),
                t("A pause of so many seconds", "Pauza na daný počet sekund"),
                Action::Wait { seconds: 1.0 },
            ),
            (
                t("Image", "Obrázek"),
                t("Show a picture for a while", "Ukaž na chvíli obrázek"),
                Action::ShowImage {
                    image: first(pickers.images),
                    seconds: 2.0,
                },
            ),
            (
                t("Sound", "Zvuk"),
                t("Play a sound once", "Přehraj zvuk jednou"),
                Action::PlaySound {
                    sound: first(pickers.sounds),
                },
            ),
            (
                t("Animation", "Animace"),
                t("Play an animation and wait for it", "Přehraj animaci a počkej na ni"),
                Action::PlayAnim {
                    animation: first(pickers.animations),
                    wait: true,
                    at: None,
                },
            ),
            (
                t("Camera move", "Pohyb kamery"),
                t("Pan the view somewhere", "Přejeď pohledem jinam"),
                Action::MoveCamera {
                    to: Vec2::ZERO,
                    seconds: 1.0,
                },
            ),
            (
                t("Line", "Replika"),
                t("Someone says something", "Někdo něco řekne"),
                Action::Say {
                    line: String::new(),
                    speaker: None,
                    wait: true,
                },
            ),
            (
                t("Go to room", "Přechod do místnosti"),
                t(
                    "End the scene somewhere else - how an intro hands over to the game",
                    "Ukonči scénu jinde - takhle intro předá hru",
                ),
                Action::GoToRoom {
                    room: first(pickers.rooms),
                    entry: None,
                },
            ),
        ];
        for (label, hint, action) in entries {
            if ui.button(label).on_hover_text(hint).clicked() {
                actions.insert = Some((selected.clone(), action));
                ui.close();
            }
        }
    })
    .response
    .on_hover_text(t(
        "Put a new step after the selected one",
        "Vlož nový krok za vybraný",
    ));
}

#[allow(clippy::too_many_arguments)]
fn strip(
    ui: &mut egui::Ui,
    line: &Timeline,
    tracks: &[Track],
    playing: bool,
    elapsed: f32,
    zoom: &mut f32,
    snap: Snap,
    selected: &Option<Vec<usize>>,
    reveal: Option<&[usize]>,
    actions: &mut TimelineActions,
) {
    let scale = PER_SECOND * zoom.max(0.05);
    // Room past the end, so there is always future to scroll into and
    // drop things onto - a strip exactly as long as its content says
    // the scene is finished, and no scene is.
    let width = ((line.seconds.max(1.0) + 30.0) * scale).max(ui.available_width());
    let height = (ROW * tracks.len() as f32 + ROW).max(ui.available_height());
    let (rect, surface) =
        ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::click_and_drag());
    let painter = ui.painter_at(ui.clip_rect().intersect(rect));

    // The middle button drags the strip around, the way every video
    // editor pans.
    if surface.dragged_by(egui::PointerButton::Middle) {
        ui.scroll_with_delta(surface.drag_delta());
    }

    // The wheel is zoom, anchored under the pointer: the moment being
    // looked at stays put while the scale changes around it.
    if let Some(pointer) = surface.hover_pos() {
        let scroll = ui.input(|i| i.raw_scroll_delta.y);
        if scroll != 0.0 {
            let at = (pointer.x - rect.min.x) / scale;
            let factor = (scroll * 0.002).exp();
            let new_zoom = (*zoom * factor).clamp(0.25, 4.0);
            let shift = at * (PER_SECOND * new_zoom - scale);
            *zoom = new_zoom;
            ui.scroll_with_delta(egui::vec2(-shift, 0.0));
        }
    }

    grid(&painter, rect, scale, tracks.len());
    ruler(&painter, rect, scale, ui.visuals().weak_text_color());

    // The moment under the pointer, for drops and body-drags: on a
    // track view "where" means "when".
    let hover_asset = surface.dnd_hover_payload::<DragAsset>();
    let dropped_asset = surface.dnd_release_payload::<DragAsset>();
    let pointer_time = ui
        .ctx()
        .pointer_interact_pos()
        .filter(|p| rect.contains(*p))
        .map(|p| ((p.x - rect.min.x) / scale).max(0.0));

    let mut dragging: Option<&Slot> = None;
    for (row, track) in tracks.iter().enumerate() {
        let top = rect.min.y + ROW * (row as f32 + 1.0);
        for &index in &track.slots {
            let Some(slot) = line.slots.get(index) else {
                continue;
            };
            let left = rect.min.x + slot.at * scale;
            let bar = egui::Rect::from_min_size(
                egui::pos2(left, top + 2.0),
                egui::vec2((slot.seconds * scale).max(MIN_BAR), ROW - 5.0),
            );
            if reveal == Some(slot.path.as_slice()) {
                ui.scroll_to_rect(bar.expand(24.0), None);
            }
            if draw_bar(
                ui, &painter, bar, slot, track.kind.fill(), scale, snap, line, selected,
                actions,
            ) {
                dragging = Some(slot);
            }
        }
    }

    // A clip being dragged by its body moves to the moment under the
    // cursor - among its own siblings, because an if-branch is a drawer
    // and a step does not fall out of one by passing over it.
    if let (Some(slot), Some(t)) = (dragging, pointer_time) {
        let x = rect.min.x + t * scale;
        painter.line_segment(
            [egui::pos2(x, rect.min.y), egui::pos2(x, rect.max.y)],
            egui::Stroke::new(2.0, egui::Color32::from_rgb(255, 235, 140)),
        );
        if ui.input(|i| i.pointer.any_released()) {
            let siblings: Vec<&Slot> = line
                .slots
                .iter()
                .filter(|s| parent_of(&s.path) == parent_of(&slot.path))
                .collect();
            let already = siblings
                .iter()
                .position(|s| s.path == slot.path)
                .unwrap_or(0);
            let to = siblings
                .iter()
                .filter(|s| s.path != slot.path && s.at < t)
                .count();
            if to != already {
                actions.reorder = Some((slot.path.clone(), to));
            }
        }
    }

    // Something from the library, hovering or landing: a caret at the
    // moment it would happen, and the insertion on release - after the
    // last step already under way by then, or ahead of everything.
    if (hover_asset.is_some() || dropped_asset.is_some())
        && let Some(t) = pointer_time
    {
        let x = rect.min.x + t * scale;
        painter.line_segment(
            [egui::pos2(x, rect.min.y), egui::pos2(x, rect.max.y)],
            egui::Stroke::new(2.0, egui::Color32::from_rgb(150, 230, 150)),
        );
        if let Some(asset) = dropped_asset {
            let mut after: Option<Vec<usize>> = None;
            for slot in &line.slots {
                if slot.at <= t {
                    after = Some(slot.path.clone());
                }
            }
            match after {
                Some(path) => actions.insert = Some((Some(path), asset.action())),
                None => actions.insert_first = Some(asset.action()),
            }
        }
    }

    if playing {
        let x = rect.min.x + elapsed * scale;
        if x <= rect.max.x {
            painter.line_segment(
                [egui::pos2(x, rect.min.y), egui::pos2(x, rect.max.y)],
                egui::Stroke::new(1.5, egui::Color32::from_rgb(255, 235, 140)),
            );
        }
    }
}

/// A path is its parent and one more step.
fn parent_of(path: &[usize]) -> &[usize] {
    path.split_last().map(|(_, prefix)| prefix).unwrap_or(&[])
}

/// The faint lines that make the strip read as a grid: seconds down,
/// rows across.
fn grid(painter: &egui::Painter, rect: egui::Rect, scale: f32, rows: usize) {
    let faint = egui::Color32::from_white_alpha(10);
    let step = if scale >= 30.0 {
        1.0
    } else if scale >= 8.0 {
        5.0
    } else {
        10.0
    };
    let mut at = 0.0f32;
    loop {
        let x = rect.min.x + at * scale;
        if x > rect.max.x {
            break;
        }
        painter.line_segment(
            [egui::pos2(x, rect.min.y + ROW * 0.5), egui::pos2(x, rect.max.y)],
            egui::Stroke::new(0.5, faint),
        );
        at += step;
    }
    for index in 0..=rows {
        let y = rect.min.y + ROW * (index as f32 + 1.0);
        painter.line_segment(
            [egui::pos2(rect.min.x, y), egui::pos2(rect.max.x, y)],
            egui::Stroke::new(0.5, faint),
        );
    }
}

/// Second marks along the top, to the full width - time does not stop
/// where the content happens to.
fn ruler(painter: &egui::Painter, rect: egui::Rect, scale: f32, colour: egui::Color32) {
    let step = if scale >= 30.0 {
        1.0
    } else if scale >= 8.0 {
        5.0
    } else {
        10.0
    };
    let mut at = 0.0f32;
    loop {
        let x = rect.min.x + at * scale;
        if x > rect.max.x {
            break;
        }
        painter.line_segment(
            [egui::pos2(x, rect.min.y), egui::pos2(x, rect.min.y + ROW * 0.5)],
            egui::Stroke::new(1.0, colour),
        );
        painter.text(
            egui::pos2(x + 3.0, rect.min.y),
            egui::Align2::LEFT_TOP,
            format!("{at:.0}s"),
            egui::FontId::monospace(10.0),
            colour,
        );
        at += step;
    }
}

/// Snap a dragged end-time to the strip's other steps, then the grid.
fn snapped(seconds: f32, slot: &Slot, line: &Timeline, scale: f32, snap: Snap) -> f32 {
    let end = slot.at + seconds;
    let threshold = 8.0 / scale.max(0.01);
    if snap.parts {
        let mut best: Option<f32> = None;
        for other in line.slots.iter().filter(|s| s.path != slot.path) {
            for edge in [other.at, other.at + other.seconds] {
                if (edge - end).abs() < threshold
                    && edge > slot.at
                    && best.is_none_or(|b| (edge - end).abs() < (b - end).abs())
                {
                    best = Some(edge);
                }
            }
        }
        if let Some(edge) = best {
            return edge - slot.at;
        }
    }
    if snap.time {
        let step = if scale >= 80.0 { 0.1 } else { 0.5 };
        let landed = (end / step).round() * step - slot.at;
        if landed > 0.0 {
            return landed;
        }
    }
    seconds
}

/// One clip. Returns whether its body is being dragged to a new place.
#[allow(clippy::too_many_arguments)]
fn draw_bar(
    ui: &mut egui::Ui,
    painter: &egui::Painter,
    bar: egui::Rect,
    slot: &Slot,
    tint: egui::Color32,
    scale: f32,
    snap: Snap,
    line: &Timeline,
    selected: &Option<Vec<usize>>,
    actions: &mut TimelineActions,
) -> bool {
    let is_selected = selected.as_ref() == Some(&slot.path);
    let response = ui.interact(
        bar,
        egui::Id::new(("timeline_bar", &slot.path)),
        egui::Sense::click_and_drag(),
    );
    if response.clicked() || response.drag_started() {
        actions.selected = Some(slot.path.clone());
    }
    let moving = response.dragged();
    if moving {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    }

    let fill = if is_selected {
        egui::Color32::from_rgb(90, 130, 190)
    } else if slot.adjustable {
        tint
    } else {
        // A step whose length is not its own is drawn dimmer, so the
        // ones that can be dragged are the ones that look draggable.
        tint.gamma_multiply(0.7)
    };
    let fill = if moving { fill.gamma_multiply(1.3) } else { fill };
    painter.rect_filled(bar, 3.0, fill);
    painter.text(
        bar.left_center() + egui::vec2(5.0, 0.0),
        egui::Align2::LEFT_CENTER,
        &slot.label,
        egui::FontId::proportional(12.0),
        egui::Color32::from_rgb(235, 235, 235),
    );

    if !slot.adjustable {
        return moving;
    }
    // The right edge is the handle, because what is being changed is
    // where the step ends rather than where it starts - a step's start
    // is whatever came before it, and dragging that would mean moving
    // everything.
    let handle = egui::Rect::from_min_size(
        egui::pos2(bar.right() - 4.0, bar.top()),
        egui::vec2(8.0, bar.height()),
    );
    let drag = ui.interact(
        handle,
        egui::Id::new(("timeline_handle", &slot.path)),
        egui::Sense::drag(),
    );
    painter.rect_filled(
        handle,
        1.0,
        if drag.hovered() || drag.dragged() {
            egui::Color32::from_rgb(255, 235, 140)
        } else {
            egui::Color32::from_rgb(150, 165, 195)
        },
    );
    if drag.dragged() {
        ui.ctx()
            .set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    }
    if drag.drag_delta().x != 0.0 {
        let seconds = (slot.seconds + drag.drag_delta().x / scale.max(0.01)).max(0.0);
        actions.set_seconds =
            Some((slot.path.clone(), snapped(seconds, slot, line, scale, snap)));
    }
    moving && !drag.dragged()
}

/// The selected step, spelled out where it can be changed.
fn details(
    ui: &mut egui::Ui,
    line: &Timeline,
    path: &[usize],
    action: &Action,
    pickers: &Pickers<'_>,
    actions: &mut TimelineActions,
) {
    ui.vertical(|ui| {
        ui.set_width(DETAILS - 16.0);
        ui.strong(crate::lang::t("Step", "Krok"));
        if let Some(slot) = line.slots.iter().find(|slot| slot.path == path) {
            ui.label(
                egui::RichText::new(if crate::lang::is_czech() {
                    format!("začíná {:.1}s, trvá {:.1}s", slot.at, slot.seconds)
                } else {
                    format!("starts {:.1}s, runs {:.1}s", slot.at, slot.seconds)
                })
                .small()
                .monospace()
                .weak(),
            );
        }
        ui.add_space(4.0);

        let mut edited = action.clone();
        let mut changed = false;
        match &mut edited {
            Action::Wait { seconds } => {
                changed |= drag_seconds(ui, "seconds", seconds);
            }
            Action::MoveCamera { to, seconds } => {
                ui.horizontal(|ui| {
                    ui.label("to");
                    changed |= ui.add(egui::DragValue::new(&mut to.x).speed(1.0)).changed();
                    changed |= ui.add(egui::DragValue::new(&mut to.y).speed(1.0)).changed();
                });
                changed |= drag_seconds(ui, "over", seconds);
            }
            Action::ShowImage { image, seconds } => {
                changed |= pick(ui, "image", image, pickers.images);
                changed |= drag_seconds(ui, "for", seconds);
            }
            Action::PlaySound { sound } => {
                changed |= pick(ui, "sound", sound, pickers.sounds);
                if ui
                    .small_button("▶ Hear")
                    .on_hover_text("Play it now, so the pick can be made by ear")
                    .clicked()
                {
                    actions.hear = Some(sound.clone());
                }
            }
            Action::PlayMusic { music } => {
                let mut id = music.clone().unwrap_or_default();
                if pick(ui, "music", &mut id, pickers.sounds) {
                    *music = (!id.is_empty()).then_some(id);
                    changed = true;
                }
                if let Some(id) = music.as_ref()
                    && ui
                        .small_button("▶ Hear")
                        .on_hover_text("Play it now, so the pick can be made by ear")
                        .clicked()
                {
                    actions.hear = Some(id.clone());
                }
            }
            Action::PlayAnim { animation, wait, at } => {
                changed |= pick(ui, "animation", animation, pickers.animations);
                if !animation.is_empty()
                    && ui
                        .small_button(crate::lang::t("✎ Edit", "✎ Upravit"))
                        .on_hover_text(crate::lang::t(
                            "Open it on the workbench: the sheet, the cut, the timing",
                            "Otevři ji na pracovním stole: arch, řez, časování",
                        ))
                        .clicked()
                {
                    actions.edit_animation = Some(animation.clone());
                }
                changed |= ui.checkbox(wait, "wait for it").changed();
                let mut placed = at.is_some();
                if ui
                    .checkbox(&mut placed, "place it")
                    .on_hover_text("Otherwise the animation says where it goes")
                    .changed()
                {
                    *at = placed.then_some(Vec2::ZERO);
                    changed = true;
                }
                if let Some(at) = at {
                    ui.horizontal(|ui| {
                        ui.label("at");
                        changed |= ui.add(egui::DragValue::new(&mut at.x).speed(1.0)).changed();
                        changed |= ui.add(egui::DragValue::new(&mut at.y).speed(1.0)).changed();
                    });
                }
            }
            Action::Say { line, .. } => {
                ui.label("line id");
                changed |= ui.text_edit_singleline(line).changed();
            }
            Action::GoToRoom { room, .. } => {
                changed |= pick(ui, "room", room, pickers.rooms);
            }
            other => {
                // A branch is edited in the script graph, where both of
                // its sides are visible; pretending to edit it here
                // would edit half of it.
                ui.label(egui::RichText::new(other.label()).weak());
            }
        }
        if changed {
            actions.edit = Some((path.to_vec(), edited));
        }

        ui.add_space(8.0);
        if ui
            .button(crate::lang::t("Remove step", "Odebrat krok"))
            .clicked()
        {
            actions.remove = Some(path.to_vec());
        }
    });
}

fn drag_seconds(ui: &mut egui::Ui, label: &str, seconds: &mut f32) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(label);
        changed = ui
            .add(
                egui::DragValue::new(seconds)
                    .range(0.0..=600.0)
                    .speed(0.1)
                    .suffix("s"),
            )
            .changed();
    });
    changed
}

/// A combo of what the project has, or a text box when it has nothing.
fn pick(ui: &mut egui::Ui, label: &str, value: &mut String, options: &[String]) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(label);
        if options.is_empty() {
            changed = ui.text_edit_singleline(value).changed();
            return;
        }
        egui::ComboBox::from_id_salt(("timeline_pick", label))
            .selected_text(if value.is_empty() { "-" } else { value.as_str() })
            .width(150.0)
            .show_ui(ui, |ui| {
                for option in options {
                    if ui
                        .selectable_value(value, option.clone(), option)
                        .changed()
                    {
                        changed = true;
                    }
                }
            });
    });
    changed
}

// ---------------------------------------------------------------------
// Editing a script by a slot's path. The path addresses `then` sides
// only, because that is the only side the timeline lays out.

/// The action a path names.
pub fn action_at<'a>(actions: &'a [Action], path: &[usize]) -> Option<&'a Action> {
    let (&index, rest) = path.split_first()?;
    let action = actions.get(index)?;
    if rest.is_empty() {
        return Some(action);
    }
    match action {
        Action::Branch { then, .. } => action_at(then, rest),
        _ => None,
    }
}

/// The list a path's parent is, for inserting siblings.
fn list_mut<'a>(actions: &'a mut Vec<Action>, prefix: &[usize]) -> Option<&'a mut Vec<Action>> {
    let Some((&index, rest)) = prefix.split_first() else {
        return Some(actions);
    };
    match actions.get_mut(index)? {
        Action::Branch { then, .. } => list_mut(then, rest),
        _ => None,
    }
}

/// Put a step ahead of everything. Returns where it landed.
///
/// The one place `insert_after` cannot reach: a drop at a moment
/// before any step has begun means "this happens first".
pub fn insert_front(actions: &mut Vec<Action>, action: Action) -> Vec<usize> {
    actions.insert(0, action);
    vec![0]
}

/// Put a step after `after`, or at the end for `None`. Returns where it
/// landed, so the caller can select it.
pub fn insert_after(
    actions: &mut Vec<Action>,
    after: Option<&[usize]>,
    action: Action,
) -> Vec<usize> {
    if let Some(path) = after
        && let Some((&last, prefix)) = path.split_last()
        && let Some(list) = list_mut(actions, prefix)
    {
        let index = (last + 1).min(list.len());
        list.insert(index, action);
        let mut landed = prefix.to_vec();
        landed.push(index);
        return landed;
    }
    // No selection, or a stale path: the end of the script is always a
    // place a step can go.
    actions.push(action);
    vec![actions.len() - 1]
}

/// Take the step a path names out.
pub fn remove(actions: &mut Vec<Action>, path: &[usize]) -> bool {
    let Some((&last, prefix)) = path.split_last() else {
        return false;
    };
    let Some(list) = list_mut(actions, prefix) else {
        return false;
    };
    if last < list.len() {
        list.remove(last);
        true
    } else {
        false
    }
}

/// Replace the step a path names with an edited copy.
pub fn replace(actions: &mut Vec<Action>, path: &[usize], action: Action) -> bool {
    let Some((&last, prefix)) = path.split_last() else {
        return false;
    };
    let Some(list) = list_mut(actions, prefix) else {
        return false;
    };
    match list.get_mut(last) {
        Some(slot) => {
            *slot = action;
            true
        }
        None => false,
    }
}

/// Move a step to a new place among its siblings. Returns where it
/// landed, so the caller can keep it selected.
pub fn reorder(actions: &mut Vec<Action>, from: &[usize], to: usize) -> Option<Vec<usize>> {
    let (&last, prefix) = from.split_last()?;
    let list = list_mut(actions, prefix)?;
    if last >= list.len() {
        return None;
    }
    let step = list.remove(last);
    let to = to.min(list.len());
    list.insert(to, step);
    let mut landed = prefix.to_vec();
    landed.push(to);
    Some(landed)
}

/// Set the length of the action a slot's path names.
///
/// Returns whether anything changed, so the caller only marks the
/// project dirty when it did.
pub fn set_seconds(actions: &mut Vec<Action>, path: &[usize], seconds: f32) -> bool {
    let Some((&last, prefix)) = path.split_last() else {
        return false;
    };
    let Some(list) = list_mut(actions, prefix) else {
        return false;
    };
    match list.get_mut(last) {
        Some(
            Action::Wait { seconds: value }
            | Action::MoveCamera { seconds: value, .. }
            | Action::ShowImage { seconds: value, .. },
        ) => {
            // Rounded, so dragging gives a number worth reading in the
            // file rather than 1.4732 seconds nobody typed.
            let rounded = (seconds.max(0.0) * 10.0).round() / 10.0;
            let changed = (*value - rounded).abs() > f32::EPSILON;
            *value = rounded;
            changed
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use infengine_adventure::action::Condition;

    #[test]
    fn a_dragged_step_lands_among_its_siblings() {
        let mut actions = vec![
            Action::Wait { seconds: 1.0 },
            Action::Wait { seconds: 2.0 },
            Action::Wait { seconds: 3.0 },
        ];
        let landed = reorder(&mut actions, &[0], 2).expect("moved");
        assert_eq!(landed, vec![2]);
        assert_eq!(
            actions,
            vec![
                Action::Wait { seconds: 2.0 },
                Action::Wait { seconds: 3.0 },
                Action::Wait { seconds: 1.0 },
            ]
        );
        // And a stale path refuses rather than moving the wrong step.
        assert!(reorder(&mut actions, &[9], 0).is_none());
    }

    #[test]
    fn a_dropped_asset_becomes_a_usable_step() {
        assert_eq!(
            DragAsset::Image("pictures/x.png".into()).action(),
            Action::ShowImage {
                image: "pictures/x.png".into(),
                seconds: 2.0
            }
        );
        assert!(matches!(
            DragAsset::Sound("kyra.sfx1".into()).action(),
            Action::PlaySound { .. }
        ));
    }

    #[test]
    fn dragging_a_wait_changes_its_seconds() {
        let mut actions = vec![Action::Wait { seconds: 1.0 }];
        assert!(set_seconds(&mut actions, &[0], 2.5));
        assert_eq!(actions[0], Action::Wait { seconds: 2.5 });
    }

    #[test]
    fn a_length_is_rounded_so_dragging_gives_a_number_worth_reading() {
        let mut actions = vec![Action::MoveCamera {
            to: Vec2::ZERO,
            seconds: 1.0,
        }];
        set_seconds(&mut actions, &[0], 1.4732);
        let Action::MoveCamera { seconds, .. } = actions[0] else {
            panic!("still a pan");
        };
        assert_eq!(seconds, 1.5);
    }

    #[test]
    fn a_length_never_goes_negative() {
        let mut actions = vec![Action::Wait { seconds: 1.0 }];
        set_seconds(&mut actions, &[0], -5.0);
        assert_eq!(actions[0], Action::Wait { seconds: 0.0 });
    }

    #[test]
    fn a_still_is_a_length_the_script_states() {
        let mut actions = vec![Action::ShowImage {
            image: "pictures/note.png".into(),
            seconds: 2.0,
        }];
        assert!(set_seconds(&mut actions, &[0], 4.0));
    }

    #[test]
    fn a_step_inside_a_branch_can_be_reached() {
        let mut actions = vec![Action::Branch {
            on: Condition::Flag("f".into()),
            then: vec![Action::Wait { seconds: 1.0 }],
            otherwise: Vec::new(),
        }];
        assert!(set_seconds(&mut actions, &[0, 0], 3.0));
        let Action::Branch { then, .. } = &actions[0] else {
            panic!("still a branch");
        };
        assert_eq!(then[0], Action::Wait { seconds: 3.0 });
    }

    #[test]
    fn a_step_with_no_length_of_its_own_refuses() {
        let mut actions = vec![Action::Say {
            line: "l".into(),
            speaker: None,
            wait: true,
                }];
        assert!(!set_seconds(&mut actions, &[0], 3.0));
    }

    #[test]
    fn a_path_that_leads_nowhere_changes_nothing() {
        let mut actions = vec![Action::Wait { seconds: 1.0 }];
        assert!(!set_seconds(&mut actions, &[9], 3.0));
        assert!(!set_seconds(&mut actions, &[], 3.0));
        assert!(!set_seconds(&mut actions, &[0, 0], 3.0), "a wait has no inside");
        assert_eq!(actions[0], Action::Wait { seconds: 1.0 });
    }

    #[test]
    fn setting_the_same_length_reports_no_change() {
        let mut actions = vec![Action::Wait { seconds: 2.0 }];
        assert!(!set_seconds(&mut actions, &[0], 2.0));
    }

    #[test]
    fn a_new_step_lands_after_the_selected_one() {
        let mut actions = vec![
            Action::Wait { seconds: 1.0 },
            Action::Wait { seconds: 2.0 },
        ];
        let landed = insert_after(&mut actions, Some(&[0]), Action::Wait { seconds: 9.0 });
        assert_eq!(landed, vec![1]);
        assert_eq!(actions[1], Action::Wait { seconds: 9.0 });
        assert_eq!(actions.len(), 3);
    }

    #[test]
    fn with_nothing_selected_a_new_step_goes_at_the_end() {
        let mut actions = vec![Action::Wait { seconds: 1.0 }];
        let landed = insert_after(&mut actions, None, Action::Wait { seconds: 9.0 });
        assert_eq!(landed, vec![1]);
    }

    #[test]
    fn a_drop_before_everything_lands_first() {
        let mut actions = vec![Action::Wait { seconds: 1.0 }];
        let landed = insert_front(&mut actions, Action::Wait { seconds: 9.0 });
        assert_eq!(landed, vec![0]);
        assert_eq!(actions[0], Action::Wait { seconds: 9.0 });
        assert_eq!(actions.len(), 2);
    }

    #[test]
    fn a_new_step_can_land_inside_a_branch() {
        let mut actions = vec![Action::Branch {
            on: Condition::Flag("f".into()),
            then: vec![Action::Wait { seconds: 1.0 }],
            otherwise: Vec::new(),
        }];
        let landed = insert_after(&mut actions, Some(&[0, 0]), Action::Wait { seconds: 9.0 });
        assert_eq!(landed, vec![0, 1]);
        let Action::Branch { then, .. } = &actions[0] else {
            panic!("still a branch");
        };
        assert_eq!(then.len(), 2);
    }

    #[test]
    fn a_stale_selection_still_adds_rather_than_losing_the_step() {
        // The path may point at a step that was just removed; the step
        // being added should not vanish with it.
        let mut actions = vec![Action::Wait { seconds: 1.0 }];
        let landed = insert_after(&mut actions, Some(&[7, 3]), Action::Wait { seconds: 9.0 });
        assert_eq!(landed, vec![1], "fell back to the end");
    }

    #[test]
    fn removing_and_replacing_reach_the_step_they_name() {
        let mut actions = vec![
            Action::Wait { seconds: 1.0 },
            Action::Wait { seconds: 2.0 },
        ];
        assert!(replace(&mut actions, &[1], Action::Wait { seconds: 5.0 }));
        assert_eq!(actions[1], Action::Wait { seconds: 5.0 });
        assert!(remove(&mut actions, &[0]));
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0], Action::Wait { seconds: 5.0 });
        assert!(!remove(&mut actions, &[9]));
    }

    #[test]
    fn a_slot_path_finds_its_action() {
        let actions = vec![Action::Branch {
            on: Condition::Flag("f".into()),
            then: vec![Action::Wait { seconds: 1.0 }],
            otherwise: Vec::new(),
        }];
        assert!(matches!(action_at(&actions, &[0]), Some(Action::Branch { .. })));
        assert_eq!(action_at(&actions, &[0, 0]), Some(&Action::Wait { seconds: 1.0 }));
        assert_eq!(action_at(&actions, &[0, 9]), None);
        assert_eq!(action_at(&actions, &[]), None);
    }
}
