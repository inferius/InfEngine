//! A83 / A95: the shot - how much of the room is seen, and from where.
//!
//! Lives in Room mode because the camera is a property of the room: a
//! scrolling room carries its view size with it wherever a scene points
//! the camera.
//!
//! A95: the numbers used to be the whole interface, which meant a room
//! taller than the screen looked finished and played wrong - the view
//! showed all 400 pixels of a 320x200 game squashed into 200. So the
//! panel now compares the room against the game's own resolution and
//! says what it finds, with the fix as one button: scroll it, follow
//! the actor. The numbers stay for anyone who wants them.

use glam::Vec2;

use crate::lang::t;
use crate::ui::UiState;

pub fn show(ui: &mut egui::Ui, state: &mut UiState<'_>) {
    let size = state.room.size;
    let window = state.game_view;
    let bigger = size.x > window.x + 0.5 || size.y > window.y + 0.5;
    let scrolls = state.room.camera.scrolls(size);

    // What this room is, in one sentence, before any number.
    if bigger && !scrolls {
        ui.label(
            egui::RichText::new(if crate::lang::is_czech() {
                format!(
                    "Místnost je větší než okno hry ({} × {}). Zatím se celá mačká na obrazovku.",
                    window.x as i32, window.y as i32
                )
            } else {
                format!(
                    "This room is bigger than the game's window ({} × {}). \
                     Right now all of it is squashed onto the screen.",
                    window.x as i32, window.y as i32
                )
            })
            .color(egui::Color32::from_rgb(255, 200, 120)),
        );
        if ui
            .button(t(
                "Scroll it, following the actor",
                "Posouvat ji za postavou",
            ))
            .on_hover_text(t(
                "The view becomes the game's window and travels with the actor",
                "Pohled bude velký jako okno hry a bude cestovat s postavou",
            ))
            .clicked()
        {
            state.room.camera.view = window;
            state.room.camera.follow = true;
        }
        ui.add_space(6.0);
    } else if scrolls {
        let moves = state.room.camera.view_in(size);
        ui.label(
            egui::RichText::new(if crate::lang::is_czech() {
                format!(
                    "Posouvá se: vidět je {} × {}, {} když se {}.",
                    moves.x as i32,
                    moves.y as i32,
                    if state.room.camera.follow {
                        "jede za postavou"
                    } else {
                        "hýbe jen skriptem"
                    },
                    if state.room.camera.follow { "hne" } else { "řekne" }
                )
            } else {
                format!(
                    "Scrolls: {} × {} on screen, {}.",
                    moves.x as i32,
                    moves.y as i32,
                    if state.room.camera.follow {
                        "travelling with the actor"
                    } else {
                        "moved only by scripts"
                    }
                )
            })
            .small()
            .weak(),
        );
    }

    let mut view = state.room.camera.view;
    let mut whole = view.x <= 0.0 && view.y <= 0.0;
    if ui
        .checkbox(&mut whole, t(
            "The whole room is on screen",
            "Celá místnost je na obrazovce",
        ))
        .on_hover_text(t(
            "Untick for a room the view moves around inside",
            "Odškrtni pro místnost, uvnitř které se pohled pohybuje",
        ))
        .changed()
    {
        // Turning it off starts from the game's window rather than
        // from zero or from the room's own size: what the view should
        // show is what the player's screen shows, and starting there
        // makes the first drag an adjustment.
        view = if whole { Vec2::ZERO } else { window.min(size) };
        state.room.camera.view = view;
    }

    if !whole {
        egui::Grid::new("camera_grid").num_columns(2).show(ui, |ui| {
            ui.label(t("view", "pohled"));
            ui.horizontal(|ui| {
                ui.add(
                    egui::DragValue::new(&mut state.room.camera.view.x)
                        .range(16.0..=size.x.max(16.0))
                        .speed(1.0),
                );
                ui.label("x");
                ui.add(
                    egui::DragValue::new(&mut state.room.camera.view.y)
                        .range(16.0..=size.y.max(16.0))
                        .speed(1.0),
                );
                if ui
                    .small_button(t("= window", "= okno"))
                    .on_hover_text(t(
                        "Set it to the game's own resolution",
                        "Nastav na rozlišení hry",
                    ))
                    .clicked()
                {
                    state.room.camera.view = window.min(size);
                }
            });
            ui.end_row();
        });
        ui.checkbox(
            &mut state.room.camera.follow,
            t("Follow the actor", "Jet za postavou"),
        )
        .on_hover_text(t(
            "Off for a cutscene, which has no actor to follow",
            "Vypnuto pro scénu, která nemá koho sledovat",
        ));

        let moves = state.room.camera.view_in(size);
        ui.label(
            egui::RichText::new(if crate::lang::is_czech() {
                format!(
                    "{} × {} z {} × {} - pohled ujede {} do stran a {} dolů",
                    moves.x as i32,
                    moves.y as i32,
                    size.x as i32,
                    size.y as i32,
                    (size.x - moves.x).max(0.0) as i32,
                    (size.y - moves.y).max(0.0) as i32,
                )
            } else {
                format!(
                    "{} x {} of {} x {} - the view can travel {} across and {} down",
                    moves.x as i32,
                    moves.y as i32,
                    size.x as i32,
                    size.y as i32,
                    (size.x - moves.x).max(0.0) as i32,
                    (size.y - moves.y).max(0.0) as i32,
                )
            })
            .small()
            .weak(),
        );
    }

    if state.playing {
        ui.label(
            egui::RichText::new(format!(
                "now at {:.0}, {:.0}",
                state.camera.x, state.camera.y
            ))
            .small()
            .monospace(),
        );
    }
}
