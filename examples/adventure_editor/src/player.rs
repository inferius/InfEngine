//! A90: the sound player - a strip above the status bar.
//!
//! Hear used to be fire-and-forget: the sound started and the editor
//! forgot it, so a three-minute track could not be stopped, paused or
//! skipped through, and picking music meant listening from the top
//! every time. Now whatever the editor plays - a Hear button, a line's
//! recording, a track from the shelf - lands here, with the three
//! controls a player owes its listener: where it is, hold it, stop it.
//!
//! The strip exists only while something is loaded and puts itself
//! away when the sound runs out.

use infengine_audio::ActiveSound;

/// The sound the editor is playing, and what the strip needs to say
/// about it.
pub struct Player {
    /// What to call it: the id it was asked for by, not the file path.
    pub name: String,
    pub handle: ActiveSound,
    pub duration: f32,
    pub paused: bool,
}

impl std::fmt::Debug for Player {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Player").field("name", &self.name).finish()
    }
}

/// Short fade on every cut, so none of them click.
const FADE: f32 = 0.05;

/// Draw the strip and run its controls. Clears the slot when the
/// sound has finished or was stopped.
pub fn panel(ctx: &egui::Context, slot: &mut Option<Player>) {
    use crate::lang::t;
    let Some(player) = slot.as_mut() else {
        return;
    };
    if player.handle.is_stopped() {
        *slot = None;
        return;
    }

    let mut finished = false;
    egui::TopBottomPanel::bottom("player").show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.label("♪");
            ui.label(egui::RichText::new(&player.name).small().strong());

            if player.paused {
                if ui
                    .small_button("▶")
                    .on_hover_text(t("Play on", "Hrát dál"))
                    .clicked()
                {
                    player.handle.resume(FADE);
                    player.paused = false;
                }
            } else if ui
                .small_button("⏸")
                .on_hover_text(t("Hold it here", "Podržet tady"))
                .clicked()
            {
                player.handle.pause(FADE);
                player.paused = true;
            }
            if ui
                .small_button("⏹")
                .on_hover_text(t("Stop", "Zastavit"))
                .clicked()
            {
                player.handle.stop(FADE);
                finished = true;
            }

            // The scrubber. While the hand is on it the knob follows
            // the hand, not the clock; the seek fires once, on release.
            let drag_id = egui::Id::new("player_scrub");
            let held = ui.ctx().data(|d| d.get_temp::<f32>(drag_id));
            let mut at = held.unwrap_or_else(|| player.handle.position());
            let end = player.duration.max(0.1);
            let scrub = ui.add(
                egui::Slider::new(&mut at, 0.0..=end)
                    .show_value(false)
                    .trailing_fill(true),
            );
            if scrub.drag_stopped() {
                ui.ctx().data_mut(|d| d.remove::<f32>(drag_id));
                player.handle.seek_to(at.min(player.duration));
            } else if scrub.dragged() {
                ui.ctx().data_mut(|d| d.insert_temp(drag_id, at));
            } else if scrub.changed() {
                // A click on the rail rather than a drag of the knob.
                player.handle.seek_to(at.min(player.duration));
            }

            ui.label(
                egui::RichText::new(format!(
                    "{} / {}",
                    mmss(player.handle.position()),
                    mmss(player.duration)
                ))
                .small()
                .monospace()
                .weak(),
            );
        });
    });
    if finished {
        *slot = None;
    } else {
        // The progress moves whether the mouse does or not.
        ctx.request_repaint();
    }
}

/// Seconds as a clock reads them.
fn mmss(seconds: f32) -> String {
    let whole = seconds.max(0.0) as u32;
    format!("{}:{:02}", whole / 60, whole % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_read_like_a_clock() {
        assert_eq!(mmss(0.0), "0:00");
        assert_eq!(mmss(7.9), "0:07");
        assert_eq!(mmss(65.0), "1:05");
        assert_eq!(mmss(600.0), "10:00");
        assert_eq!(mmss(-3.0), "0:00", "a broken position does not underflow");
    }
}
