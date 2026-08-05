//! A97: a bitmap font, opened and looked at.
//!
//! The Fonts shelf could list a font and nothing else - you could see
//! that `8fat` existed and had 96 glyphs, and that was the end of what
//! the editor would tell you. But a bitmap font is exactly the kind of
//! asset that needs looking at: whether the ink came out solid, which
//! accented characters the game's own encoding actually carries, how
//! wide a line of it runs.
//!
//! So a font opens on the workbench like an animation: the atlas at a
//! readable zoom with every glyph's cell drawn on it, the character
//! table, and a line of sample text drawn from the atlas itself - the
//! same glyphs the game will draw, at the size it will draw them.

use infengine_adventure::pixfont::PixFont;

use crate::lang::t;

/// The font on the workbench.
#[derive(Debug, Clone, PartialEq)]
pub struct FontDesk {
    pub font: PixFont,
    /// What it was called when opened, so a rename replaces the right
    /// file rather than leaving both.
    pub original_id: String,
    /// The line drawn in the sample.
    pub sample: String,
    /// How big the sample is drawn, in whole multiples.
    pub sample_scale: f32,
    /// How big the atlas is drawn.
    pub atlas_scale: f32,
    /// Whether the glyph cells are outlined on the atlas.
    pub show_cells: bool,
    pub dirty: bool,
}

impl FontDesk {
    pub fn open(font: PixFont) -> Self {
        Self {
            original_id: font.id.clone(),
            sample: "The quick brown fox 0123".to_string(),
            sample_scale: 2.0,
            atlas_scale: 2.0,
            show_cells: true,
            dirty: false,
            font,
        }
    }

    /// Characters the font has, in order.
    pub fn characters(&self) -> String {
        self.font.glyphs.keys().collect()
    }

    /// Which of the sample's characters this font cannot draw.
    ///
    /// The question a translator asks first: a Czech line drawn with a
    /// font that has no ě is a line with holes in it, and finding out
    /// by reading the screen is the slow way.
    pub fn missing(&self) -> Vec<char> {
        let mut gone: Vec<char> = self
            .sample
            .chars()
            .filter(|c| *c != ' ' && !self.font.glyphs.contains_key(c))
            .collect();
        gone.sort_unstable();
        gone.dedup();
        gone
    }
}

/// What the workspace asked the app to do.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct FontDeskActions {
    pub save: bool,
    pub close: bool,
}

/// Draw the workspace. `atlas` is the font's own image.
pub fn panel(
    ctx: &egui::Context,
    desk: &mut FontDesk,
    atlas: Option<egui::TextureHandle>,
) -> FontDeskActions {
    let mut actions = FontDeskActions::default();
    egui::CentralPanel::default().show(ctx, |ui| {
        header(ui, desk, &mut actions);
        ui.separator();
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width((ui.available_width() - 300.0).max(200.0));
                egui::ScrollArea::both()
                    .id_salt("font_atlas")
                    .auto_shrink([false, false])
                    .show(ui, |ui| atlas_view(ui, desk, atlas.as_ref()));
            });
            ui.separator();
            ui.vertical(|ui| {
                ui.set_width(284.0);
                egui::ScrollArea::vertical()
                    .id_salt("font_fields")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        sample(ui, desk, atlas.as_ref());
                        ui.separator();
                        fields(ui, desk);
                    });
            });
        });
    });
    actions
}

fn header(ui: &mut egui::Ui, desk: &mut FontDesk, actions: &mut FontDeskActions) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(t("FONT", "PÍSMO")).small().weak());
        ui.heading(&desk.font.id);
        ui.label(
            egui::RichText::new(if crate::lang::is_czech() {
                format!("{} znaků · řádek {}px", desk.font.glyphs.len(), desk.font.line_height)
            } else {
                format!(
                    "{} glyphs · line {}px",
                    desk.font.glyphs.len(),
                    desk.font.line_height
                )
            })
            .small()
            .weak(),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button(t("Close", "Zavřít")).clicked() {
                actions.close = true;
            }
            if ui
                .add_enabled(desk.dirty, egui::Button::new(t("Save", "Uložit")))
                .on_disabled_hover_text(t("Nothing has changed", "Nic se nezměnilo"))
                .clicked()
            {
                actions.save = true;
            }
            if desk.dirty {
                ui.label(
                    egui::RichText::new(t("● unsaved", "● neuloženo"))
                        .small()
                        .color(egui::Color32::from_rgb(255, 200, 120)),
                );
            }
        });
    });
}

