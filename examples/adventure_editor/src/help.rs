//! A42.2: a window that explains the window.
//!
//! Every part of this editor was named by someone who already knew what
//! it did. That is the usual way an editor becomes unreadable: nothing
//! is wrong, and nothing says what any of it is for.
//!
//! So there is one page that describes the layout in the order a reader
//! meets it - left, middle, right, bottom - then the four modes, then
//! how things get added. It opens by itself once per run and can be
//! closed like any window.
//!
//! It first opened only when no room was loaded, on the reasoning that
//! an empty editor is the newcomer's view. That was wrong: a project
//! opens straight into its first room, so the one person who needed the
//! page was the one person who never saw it.
//!
//! The text is written for someone who has never seen an adventure
//! editor. Terms the rest of the interface uses are introduced here and
//! nowhere else: a hotspot is a clickable region, the floor is where the
//! actor may stand, a sheet is a strip of frames.

use crate::mode::Mode;

/// Whether the page is open, and whether it has ever been opened.
#[derive(Debug, Clone, Default)]
pub struct HelpUi {
    pub open: bool,
    /// Set once the page has been shown, so it is offered once and then
    /// stays out of the way.
    pub seen: bool,
}

impl HelpUi {
    /// Open the page unprompted, once per run.
    ///
    /// Once per run rather than once ever because nothing here is
    /// persisted yet; closing it costs a click and finding it again
    /// costs knowing where to look, and the second is the worse of the
    /// two to get wrong.
    pub fn offer_once(&mut self) {
        if !self.seen {
            self.open = true;
            self.seen = true;
        }
    }
}

/// One part of the window, and what it is for.
struct Part {
    where_: &'static str,
    what: &'static str,
}

const PARTS: &[Part] = &[
    Part {
        where_: "Left",
        what: "Every room in the project. Click one to open it. \
               The box at the top filters the list by name.",
    },
    Part {
        where_: "Middle",
        what: "The room itself, as the player will see it. \
               Drag with the middle button to pan, scroll to zoom. \
               What you draw with the current tool goes here.",
    },
    Part {
        where_: "Right",
        what: "The inspector. What it shows follows the mode: the room's \
               parts in Room, animations in Animate, the script's steps \
               in Script, the tester's controls in Test.",
    },
    Part {
        where_: "Bottom",
        what: "What is under the cursor, and what just happened. \
               In Animate mode the timeline sits beneath it.",
    },
    Part {
        where_: "Top",
        what: "The menus, then the four mode pills - Room, Animate, \
               Script, Test - then Save and Play. The pills are the \
               spine of the editor: pick what you are doing and the \
               tools, inspector and centre follow.",
    },
];

pub fn window(ctx: &egui::Context, state: &mut HelpUi) {
    let mut open = state.open;
    egui::Window::new("How this editor works")
        .open(&mut open)
        .default_width(460.0)
        .default_height(520.0)
        .collapsible(false)
        .show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("help_scroll")
                .show(ui, |ui| {
                    ui.label(
                        "This is an editor for point-and-click adventures. \
                         A game is a set of rooms; a room is one screen the \
                         player looks at and clicks on.",
                    );
                    ui.add_space(10.0);

                    ui.heading("The window");
                    for part in PARTS {
                        ui.add_space(4.0);
                        ui.label(egui::RichText::new(part.where_).strong());
                        ui.label(egui::RichText::new(part.what).small());
                    }

                    ui.add_space(12.0);
                    ui.heading("The four modes");
                    ui.label(
                        egui::RichText::new(
                            "The pills at the top. Each answers one question \
                             about the game you are making.",
                        )
                        .small()
                        .weak(),
                    );
                    for mode in Mode::ALL {
                        ui.add_space(4.0);
                        ui.label(egui::RichText::new(mode.label()).strong());
                        ui.label(egui::RichText::new(mode.hint()).small());
                    }

                    ui.add_space(12.0);
                    ui.heading("Adding things");
                    ui.label(
                        "Everything a room can hold is added from the Room menu. \
                         What you add appears in the middle of the view, and the \
                         editor switches to Room mode to edit it.",
                    );
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new(
                            "You can also draw one directly: in Room mode the Outline \
                             tool traces the walkable area, and the Box out tool drags \
                             a rectangle over something clickable.",
                        )
                        .small(),
                    );

                    ui.add_space(12.0);
                    ui.heading("The words");
                    for (term, meaning) in TERMS {
                        ui.add_space(4.0);
                        ui.label(egui::RichText::new(*term).strong());
                        ui.label(egui::RichText::new(*meaning).small());
                    }
                });
        });
    state.open = open;
}

/// Terms the rest of the interface uses without explaining.
const TERMS: &[(&str, &str)] = &[
    (
        "Floor",
        "The part of the picture the actor may stand on. Without one \
         the actor cannot move at all.",
    ),
    (
        "Clickable thing",
        "A region of the picture the player can click, with a name they \
         read and a script that runs.",
    ),
    (
        "Way out",
        "A region that sends the player to another room. It names the \
         room and, optionally, which arrival point to use there.",
    ),
    (
        "Arrival point",
        "Where the actor stands on entering. A room reached from three \
         directions wants three of them.",
    ),
    (
        "Sheet",
        "One image holding a row of frames. Cutting it up turns it into \
         an animation.",
    ),
    (
        "Screen",
        "A part of the game that is not a room: a title card, a menu, \
         the end credits.",
    ),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_page_is_offered_once_and_then_left_alone() {
        let mut help = HelpUi::default();
        assert!(!help.open, "it does not open before the first frame");

        help.offer_once();
        assert!(help.open);

        help.open = false;
        help.offer_once();
        assert!(!help.open, "offering it again would be nagging");
    }

    #[test]
    fn every_part_of_the_window_is_described() {
        // Four edges and the middle: a reader must not find a region of
        // the window this page failed to mention.
        assert_eq!(PARTS.len(), 5);
        for part in PARTS {
            assert!(!part.where_.is_empty());
            assert!(
                part.what.len() > 40,
                "{} needs more than a phrase",
                part.where_
            );
        }
    }

    #[test]
    fn every_term_the_interface_uses_is_defined() {
        for (term, meaning) in TERMS {
            assert!(!term.is_empty());
            assert!(meaning.len() > 30, "{term} is not really explained");
        }
    }
}
