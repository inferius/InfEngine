//! A59.2: the scene as a strip of time you can drag.
//!
//! The Scene tab can run a script and show a clock. That answers "when
//! does this happen" one moment at a time, which is not how anyone tunes
//! a cutscene - you need the whole shape at once, and you need to change
//! it where you can see it.
//!
//! So the script is drawn as bars along a second-ruled strip: each step
//! at the time it starts, as long as it lasts. A playhead runs across it
//! while the scene plays. The bars whose length is a number in the
//! script - a wait, a pan, a still - can be dragged by their right edge,
//! and the rest cannot, because offering to stretch a line whose length
//! comes from its own text would be offering an edit the editor cannot
//! make.
//!
//! It is also where steps are added. The Add menu drops a sound, an
//! image with a duration, an animation, a wait or a camera move after
//! whatever is selected, and the selected step's details sit in a column
//! beside the strip where they can be changed and the step removed. The
//! strip is the scene; the things in the scene should be put there.

use glam::Vec2;
use infengine_adventure::action::Action;
use infengine_adventure::schedule::{Slot, Timeline};

/// Height of one bar, in points.
const ROW: f32 = 22.0;
/// How far a nested branch is stepped in.
const INDENT: f32 = 14.0;
/// Pixels a second takes, before zoom.
const PER_SECOND: f32 = 40.0;
/// Shortest a bar is drawn, so a step that takes no time is still a
/// thing you can see and click rather than a line of zero width.
const MIN_BAR: f32 = 6.0;
/// Width of the details column.
const DETAILS: f32 = 250.0;

/// What the timeline asked for.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct TimelineActions {
    /// A step's length was dragged to a new number of seconds.
    pub set_seconds: Option<(Vec<usize>, f32)>,
    /// A step was clicked.
    pub selected: Option<Vec<usize>>,
    /// A new step, to go after the path - or at the end for `None`.
    pub insert: Option<(Option<Vec<usize>>, Action)>,
    /// The selected step, edited in the details column.
    pub edit: Option<(Vec<usize>, Action)>,
    /// The selected step, removed.
    pub remove: Option<Vec<usize>>,
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