/// The atlas, with every glyph's cell drawn on it.
fn atlas_view(ui: &mut egui::Ui, desk: &FontDesk, atlas: Option<&egui::TextureHandle>) {
    let Some(texture) = atlas else {
        ui.label(
            egui::RichText::new(t("The atlas did not load.", "Atlas se nenačetl."))
                .weak(),
        );
        return;
    };
    let scale = desk.atlas_scale;
    let size = texture.size_vec2() * scale;
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let painter = ui.painter_at(ui.clip_rect().intersect(rect));
    painter.rect_filled(rect, 0.0, egui::Color32::from_gray(18));
    egui::Image::new((texture.id(), size)).paint_at(ui, rect);

    if !desk.show_cells {
        return;
    }
    for glyph in desk.font.glyphs.values() {
        let cell = egui::Rect::from_min_size(
            rect.min + egui::vec2(glyph.x as f32 * scale, glyph.y as f32 * scale),
            egui::vec2(glyph.w as f32 * scale, desk.font.line_height as f32 * scale),
        );
        painter.rect_stroke(
            cell,
            0.0,
            egui::Stroke::new(1.0, egui::Color32::from_white_alpha(40)),
            egui::StrokeKind::Inside,
        );
    }
}

/// A line of the font's own glyphs, drawn from the atlas.
fn sample(ui: &mut egui::Ui, desk: &mut FontDesk, atlas: Option<&egui::TextureHandle>) {
    ui.label(egui::RichText::new(t("Sample", "Ukázka")).small().weak());
    if ui.text_edit_singleline(&mut desk.sample).changed() {
        // The sample is the workbench's own, not the font's, so typing
        // in it does not make the asset unsaved.
    }
    ui.add(
        egui::Slider::new(&mut desk.sample_scale, 1.0..=6.0)
            .text(t("drawn at", "kreslit v")),
    );

    let Some(texture) = atlas else {
        return;
    };
    let scale = desk.sample_scale.max(1.0);
    let line = desk.font.line_height.max(1) as f32 * scale;
    // Measured first, so the strip is exactly as wide as the text.
    let width: f32 = desk
        .sample
        .chars()
        .map(|c| match desk.font.glyphs.get(&c) {
            Some(glyph) => (glyph.w + desk.font.letter_spacing) as f32 * scale,
            None if c == ' ' => {
                let space = if desk.font.space_width > 0 {
                    desk.font.space_width
                } else {
                    desk.font.line_height / 2
                };
                space as f32 * scale
            }
            None => 0.0,
        })
        .sum();

    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(width.max(8.0), line), egui::Sense::hover());
    ui.painter().rect_filled(rect, 2.0, egui::Color32::from_gray(18));
    let sheet = texture.size_vec2();
    let mut pen = rect.min.x;
    for c in desk.sample.chars() {
        match desk.font.glyphs.get(&c) {
            Some(glyph) => {
                let drawn = egui::vec2(glyph.w as f32 * scale, line);
                let at = egui::Rect::from_min_size(egui::pos2(pen, rect.min.y), drawn);
                let uv = egui::Rect::from_min_max(
                    egui::pos2(glyph.x as f32 / sheet.x, glyph.y as f32 / sheet.y),
                    egui::pos2(
                        (glyph.x + glyph.w) as f32 / sheet.x,
                        (glyph.y + desk.font.line_height) as f32 / sheet.y,
                    ),
                );
                egui::Image::new((texture.id(), drawn))
                    .uv(uv)
                    .paint_at(ui, at);
                pen += (glyph.w + desk.font.letter_spacing) as f32 * scale;
            }
            None => {
                let space = if desk.font.space_width > 0 {
                    desk.font.space_width
                } else {
                    desk.font.line_height / 2
                };
                pen += space as f32 * scale;
            }
        }
    }

    let missing = desk.missing();
    if !missing.is_empty() {
        ui.label(
            egui::RichText::new(if crate::lang::is_czech() {
                format!("chybí: {}", missing.iter().collect::<String>())
            } else {
                format!("missing: {}", missing.iter().collect::<String>())
            })
            .small()
            .color(egui::Color32::from_rgb(255, 160, 120)),
        );
    }
}

