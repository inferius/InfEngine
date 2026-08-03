//! A62.1: the library - everything a scene can be built from.
//!
//! Scenes are made of pictures, animations, and sounds, and until now
//! those lived nowhere visible: the timeline's Add menu could name them
//! but nothing showed them. An author cannot compose from material they
//! cannot see. The library is one window with everything on the shelf -
//! browse it, preview it, and put it straight into the open scene.
//!
//! "Add to scene" inserts the asset as a step after the selected one in
//! the scene's timeline, which is the same move the timeline's own Add
//! menu makes - the library is a bigger door into the same room.

use std::collections::HashMap;

use infengine_adventure::{Action, Project};

use crate::timeline::DragAsset;

/// Which shelf is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Shelf {
    #[default]
    Pictures,
    Animations,
    Sounds,
    Screens,
}

/// The window's own state.
#[derive(Default)]
pub struct LibraryUi {
    pub open: bool,
    filter: String,
    shelf: Shelf,
    thumbs: HashMap<String, Option<egui::TextureHandle>>,
}

/// What the library asked for.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct LibraryActions {
    /// A step to add to the open scene's timeline.
    pub add: Option<Action>,
    /// An animation to preview in the canvas.
    pub preview: Option<String>,
}

/// Draw the window. Returns what was asked for.
pub fn window(
    ctx: &egui::Context,
    state: &mut LibraryUi,
    project: &Project,
    pictures: &[String],
) -> LibraryActions {
    let mut actions = LibraryActions::default();
    let mut open = state.open;
    egui::Window::new("Library")
        .open(&mut open)
        .default_size(egui::vec2(430.0, 520.0))
        .show(ctx, |ui| {
            header(ui, state, project, pictures);
            ui.separator();
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| match state.shelf {
                    Shelf::Pictures => pictures_shelf(ui, state, project, pictures, &mut actions),
                    Shelf::Animations => animations_shelf(ui, state, project, &mut actions),
                    Shelf::Sounds => sounds_shelf(ui, state, project, &mut actions),
                    Shelf::Screens => screens_shelf(ui, state, project),
                });
        });
    state.open = open;
    actions
}

fn header(ui: &mut egui::Ui, state: &mut LibraryUi, project: &Project, pictures: &[String]) {
    ui.horizontal(|ui| {
        let shelves = [
            (Shelf::Pictures, format!("Pictures ({})", pictures.len())),
            (
                Shelf::Animations,
                format!("Animations ({})", project.animations.iter().count()),
            ),
            (
                Shelf::Sounds,
                format!("Sounds ({})", project.sounds.ids().count()),
            ),
            (Shelf::Screens, format!("Screens ({})", project.screens.len())),
        ];
        for (shelf, label) in shelves {
            ui.selectable_value(&mut state.shelf, shelf, label);
        }
    });
    ui.horizontal(|ui| {
        ui.label("Find");
        ui.text_edit_singleline(&mut state.filter);
        if !state.filter.is_empty() && ui.small_button("clear").clicked() {
            state.filter.clear();
        }
    });
}

/// Case-insensitive name filter, empty matching everything.
fn shown<'a>(filter: &str, names: impl Iterator<Item = &'a str>) -> Vec<&'a str> {
    let needle = filter.trim().to_ascii_lowercase();
    names
        .filter(|name| needle.is_empty() || name.to_ascii_lowercase().contains(&needle))
        .collect()
}

fn pictures_shelf(
    ui: &mut egui::Ui,
    state: &mut LibraryUi,
    project: &Project,
    pictures: &[String],
    actions: &mut LibraryActions,
) {
    // A couple of reads per frame keeps opening the window smooth; the
    // rest fill in as the frames go by.
    let mut budget = 2usize;
    for name in shown(&state.filter, pictures.iter().map(String::as_str)) {
        let thumb = match state.thumbs.get(name) {
            Some(loaded) => loaded.clone(),
            None if budget > 0 => {
                budget -= 1;
                let loaded = read_thumb(ui.ctx(), project, name);
                state.thumbs.insert(name.to_string(), loaded.clone());
                loaded
            }
            None => {
                ui.ctx().request_repaint();
                None
            }
        };
        ui.horizontal(|ui| {
            match thumb {
                Some(texture) => {
                    let size = texture.size_vec2();
                    let scale = 72.0 / size.x.max(1.0);
                    ui.image((texture.id(), size * scale));
                }
                None => {
                    ui.allocate_exact_size(egui::vec2(72.0, 32.0), egui::Sense::hover());
                }
            }
            draggable(ui, name, DragAsset::Image(name.to_string()));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .small_button("Add to scene")
                    .on_hover_text("Show this picture for a while, as a timeline step")
                    .clicked()
                {
                    actions.add = Some(Action::ShowImage {
                        image: name.to_string(),
                        seconds: 2.0,
                    });
                }
            });
        });
    }
}