/// Draw the strip. Returns what was asked for.
#[allow(clippy::too_many_arguments)]
pub fn panel(
    ctx: &egui::Context,
    line: &Timeline,
    playing: bool,
    elapsed: f32,
    zoom: &mut f32,
    selected: &Option<Vec<usize>>,
    detail: Option<&Action>,
    pickers: &Pickers<'_>,
) -> TimelineActions {
    let mut actions = TimelineActions::default();
    egui::TopBottomPanel::bottom("timeline")
        .resizable(true)
        .default_height(200.0)
        .show(ctx, |ui| {
            header(ui, line, playing, elapsed, zoom, selected, pickers, &mut actions);
            ui.separator();
            if line.slots.is_empty() {
                ui.label(
                    egui::RichText::new(
                        "Nothing to lay out. Play a scene, or Add a first step.",
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
                    egui::ScrollArea::both()
                        .id_salt("timeline_scroll")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            strip(ui, line, playing, elapsed, *zoom, selected, &mut actions);
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

#[allow(clippy::too_many_arguments)]
fn header(
    ui: &mut egui::Ui,
    line: &Timeline,
    playing: bool,
    elapsed: f32,
    zoom: &mut f32,
    selected: &Option<Vec<usize>>,
    pickers: &Pickers<'_>,
    actions: &mut TimelineActions,
) {
    ui.horizontal(|ui| {
        ui.heading("Timeline");
        ui.label(
            egui::RichText::new(format!("{:.1}s", line.seconds))
                .monospace()
                .weak(),
        );
        if playing {
            ui.label(
                egui::RichText::new(format!("now {elapsed:.1}s"))
                    .monospace()
                    .strong(),
            );
        }
        add_menu(ui, selected, pickers, actions);
        ui.add(
            egui::Slider::new(zoom, 0.25..=4.0)
                .show_value(false)
                .text("zoom"),
        );
    });
    if !line.untaken.is_empty() {
        // Said rather than hidden: a scene laid out down one side of a
        // choice is half the story, and tuning the half the player may
        // not see is worse than knowing you cannot see it.
        ui.label(
            egui::RichText::new(format!(
                "{} branch{} not shown: only the taken side is laid out",
                line.untaken.len(),
                if line.untaken.len() == 1 { "" } else { "es" }
            ))
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
    ui.menu_button("Add step", |ui| {
        let first = |list: &[String]| list.first().cloned().unwrap_or_default();
        let entries: [(&str, &str, Action); 6] = [
            ("Wait", "A pause of so many seconds", Action::Wait { seconds: 1.0 }),
            (
                "Image",
                "Show a picture for a while",
                Action::ShowImage {
                    image: first(pickers.images),
                    seconds: 2.0,
                },
            ),
            (
                "Sound",
                "Play a sound once",
                Action::PlaySound {
                    sound: first(pickers.sounds),
                },
            ),
            (
                "Animation",
                "Play an animation and wait for it",
                Action::PlayAnim {
                    animation: first(pickers.animations),
                    wait: true,
                    at: None,
                },
            ),
            (
                "Camera move",
                "Pan the view somewhere",
                Action::MoveCamera {
                    to: Vec2::ZERO,
                    seconds: 1.0,
                },
            ),
            (
                "Line",
                "Someone says something",
                Action::Say {
                    line: String::new(),
                    speaker: None,
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
    .on_hover_text("Put a new step after the selected one");
}

fn strip(
    ui: &mut egui::Ui,
    line: &Timeline,
    playing: bool,
    elapsed: f32,
    zoom: f32,
    selected: &Option<Vec<usize>>,
    actions: &mut TimelineActions,
) {
    let scale = PER_SECOND * zoom.max(0.05);
    let width = (line.seconds.max(1.0) * scale + 200.0).max(ui.available_width());
    let height = ROW * line.slots.len() as f32 + ROW;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    let painter = ui.painter_at(rect);

    ruler(&painter, rect, line.seconds, scale, ui.visuals().weak_text_color());

    // Row separators, so a bar and its neighbours read as rows rather
    // than as marks floating in space.
    let faint = egui::Color32::from_white_alpha(10);
    for index in 0..=line.slots.len() {
        let y = rect.min.y + ROW * (index as f32 + 1.0);
        painter.line_segment(
            [egui::pos2(rect.min.x, y), egui::pos2(rect.max.x, y)],
            egui::Stroke::new(0.5, faint),
        );
    }

    for (index, slot) in line.slots.iter().enumerate() {
        let top = rect.min.y + ROW * (index as f32 + 1.0);
        let left = rect.min.x + slot.at * scale + slot.depth as f32 * INDENT;
        let bar = egui::Rect::from_min_size(
            egui::pos2(left, top + 2.0),
            egui::vec2((slot.seconds * scale).max(MIN_BAR), ROW - 5.0),
        );
        draw_bar(ui, &painter, bar, slot, selected, actions);
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

/// Second marks along the top.
fn ruler(
    painter: &egui::Painter,
    rect: egui::Rect,
    seconds: f32,
    scale: f32,
    colour: egui::Color32,
) {
    // One mark a second while they are far enough apart to read, then
    // every five, then every ten. A ruler whose numbers overlap is a
    // smear rather than a scale.
    let step = if scale >= 30.0 {
        1.0
    } else if scale >= 8.0 {
        5.0
    } else {
        10.0
    };
    let mut at = 0.0f32;
    while at <= seconds.max(1.0) + step {
        let x = rect.min.x + at * scale;
        if x > rect.max.x {
            break;
        }
        painter.line_segment(
            [
                egui::pos2(x, rect.min.y),
                egui::pos2(x, rect.min.y + ROW * 0.5),
            ],
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

fn draw_bar(
    ui: &mut egui::Ui,
    painter: &egui::Painter,
    bar: egui::Rect,
    slot: &Slot,
    selected: &Option<Vec<usize>>,
    actions: &mut TimelineActions,
) {
    let is_selected = selected.as_ref() == Some(&slot.path);
    let response = ui.interact(
        bar,
        egui::Id::new(("timeline_bar", &slot.path)),
        egui::Sense::click(),
    );
    if response.clicked() {
        actions.selected = Some(slot.path.clone());
    }

    let fill = if is_selected {
        egui::Color32::from_rgb(90, 130, 190)
    } else if slot.adjustable {
        egui::Color32::from_rgb(60, 95, 140)
    } else {
        // A step whose length is not its own is drawn dimmer, so the
        // ones that can be dragged are the ones that look draggable.
        egui::Color32::from_rgb(70, 75, 90)
    };
    painter.rect_filled(bar, 3.0, fill);
    painter.text(
        bar.left_center() + egui::vec2(5.0, 0.0),
        egui::Align2::LEFT_CENTER,
        &slot.label,
        egui::FontId::proportional(12.0),
        egui::Color32::from_rgb(235, 235, 235),
    );

    if !slot.adjustable {
        return;
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
        // Whatever the bar's own scale is, which the caller knows and
        // this does not - so the delta is turned back into seconds by
        // the width the bar already has.
        let per_second = (bar.width() / slot.seconds.max(0.01)).max(1.0);
        let seconds = (slot.seconds + drag.drag_delta().x / per_second).max(0.0);
        actions.set_seconds = Some((slot.path.clone(), seconds));
    }
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
        ui.strong("Step");
        if let Some(slot) = line.slots.iter().find(|slot| slot.path == path) {
            ui.label(
                egui::RichText::new(format!(
                    "starts {:.1}s, runs {:.1}s",
                    slot.at, slot.seconds
                ))
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
            }
            Action::PlayMusic { music } => {
                let mut id = music.clone().unwrap_or_default();
                if pick(ui, "music", &mut id, pickers.sounds) {
                    *music = (!id.is_empty()).then_some(id);
                    changed = true;
                }
            }
            Action::PlayAnim { animation, wait, at } => {
                changed |= pick(ui, "animation", animation, pickers.animations);
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
        if ui.button("Remove step").clicked() {
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
