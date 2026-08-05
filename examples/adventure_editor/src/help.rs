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

/// The layout in the order a reader meets it, in the interface
/// language. A function rather than a constant so the switch in the
/// View menu takes effect on the next frame.
fn parts() -> [Part; 6] {
    use crate::lang::t;
    [
        Part {
            where_: t("Left", "Vlevo"),
            what: t(
                "The library: everything the game is made of. Rooms, \
                 pictures, animations, sounds, scripts, screens, fonts. \
                 The box at the top searches the open shelf.",
                "Knihovna: všechno, z čeho se hra skládá. Místnosti, \
                 obrázky, animace, zvuky, skripty, obrazovky, písma. \
                 Políčko nahoře prohledává otevřenou polici.",
            ),
        },
        Part {
            where_: t("Middle", "Uprostřed"),
            what: t(
                "The room itself, as the player will see it. \
                 Drag with the middle button to pan, scroll to zoom. \
                 What you draw with the current tool goes here.",
                "Samotná místnost, jak ji uvidí hráč. Prostředním \
                 tlačítkem posouváš, kolečkem přibližuješ. Sem se \
                 kreslí to, co děláš aktivním nástrojem.",
            ),
        },
        Part {
            where_: t("Right", "Vpravo"),
            what: t(
                "The inspector. What it shows follows the mode: the room's \
                 parts in Room, animations in Animate, the script's steps \
                 in Script, the tester's controls in Test.",
                "Inspektor. Co ukazuje, řídí režim: části místnosti v \
                 režimu Místnost, animace v Animaci, kroky skriptu ve \
                 Skriptu, ovládání testera v Testu.",
            ),
        },
        Part {
            where_: t("Bottom", "Dole"),
            what: t(
                "What is under the cursor, and what just happened. \
                 In Animate mode the timeline sits beneath it.",
                "Co je pod kurzorem a co se právě stalo. V režimu \
                 Animace je pod tím časová osa.",
            ),
        },
        Part {
            where_: t("Opening a game", "Otevření hry"),
            what: t(
                "Project > Open a game lists the games you had open and \
                 browses for another. A folder holding rooms/ is a game, \
                 and New game writes a fresh one.",
                "Projekt > Otevřít hru ukáže naposledy otevřené hry a nechá \
                 tě projít složky. Složka s rooms/ je hra a Nová hra založí \
                 čerstvou.",
            ),
        },
        Part {
            where_: t("Top", "Nahoře"),
            what: t(
                "The menus, then the mode pills - Room, Animate, Script, \
                 Test, Story - then Save and Play. The pills are the \
                 spine of the editor: pick what you are doing and the \
                 tools, inspector and centre follow.",
                "Menu, pak přepínače režimů - Místnost, Animace, Skript, \
                 Test, Příběh - a Uložit s Přehrát. Přepínače jsou páteř \
                 editoru: vyber, co děláš, a nástroje, inspektor i střed \
                 se přizpůsobí.",
            ),
        },
    ]
}