fn read_thumb(
    ctx: &egui::Context,
    project: &Project,
    name: &str,
) -> Option<egui::TextureHandle> {
    let small = image::open(project.resolve(name)).ok()?.thumbnail(144, 144);
    let rgba = small.to_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    let pixels = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
    Some(ctx.load_texture(format!("library:{name}"), pixels, egui::TextureOptions::LINEAR))
}

fn animations_shelf(
    ui: &mut egui::Ui,
    state: &LibraryUi,
    project: &Project,
    actions: &mut LibraryActions,
) {
    let names: Vec<&str> = project.animations.iter().map(|a| a.id.as_str()).collect();
    for name in shown(&state.filter, names.into_iter()) {
        let Some(animation) = project.animations.iter().find(|a| a.id == name) else {
            continue;
        };
        ui.horizontal(|ui| {
            draggable(ui, name, DragAsset::Animation(name.to_string()));
            ui.label(
                egui::RichText::new(format!("{} frames", animation.frames))
                    .small()
                    .weak(),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .small_button("Add to scene")
                    .on_hover_text("Play this animation, as a timeline step")
                    .clicked()
                {
                    actions.add = Some(Action::PlayAnim {
                        animation: name.to_string(),
                        wait: true,
                        at: None,
                    });
                }
                if ui
                    .small_button("Preview")
                    .on_hover_text("Play it in the canvas")
                    .clicked()
                {
                    actions.preview = Some(name.to_string());
                }
            });
        });
    }
}

fn sounds_shelf(
    ui: &mut egui::Ui,
    state: &LibraryUi,
    project: &Project,
    actions: &mut LibraryActions,
) {
    let names: Vec<&str> = project.sounds.ids().collect();
    for name in shown(&state.filter, names.into_iter()) {
        ui.horizontal(|ui| {
            draggable(ui, name, DragAsset::Sound(name.to_string()));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .small_button("Add to scene")
                    .on_hover_text("Play this sound, as a timeline step")
                    .clicked()
                {
                    actions.add = Some(Action::PlaySound {
                        sound: name.to_string(),
                    });
                }
            });
        });
    }
}

/// An asset's name, draggable onto the timeline.
///
/// The label is the handle: picking a thing up by its name and putting
/// it where it should happen is the whole gesture, and the button next
/// to it stays for anyone who would rather press than drag.
fn draggable(ui: &mut egui::Ui, name: &str, asset: DragAsset) {
    let id = egui::Id::new(("library_drag", name));
    ui.dnd_drag_source(id, asset, |ui| {
        ui.label(name);
    })
    .response
    .on_hover_cursor(egui::CursorIcon::Grab)
    .on_hover_text("Drag onto the timeline to add it to the scene");
}

fn screens_shelf(ui: &mut egui::Ui, state: &LibraryUi, project: &Project) {
    ui.label(
        egui::RichText::new(
            "Menus and overlays. A scene opens one with an Open screen step; \
             edit them under Project.",
        )
        .small()
        .weak(),
    );
    ui.add_space(4.0);
    let names: Vec<&str> = project.screens.iter().map(|s| s.id.as_str()).collect();
    for name in shown(&state.filter, names.into_iter()) {
        let widgets = project
            .screens
            .iter()
            .find(|s| s.id == name)
            .map(|s| s.widgets.len())
            .unwrap_or(0);
        ui.horizontal(|ui| {
            ui.label(name);
            ui.label(
                egui::RichText::new(format!("{widgets} widgets"))
                    .small()
                    .weak(),
            );
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_filter_ignores_case_and_an_empty_one_keeps_everything() {
        let names = ["intro_sky.png", "note.en.png"];
        assert_eq!(shown("", names.iter().copied()).len(), 2);
        assert_eq!(shown("SKY", names.iter().copied()), vec!["intro_sky.png"]);
        assert!(shown("zzz", names.iter().copied()).is_empty());
    }

    #[test]
    fn actions_default_to_doing_nothing() {
        let actions = LibraryActions::default();
        assert!(actions.add.is_none() && actions.preview.is_none());
    }
}