/// The metrics, and the characters the font carries.
fn fields(ui: &mut egui::Ui, desk: &mut FontDesk) {
    ui.horizontal(|ui| {
        ui.label(t("name", "název"));
        desk.dirty |= ui.text_edit_singleline(&mut desk.font.id).changed();
    });
    ui.horizontal(|ui| {
        ui.label(t("line height", "výška řádku"));
        desk.dirty |= ui
            .add(egui::DragValue::new(&mut desk.font.line_height).range(1..=128))
            .changed();
    });
    ui.horizontal(|ui| {
        ui.label(t("letter spacing", "mezera mezi písmeny"))
            .on_hover_text(t(
                "Extra pixels after every glyph. These fonts usually carry their own.",
                "Pixely navíc za každým znakem. Tahle písma je většinou mají v sobě.",
            ));
        desk.dirty |= ui
            .add(egui::DragValue::new(&mut desk.font.letter_spacing).range(0..=16))
            .changed();
    });
    ui.horizontal(|ui| {
        ui.label(t("space width", "šířka mezery"))
            .on_hover_text(t(
                "Zero falls back to half the line height.",
                "Nula znamená polovinu výšky řádku.",
            ));
        desk.dirty |= ui
            .add(egui::DragValue::new(&mut desk.font.space_width).range(0..=64))
            .changed();
    });

    ui.add_space(6.0);
    ui.add(
        egui::Slider::new(&mut desk.atlas_scale, 1.0..=8.0)
            .text(t("atlas zoom", "lupa atlasu")),
    );
    ui.checkbox(&mut desk.show_cells, t("show the cells", "ukázat buňky"));

    ui.add_space(6.0);
    ui.label(
        egui::RichText::new(t("Characters it carries", "Znaky, které umí"))
            .small()
            .weak(),
    );
    ui.label(egui::RichText::new(desk.characters()).monospace().small());
}

#[cfg(test)]
mod tests {
    use super::*;
    use infengine_adventure::pixfont::PixGlyph;

    fn font() -> PixFont {
        let mut glyphs = std::collections::BTreeMap::new();
        for (index, c) in "abc".chars().enumerate() {
            glyphs.insert(
                c,
                PixGlyph {
                    x: index as u32 * 8,
                    y: 0,
                    w: 8,
                },
            );
        }
        PixFont {
            id: "tiny".into(),
            image: "fonts/tiny.png".into(),
            line_height: 8,
            letter_spacing: 0,
            space_width: 0,
            glyphs,
        }
    }

    #[test]
    fn the_characters_read_in_order() {
        let desk = FontDesk::open(font());
        assert_eq!(desk.characters(), "abc");
    }

    #[test]
    fn a_sample_reports_what_the_font_cannot_draw() {
        // The question a translator asks first.
        let mut desk = FontDesk::open(font());
        desk.sample = "a b z".into();
        assert_eq!(desk.missing(), vec!['z'], "spaces are not missing glyphs");
        desk.sample = "abc".into();
        assert!(desk.missing().is_empty());
    }

    #[test]
    fn opening_remembers_the_id_so_a_rename_replaces_the_file() {
        let mut desk = FontDesk::open(font());
        desk.font.id = "small".into();
        assert_eq!(desk.original_id, "tiny");
    }
}
