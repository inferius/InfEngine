//! A62.1 / A84: the library - everything the game is made of, always
//! on the left.
//!
//! It was a floating window once, and the one thing every report about
//! it agreed on was that nobody found it. A library that has to be
//! found is a contradiction; this one is the left edge of the window,
//! open from the first frame to the last.
//!
//! One search box, one row of shelves - rooms, pictures, animations,
//! sounds, scripts, screens, fonts - and the rooms are simply the first
//! shelf, because a room is an asset like any other: the thing scenes
//! are built in, next to the things they are built from.
//!
//! "Add to scene" inserts the asset as a step in the scene's timeline;
//! dragging it onto the timeline does the same with placement. The
//! library is the shelf, the timeline is the bench.

use std::collections::HashMap;

use infengine_adventure::{Action, Project};

use crate::timeline::DragAsset;
use crate::ui::{UiActions, UiState};

/// Which shelf is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Shelf {
    #[default]
    Rooms,
    Pictures,
    Animations,
    Sounds,
    Scripts,
    Screens,
    Fonts,
}

impl Shelf {
    const ALL: [Shelf; 7] = [
        Shelf::Rooms,
        Shelf::Pictures,
        Shelf::Animations,
        Shelf::Sounds,
        Shelf::Scripts,
        Shelf::Screens,
        Shelf::Fonts,
    ];

    fn label(self) -> &'static str {
        match self {
            Shelf::Rooms => "Rooms",
            Shelf::Pictures => "Pictures",
            Shelf::Animations => "Animations",
            Shelf::Sounds => "Sounds",
            Shelf::Scripts => "Scripts",
            Shelf::Screens => "Screens",
            Shelf::Fonts => "Fonts",
        }
    }
}

/// The panel's own state.
#[derive(Default)]
pub struct LibraryUi {
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
    /// A sound or track to hear, by its id.
    pub play: Option<String>,
    /// A script to put on the graph canvas, in Script mode.
    pub open_script: Option<String>,
    /// The screen designer was asked for.
    pub edit_screens: bool,
}

/// Draw the panel. Returns what was asked for; opening a room goes
/// through `actions`, the same road the old room list used.
pub fn panel(
    ctx: &egui::Context,
    state: &mut LibraryUi,
    ui_state: &mut UiState<'_>,
    actions: &mut UiActions,
    pictures: &[String],
) -> LibraryActions {
    let mut asked = LibraryActions::default();
    // Answer a few of the room list's thumbnail requests from last
    // frame before drawing it again.
    ui_state.cards.tick(ctx, ui_state.project, 3);
    egui::SidePanel::left("library")
        .default_width(270.0)
        .show(ctx, |ui| {
            header(ui, state, ui_state.project, pictures);
            ui.separator();
            let footer = 26.0;
            egui::ScrollArea::vertical()
                .id_salt("library_scroll")
                .auto_shrink([false, false])
                .max_height((ui.available_height() - footer).max(60.0))
                .show(ui, |ui| match state.shelf {
                    Shelf::Rooms => rooms_shelf(ui, state, ui_state, actions),
                    Shelf::Pictures => {
                        pictures_shelf(ui, state, ui_state.project, pictures, &mut asked)
                    }
                    Shelf::Animations => {
                        animations_shelf(ui, state, ui_state.project, &mut asked)
                    }
                    Shelf::Sounds => sounds_shelf(ui, state, ui_state.project, &mut asked),
                    Shelf::Scripts => scripts_shelf(ui, state, ui_state.project, &mut asked),
                    Shelf::Screens => screens_shelf(ui, state, ui_state.project, &mut asked),
                    Shelf::Fonts => fonts_shelf(ui, state, ui_state.project),
                });
            ui.separator();
            ui.label(
                egui::RichText::new("Drag an asset onto the timeline to put it in the scene.")
                    .small()
                    .weak(),
            );
        });
    asked
}

