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
//! script - a wait, a pan - can be dragged by their right edge, and the
//! rest cannot, because offering to stretch a line whose length comes
//! from its own text would be offering an edit the editor cannot make.
//!
//! It sits along the bottom rather than in the inspector because a
//! timeline wants width, and a twenty-second scene squeezed into a
//! 340-pixel column is a row of slivers.

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

/// What the timeline asked for.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct TimelineActions {
    /// A step's length was dragged to a new number of seconds.
    pub set_seconds: Option<(Vec<usize>, f32)>,
    /// A step was clicked.
    pub selected: Option<Vec<usize>>,
}

/// Draw the strip. Returns what was asked for.
pub fn panel(
    ctx: &egui::Context,
    line: &Timeline,
    playing: bool,
    elapsed: f32,
    zoom: &mut f32,
    selected: &Option<Vec<usize>>,
) -> TimelineActions {
    let mut actions = TimelineActions::default();
    egui::TopBottomPanel::bottom("timeline")
        .resizable(true)
        .default_height(180.0)
        .show(ctx, |ui| {
            header(ui, line, playing, elapsed, zoom);
            ui.separator();
            if line.slots.is_empty() {
                ui.label(
                    egui::RichText::new(
                        "Nothing to lay out. Pick a script in the Scene tab.",
                    )
                    .weak(),
                );
                return;
            }
            egui::ScrollArea::both()
                .id_salt("timeline_scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    strip(ui, line, playing, elapsed, *zoom, selected, &mut actions);
                });
        });
    actions
}

fn header(ui: &mut egui::Ui, line: &Timeline, playing: bool, elapsed: f32, zoom: &mut f32) {
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

/// Set the length of the action a slot's path names.
///
/// Returns whether anything changed, so the caller only marks the
/// project dirty when it did.
pub fn set_seconds(actions: &mut [Action], path: &[usize], seconds: f32) -> bool {
    let Some((&index, rest)) = path.split_first() else {
        return false;
    };
    let Some(action) = actions.get_mut(index) else {
        return false;
    };
    if rest.is_empty() {
        return match action {
            Action::Wait { seconds: value } | Action::MoveCamera { seconds: value, .. } => {
                let rounded = (seconds.max(0.0) * 10.0).round() / 10.0;
                let changed = (*value - rounded).abs() > f32::EPSILON;
                *value = rounded;
                changed
            }
            _ => false,
        };
    }
    // Into a branch. Only the taken side is laid out, so only the taken
    // side can be reached from a bar.
    match action {
        Action::Branch { then, .. } => set_seconds(then, rest, seconds),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec2;
    use infengine_adventure::action::Condition;

    #[test]
    fn dragging_a_wait_changes_its_seconds() {
        let mut actions = vec![Action::Wait { seconds: 1.0 }];
        assert!(set_seconds(&mut actions, &[0], 2.5));
        assert_eq!(actions[0], Action::Wait { seconds: 2.5 });
    }

    #[test]
    fn a_length_is_rounded_so_dragging_gives_a_number_worth_reading() {
        // A pan of 1.4732 seconds is a number nobody typed and nobody
        // wants to see in a file.
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
        // Its length comes from its text or its frames, so accepting the
        // drag would write a number nothing reads.
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
        // Otherwise every frame of a drag that moved nothing marks the
        // project dirty.
        let mut actions = vec![Action::Wait { seconds: 2.0 }];
        assert!(!set_seconds(&mut actions, &[0], 2.0));
    }
}