pub fn window(ctx: &egui::Context, state: &mut HelpUi) {
    use crate::lang::t;
    let mut open = state.open;
    egui::Window::new(t("How this editor works", "Jak tento editor funguje"))
        .id(egui::Id::new("help_window"))
        .open(&mut open)
        .default_width(460.0)
        .default_height(520.0)
        .collapsible(false)
        .show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("help_scroll")
                .show(ui, |ui| {
                    ui.label(t(
                        "This is an editor for point-and-click adventures. \
                         A game is a set of rooms; a room is one screen the \
                         player looks at and clicks on.",
                        "Tohle je editor point-and-click adventur. Hra je \
                         sada místností; místnost je jedna obrazovka, na \
                         kterou se hráč dívá a kliká.",
                    ));
                    ui.add_space(10.0);

                    ui.heading(t("The window", "Okno"));
                    for part in parts() {
                        ui.add_space(4.0);
                        ui.label(egui::RichText::new(part.where_).strong());
                        ui.label(egui::RichText::new(part.what).small());
                    }

                    ui.add_space(12.0);
                    ui.heading(t("The modes", "Režimy"));
                    ui.label(
                        egui::RichText::new(t(
                            "The pills at the top. Each answers one question \
                             about the game you are making.",
                            "Přepínače nahoře. Každý odpovídá na jednu otázku \
                             o hře, kterou tvoříš.",
                        ))
                        .small()
                        .weak(),
                    );
                    for mode in Mode::ALL {
                        ui.add_space(4.0);
                        ui.label(egui::RichText::new(mode.label()).strong());
                        ui.label(egui::RichText::new(mode.hint()).small());
                    }

                    ui.add_space(12.0);
                    ui.heading(t("Adding things", "Přidávání věcí"));
                    ui.label(t(
                        "Everything a room can hold is added from the Room menu. \
                         What you add appears in the middle of the view, and the \
                         editor switches to Room mode to edit it.",
                        "Všechno, co místnost může obsahovat, se přidává z menu \
                         Místnost. Přidané se objeví uprostřed pohledu a editor \
                         se přepne do režimu Místnost, kde se to upravuje.",
                    ));
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new(t(
                            "You can also draw one directly: in Room mode the Outline \
                             tool traces the walkable area, and the Box out tool drags \
                             a rectangle over something clickable.",
                            "Jde to i kreslením: v režimu Místnost nástroj Obrys \
                             obtáhne pochozí plochu a nástroj Vyznačit natáhne \
                             obdélník přes něco klikacího.",
                        ))
                        .small(),
                    );

                    ui.add_space(12.0);
                    ui.heading(t("The words", "Pojmy"));
                    for (term, meaning) in terms() {
                        ui.add_space(4.0);
                        ui.label(egui::RichText::new(term).strong());
                        ui.label(egui::RichText::new(meaning).small());
                    }
                });
        });
    state.open = open;
}

/// Terms the rest of the interface uses without explaining, in the
/// interface language.
fn terms() -> [(&'static str, &'static str); 6] {
    use crate::lang::t;
    [
        (
            t("Floor", "Podlaha"),
            t(
                "The part of the picture the actor may stand on. Without one \
                 the actor cannot move at all.",
                "Část obrázku, na které smí postava stát. Bez ní se postava \
                 vůbec nepohne.",
            ),
        ),
        (
            t("Clickable thing", "Klikací věc"),
            t(
                "A region of the picture the player can click, with a name they \
                 read and a script that runs.",
                "Oblast obrázku, na kterou hráč může kliknout - má jméno, \
                 které si přečte, a skript, který se spustí.",
            ),
        ),
        (
            t("Way out", "Východ"),
            t(
                "A region that sends the player to another room. It names the \
                 room and, optionally, which arrival point to use there.",
                "Oblast, která pošle hráče do jiné místnosti. Říká kam a \
                 případně na které místo příchodu.",
            ),
        ),
        (
            t("Arrival point", "Místo příchodu"),
            t(
                "Where the actor stands on entering. A room reached from three \
                 directions wants three of them.",
                "Kde postava stojí po příchodu. Místnost přístupná ze tří \
                 stran jich chce mít tři.",
            ),
        ),
        (
            t("Sheet", "Arch"),
            t(
                "One image holding a row of frames. Cutting it up turns it into \
                 an animation.",
                "Jeden obrázek s řadou snímků. Rozřezáním z něj vznikne \
                 animace.",
            ),
        ),
        (
            t("Screen", "Obrazovka"),
            t(
                "A part of the game that is not a room: a title card, a menu, \
                 the end credits.",
                "Část hry, která není místnost: titulek, menu, závěrečné \
                 titulky.",
            ),
        ),
    ]
}

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
        let _guard = crate::lang::test_lock();
        // Four edges and the middle, in both languages: a reader must
        // not find a region of the window this page failed to mention.
        for czech in [false, true] {
            crate::lang::set_czech(czech);
            let parts = parts();
            assert_eq!(parts.len(), 6);
            for part in parts {
                assert!(!part.where_.is_empty());
                assert!(
                    part.what.len() > 40,
                    "{} needs more than a phrase",
                    part.where_
                );
            }
        }
        crate::lang::set_czech(false);
    }

    #[test]
    fn every_term_the_interface_uses_is_defined() {
        let _guard = crate::lang::test_lock();
        for czech in [false, true] {
            crate::lang::set_czech(czech);
            for (term, meaning) in terms() {
                assert!(!term.is_empty());
                assert!(meaning.len() > 30, "{term} is not really explained");
            }
        }
        crate::lang::set_czech(false);
    }
}