fn header(ui: &mut egui::Ui, state: &mut LibraryUi, project: &Project, pictures: &[String]) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("LIBRARY").small().weak());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let count = match state.shelf {
                Shelf::Rooms => project.rooms.len(),
                Shelf::Pictures => pictures.len(),
                Shelf::Animations => project.animations.iter().count(),
                Shelf::Sounds => project.sounds.ids().count(),
                Shelf::Scripts => project.scripts.len(),
                Shelf::Screens => project.screens.len(),
                Shelf::Fonts => project.fonts.len(),
            };
            ui.label(egui::RichText::new(count.to_string()).small().weak());
        });
    });
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(&mut state.filter)
                .hint_text("Search assets")
                .desired_width(f32::INFINITY),
        );
    });
    ui.horizontal_wrapped(|ui| {
        for shelf in Shelf::ALL {
            ui.selectable_value(&mut state.shelf, shelf, shelf.label());
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

// ---------------------------------------------------------------------
// Rooms - the scene list, folded by family.

/// The rooms, with same-background families folded.
///
/// `gencavb_127` through `gencavb_140` are one cave photographed
/// fourteen times, and a flat list of them buries the rooms that are
/// actually distinct. A search unfolds everything, because someone
/// typing a name wants the row itself, not the drawer it is in.
fn rooms_shelf(
    ui: &mut egui::Ui,
    state: &LibraryUi,
    ui_state: &mut UiState<'_>,
    actions: &mut UiActions,
) {
    if ui_state.project.is_empty() {
        ui.label("No rooms found.");
        ui.label(
            egui::RichText::new("Point the editor at a folder containing rooms/.")
                .small()
                .weak(),
        );
        return;
    }
    let labels: Vec<(usize, String)> = matching_rooms(ui_state.project, &state.filter)
        .into_iter()
        .filter_map(|index| {
            let path = ui_state.project.rooms.get(index)?;
            Some((index, Project::room_label(path)))
        })
        .collect();
    let searching = !state.filter.trim().is_empty();
    for (family, members) in group_by_family(&labels) {
        if members.len() == 1 || searching {
            for &(index, ref label) in &members {
                room_row(ui, ui_state, actions, index, label);
            }
            continue;
        }
        let holds_current = members
            .iter()
            .any(|(index, _)| ui_state.current_room == Some(*index));
        egui::CollapsingHeader::new(format!("{family} \u{b7} {}", members.len()))
            .id_salt(("family", family))
            .default_open(holds_current)
            .show(ui, |ui| {
                for &(index, ref label) in &members {
                    room_row(ui, ui_state, actions, index, label);
                }
            });
    }
}

/// One room's row: its background, its name, and what it holds.
fn room_row(
    ui: &mut egui::Ui,
    ui_state: &mut UiState<'_>,
    actions: &mut UiActions,
    index: usize,
    label: &str,
) {
    if let Some(path) = ui_state.project.rooms.get(index) {
        ui_state.cards.request(label, path);
    }
    let card = ui_state.cards.get(label);
    let selected = ui_state.current_room == Some(index);

    let mut text = egui::text::LayoutJob::default();
    text.append(
        label,
        0.0,
        egui::TextFormat {
            font_id: egui::FontId::proportional(13.0),
            color: ui.visuals().strong_text_color(),
            ..Default::default()
        },
    );
    if let Some(card) = card {
        text.append(
            &format!("\n{}", crate::cards::caption(card)),
            0.0,
            egui::TextFormat {
                font_id: egui::FontId::proportional(10.0),
                color: ui.visuals().weak_text_color(),
                ..Default::default()
            },
        );
    }

    let button = match card.and_then(|card| card.thumb.as_ref()) {
        Some(texture) => {
            let size = texture.size_vec2();
            let scale = crate::cards::THUMB / size.x.max(1.0);
            egui::Button::image_and_text(
                egui::Image::new((texture.id(), size * scale)),
                text,
            )
        }
        None => egui::Button::new(text),
    };
    let row = ui.add_sized(
        [ui.available_width(), 30.0],
        button.selected(selected).frame(selected),
    );
    if row.clicked() {
        actions.open_room = Some(index);
    }
}

/// Which family a room belongs to: its name up to a numbered suffix.
///
/// The numbered suffixes are how instances of one background are told
/// apart, so the family is exactly "the rooms sharing a picture".
pub fn family(label: &str) -> &str {
    match label.rsplit_once('_') {
        Some((head, tail))
            if !head.is_empty() && !tail.is_empty() && tail.bytes().all(|b| b.is_ascii_digit()) =>
        {
            head
        }
        _ => label,
    }
}

/// Group labelled rooms by family, keeping first-seen order.
pub fn group_by_family(labels: &[(usize, String)]) -> Vec<(String, Vec<(usize, String)>)> {
    let mut order: Vec<String> = Vec::new();
    let mut groups: std::collections::HashMap<String, Vec<(usize, String)>> = Default::default();
    for (index, label) in labels {
        let family = family(label).to_string();
        if !groups.contains_key(&family) {
            order.push(family.clone());
        }
        groups
            .entry(family)
            .or_default()
            .push((*index, label.clone()));
    }
    order
        .into_iter()
        .map(|family| {
            let members = groups.remove(&family).unwrap_or_default();
            (family, members)
        })
        .collect()
}

/// Indices of the rooms whose name contains `filter`, case-insensitively.
///
/// An empty filter matches everything, so the box starts out showing the
/// whole project rather than nothing.
pub fn matching_rooms(project: &Project, filter: &str) -> Vec<usize> {
    let needle = filter.trim().to_ascii_lowercase();
    project
        .rooms
        .iter()
        .enumerate()
        .filter(|(_, path)| {
            needle.is_empty()
                || Project::room_label(path)
                    .to_ascii_lowercase()
                    .contains(&needle)
        })
        .map(|(index, _)| index)
        .collect()
}

// ---------------------------------------------------------------------
// The other shelves.

fn pictures_shelf(
    ui: &mut egui::Ui,
    state: &mut LibraryUi,
    project: &Project,
    pictures: &[String],
    actions: &mut LibraryActions,
) {
    // A couple of reads per frame keeps the panel smooth; the rest fill
    // in as the frames go by.
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
                    let scale = 56.0 / size.x.max(1.0);
                    ui.image((texture.id(), size * scale));
                }
                None => {
                    ui.allocate_exact_size(egui::vec2(56.0, 24.0), egui::Sense::hover());
                }
            }
            draggable(ui, name, DragAsset::Image(name.to_string()));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .small_button("+")
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
                    .small_button("+")
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
                    .small_button("▶")
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
                    .small_button("+")
                    .on_hover_text("Play this sound, as a timeline step")
                    .clicked()
                {
                    actions.add = Some(Action::PlaySound {
                        sound: name.to_string(),
                    });
                }
                if ui
                    .small_button("▶")
                    .on_hover_text("Hear it now, in the editor")
                    .clicked()
                {
                    actions.play = Some(name.to_string());
                }
            });
        });
    }
}

