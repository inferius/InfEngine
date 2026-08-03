//! A41.5: what moves.
//!
//! Animations and the sheets they come from, in that order, because the
//! sheet is where an animation comes from and the list is where it ends
//! up. Cutting a sheet takes over the tab while it is open: it is one
//! job with its own controls, and leaving the rest visible underneath
//! only invites clicking on the wrong thing.

use crate::ui::{UiActions, UiState};

pub fn show(ui: &mut egui::Ui, state: &mut UiState<'_>, actions: &mut UiActions) {
    if let Some(draft) = state.sheet.as_mut() {
        cutting(ui, state.project, draft, actions);
        return;
    }

    animations(ui, state, actions);
    ui.add_space(10.0);
    sheets(ui, state, actions);
}

fn animations(ui: &mut egui::Ui, state: &mut UiState<'_>, actions: &mut UiActions) {
    ui.heading(format!("Animations ({})", state.animations.len()));
    if state.animations.is_empty() {
        super::nothing_yet(
            ui,
            "Nothing moves in this room.",
            "Cut one out of a sheet below, or it stays a still picture.",
        );
        return;
    }

    ui.label(
        egui::RichText::new("Click one to play it over the room.")
            .small()
            .weak(),
    );
    egui::ScrollArea::vertical()
        .id_salt("animation_list")
        .max_height(160.0)
        .show(ui, |ui| {
            for animation in state.animations {
                let selected = state.playback.selected.as_deref() == Some(&animation.id);
                let label = format!(
                    "{}  ({} frames, {}x{})",
                    animation.id,
                    animation.frames,
                    animation.frame_size.x as i32,
                    animation.frame_size.y as i32
                );
                if ui.selectable_label(selected, label).clicked() {
                    actions.play_animation = Some(animation.id.clone());
                }
            }
        });

    let Some(current) = state
        .playback
        .selected
        .as_deref()
        .and_then(|id| state.animations.iter().find(|a| a.id == id))
    else {
        return;
    };

    ui.separator();
    ui.horizontal(|ui| {
        let playing = state.playback.playing;
        if ui.button(if playing { "Pause" } else { "Play" }).clicked() {
            state.playback.playing = !playing;
        }
        if ui.button("<").on_hover_text("Previous frame").clicked() {
            state.playback.step(-1, current);
        }
        if ui.button(">").on_hover_text("Next frame").clicked() {
            state.playback.step(1, current);
        }
        if ui.button("Stop").clicked() {
            state.playback.clear();
        }
    });

    let mut frame = state.playback.frame(current);
    let last = current.frames.saturating_sub(1);
    if ui
        .add(egui::Slider::new(&mut frame, 0..=last).text("frame"))
        .changed()
    {
        state.playback.scrub_to(frame, current);
    }
    ui.add(egui::Slider::new(&mut state.playback.speed, 0.1..=4.0).text("speed"));
}

fn sheets(ui: &mut egui::Ui, state: &mut UiState<'_>, actions: &mut UiActions) {
    ui.heading(format!("Sheets ({})", state.project.sheets.len()));
    if state.project.sheets.is_empty() {
        super::nothing_yet(
            ui,
            "No loose images to cut up.",
            "A sheet is a strip of frames under sprites/ in the project folder.",
        );
        return;
    }
    ui.label(
        egui::RichText::new("A strip of frames. Open one to cut it into an animation.")
            .small()
            .weak(),
    );
    egui::ScrollArea::vertical()
        .id_salt("sheet_list")
        .max_height(140.0)
        .show(ui, |ui| {
            for sheet in &state.project.sheets {
                let name = sheet.rsplit('/').next().unwrap_or(sheet.as_str());
                if ui.selectable_label(false, name).on_hover_text(sheet).clicked() {
                    actions.open_sheet = Some(sheet.clone());
                }
            }
        });
}

/// The sheet as an interface texture, cached per image.
///
/// The canvas has its own copy for drawing the room; this one exists
/// so the cutter can show the animation playing right in the panel,
/// which is the only way to tune a cut by eye.
fn sheet_texture(
    ui: &egui::Ui,
    project: &infengine_adventure::Project,
    image: &str,
) -> Option<egui::TextureHandle> {
    let key = egui::Id::new(("sheet_preview", image));
    if let Some(handle) = ui.ctx().data(|d| d.get_temp::<egui::TextureHandle>(key)) {
        return Some(handle);
    }
    let loaded = image::open(project.resolve(image)).ok()?;
    let rgba = loaded.to_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    let pixels = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
    let handle = ui.ctx().load_texture(
        format!("cutter:{image}"),
        pixels,
        egui::TextureOptions::NEAREST,
    );
    ui.ctx().data_mut(|d| d.insert_temp(key, handle.clone()));
    Some(handle)
}

/// The draft playing, so the cut is judged by the result.
fn preview(ui: &mut egui::Ui, project: &infengine_adventure::Project, draft: &crate::sheet::SheetDraft) {
    let Some(texture) = sheet_texture(ui, project, &draft.image) else {
        return;
    };
    let animation = draft.to_animation();
    let now = ui.input(|i| i.time) as f32;
    let frame = animation.frame_at(now);
    let (uv_min, uv_max) = animation.frame_uv(frame, draft.sheet_size);

    let scale = (96.0 / draft.frame_size.x.max(1.0))
        .min(96.0 / draft.frame_size.y.max(1.0))
        .clamp(0.25, 4.0);
    let shown = egui::vec2(draft.frame_size.x * scale, draft.frame_size.y * scale);
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(shown, egui::Sense::hover());
        ui.painter()
            .rect_filled(rect, 2.0, egui::Color32::from_gray(25));
        egui::Image::new((texture.id(), shown))
            .uv(egui::Rect::from_min_max(
                egui::pos2(uv_min.x, uv_min.y),
                egui::pos2(uv_max.x, uv_max.y),
            ))
            .paint_at(ui, rect);
        ui.label(
            egui::RichText::new(format!("frame {} of {}", frame + 1, animation.frames))
                .small()
                .weak(),
        );
    });
    // Keep the preview moving even while the mouse rests.
    ui.ctx().request_repaint();
}

/// Cutting a sheet into frames.
fn cutting(
    ui: &mut egui::Ui,
    project: &infengine_adventure::Project,
    draft: &mut crate::sheet::SheetDraft,
    actions: &mut UiActions,
) {
    ui.heading("Cutting a sheet");
    ui.label(egui::RichText::new(&draft.image).small().weak());
    ui.label(
        egui::RichText::new("The cut is drawn over the picture as you change it.")
            .small()
            .weak(),
    );
    ui.separator();

    ui.horizontal(|ui| {
        ui.label("name");
        ui.text_edit_singleline(&mut draft.id)
            .on_hover_text("What the animation will be called.");
    });

    let sheet = draft.sheet_size;
    ui.horizontal(|ui| {
        ui.label("one frame");
        ui.add(
            egui::DragValue::new(&mut draft.frame_size.x)
                .range(1.0..=sheet.x.max(1.0))
                .speed(1.0),
        );
        ui.label("x");
        ui.add(
            egui::DragValue::new(&mut draft.frame_size.y)
                .range(1.0..=sheet.y.max(1.0))
                .speed(1.0),
        );
    });

    ui.horizontal(|ui| {
        ui.label("gap")
            .on_hover_text("Empty pixels between neighbouring frames");
        ui.add(egui::DragValue::new(&mut draft.gap.x).range(0.0..=64.0).speed(1.0));
        ui.label("x");
        ui.add(egui::DragValue::new(&mut draft.gap.y).range(0.0..=64.0).speed(1.0));
        ui.label("columns")
            .on_hover_text("Frames per row; 0 keeps one long row");
        ui.add(egui::DragValue::new(&mut draft.columns).range(0..=64).speed(1.0));
    });

    // The upper bound moves with the frame width, so the slider can
    // never promise frames that would sample past the sheet's edge.
    let most = draft.max_frames();
    draft.frames = draft.frames.clamp(1, most);
    ui.add(egui::Slider::new(&mut draft.frames, 1..=most).text("frames"));
    ui.add(egui::Slider::new(&mut draft.fps, 1.0..=24.0).text("per second"));
    ui.checkbox(&mut draft.looping, "repeats")
        .on_hover_text("Otherwise it plays once and stops on the last frame.");

    let mut in_room = draft.room.is_some();
    if ui
        .checkbox(&mut in_room, "belongs to this room")
        .on_hover_text("Otherwise it can be played from any room.")
        .changed()
    {
        draft.room = in_room.then(String::new);
    }

    ui.separator();
    // The result, playing: a cut is tuned by watching it run.
    preview(ui, project, draft);

    ui.separator();
    ui.horizontal(|ui| {
        if ui
            .add_enabled(draft.is_valid(), egui::Button::new("Keep"))
            .on_hover_text("Add it to the project's animations")
            .clicked()
        {
            actions.save_sheet = true;
        }
        if ui.button("Discard").clicked() {
            actions.close_sheet = true;
        }
    });
}