/// Every script in the project, each a click from its graph.
fn scripts_shelf(
    ui: &mut egui::Ui,
    state: &LibraryUi,
    project: &Project,
    actions: &mut LibraryActions,
) {
    ui.label(
        egui::RichText::new("Click one to open it on the canvas, in Script mode.")
            .small()
            .weak(),
    );
    ui.add_space(4.0);
    let mut ids: Vec<&String> = project.scripts.keys().collect();
    ids.sort();
    for id in shown(&state.filter, ids.into_iter().map(String::as_str)) {
        let script = project.scripts.get(id);
        let steps = script.map(|s| s.actions.len()).unwrap_or(0);
        let unknown = script.map(|s| s.unknown_count()).unwrap_or(0);
        ui.horizontal(|ui| {
            if ui
                .selectable_label(false, id)
                .on_hover_text(format!("{steps} steps"))
                .clicked()
            {
                actions.open_script = Some(id.to_string());
            }
            if unknown > 0 {
                ui.label(
                    egui::RichText::new(format!("{unknown} unknown"))
                        .small()
                        .color(egui::Color32::from_rgb(230, 150, 60)),
                );
            }
        });
    }
}

fn screens_shelf(
    ui: &mut egui::Ui,
    state: &LibraryUi,
    project: &Project,
    actions: &mut LibraryActions,
) {
    ui.label(
        egui::RichText::new("Menus and overlays. A scene opens one with an Open screen step.")
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
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .small_button("Edit")
                    .on_hover_text("Open the screen designer")
                    .clicked()
                {
                    actions.edit_screens = true;
                }
            });
        });
    }
}

fn fonts_shelf(ui: &mut egui::Ui, state: &mut LibraryUi, project: &Project) {
    ui.label(
        egui::RichText::new(
            "The game's lettering. The manifest picks which one subtitles and menus draw with.",
        )
        .small()
        .weak(),
    );
    ui.add_space(4.0);
    let names: Vec<String> = project.fonts.iter().map(|f| f.id.clone()).collect();
    for name in shown(&state.filter, names.iter().map(String::as_str)) {
        let Some(font) = project.fonts.iter().find(|f| f.id == name) else {
            continue;
        };
        // The atlas itself is the best preview a font has.
        let thumb = match state.thumbs.get(&font.image) {
            Some(loaded) => loaded.clone(),
            None => {
                let loaded = read_thumb(ui.ctx(), project, &font.image);
                state.thumbs.insert(font.image.clone(), loaded.clone());
                loaded
            }
        };
        ui.horizontal(|ui| {
            if let Some(texture) = thumb {
                let size = texture.size_vec2();
                let scale = (120.0 / size.x.max(1.0)).min(2.0);
                ui.image((texture.id(), size * scale));
            }
            ui.label(name);
            ui.label(
                egui::RichText::new(format!(
                    "{} glyphs · line {}px",
                    font.glyphs.len(),
                    font.line_height
                ))
                .small()
                .weak(),
            );
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

#[cfg(test)]
mod tests {
    use super::*;

    fn project_of(names: &[&str]) -> Project {
        Project {
            root: std::path::PathBuf::new(),
            rooms: names
                .iter()
                .map(|n| std::path::PathBuf::from(format!("a/{n}.room.ron")))
                .collect(),
            animations: Default::default(),
            locales: Default::default(),
            sheets: Vec::new(),
            scripts: Default::default(),
            script_errors: Vec::new(),
            screens: Vec::new(),
            fonts: Vec::new(),
            sounds: Default::default(),
            manifest: None,
        }
    }

    #[test]
    fn the_filter_ignores_case_and_an_empty_one_keeps_everything() {
        let names = ["intro_sky.png", "note.en.png"];
        assert_eq!(shown("", names.iter().copied()).len(), 2);
        assert_eq!(shown("SKY", names.iter().copied()), vec!["intro_sky.png"]);
        assert!(shown("zzz", names.iter().copied()).is_empty());
    }

    #[test]
    fn an_empty_room_filter_matches_every_room() {
        let project = project_of(&["alchemy", "bridge"]);
        assert_eq!(matching_rooms(&project, ""), vec![0, 1]);
        assert_eq!(matching_rooms(&project, "   "), vec![0, 1]);
    }

    #[test]
    fn the_room_filter_ignores_case_and_matches_anywhere_in_the_name() {
        let project = project_of(&["alchemy", "bridge", "catacom"]);
        assert_eq!(matching_rooms(&project, "RID"), vec![1]);
        assert_eq!(matching_rooms(&project, "c"), vec![0, 2]);
        assert!(matching_rooms(&project, "zzz").is_empty());
    }

    #[test]
    fn a_family_is_the_name_before_the_numbered_suffix() {
        assert_eq!(family("gencavb_127"), "gencavb");
        assert_eq!(family("bridge"), "bridge");
        assert_eq!(family("_intro"), "_intro", "no name before the underscore");
        assert_eq!(family("note_v2_final"), "note_v2_final", "letters in the tail");
    }

    #[test]
    fn grouping_keeps_first_seen_order_and_gathers_families() {
        let labels = vec![
            (0, "bridge".to_string()),
            (1, "gencavb_127".to_string()),
            (2, "gencavb_128".to_string()),
            (3, "well".to_string()),
        ];
        let groups = group_by_family(&labels);
        assert_eq!(groups.len(), 3);
        assert_eq!(groups[0].0, "bridge");
        assert_eq!(groups[1].0, "gencavb");
        assert_eq!(groups[1].1.len(), 2);
        assert_eq!(groups[2].0, "well");
    }

    #[test]
    fn actions_default_to_doing_nothing() {
        let actions = LibraryActions::default();
        assert!(actions.add.is_none() && actions.preview.is_none());
        assert!(actions.open_script.is_none() && !actions.edit_screens);
    }
}
