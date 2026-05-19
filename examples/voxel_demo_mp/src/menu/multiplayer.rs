//! Multiplayer (server list) screen.

use egui::{Color32, RichText, ScrollArea, Vec2};

use infengine_net::master::ServerInfo;

use super::{
    AddServerForm, DirectConnectForm, MenuAction, MenuContext, MenuScreen,
    SavedServer, shared,
};

/// Render the multiplayer server list. The LAN / master query
/// results live in `ctx.browser_servers`; the user-curated entries
/// live in `ctx.saved_servers`. Both are surfaced as separate
/// section headers in one scroll area.
#[allow(clippy::too_many_arguments)]
pub(crate) fn draw(
    ui: &mut egui::Ui,
    ctx: &mut MenuContext<'_>,
    selected_addr: &mut Option<String>,
    search: &mut String,
    direct_connect_open: &mut Option<DirectConnectForm>,
    add_server_open: &mut Option<AddServerForm>,
    confirm_remove: &mut Option<String>,
    actions: &mut Vec<MenuAction>,
) {
    ui.vertical_centered(|ui| {
        ui.add_space(20.0);
        ui.heading("Hra vice hracu");
        ui.add_space(4.0);
        // Status / search row.
        ui.horizontal(|ui| {
            ui.label("Hledat:");
            ui.add_sized(
                Vec2::new(220.0, 20.0),
                egui::TextEdit::singleline(search),
            );
        });
        ui.label(status_line(ctx));
        ui.add_space(4.0);
    });

    let needle = search.to_lowercase();
    let lan_filtered: Vec<&ServerInfo> = ctx
        .browser_servers
        .iter()
        .filter(|s| {
            needle.is_empty()
                || s.name.to_lowercase().contains(&needle)
                || s.addr.to_lowercase().contains(&needle)
        })
        .collect();
    let saved_filtered: Vec<&SavedServer> = ctx
        .saved_servers
        .iter()
        .filter(|s| {
            needle.is_empty()
                || s.name.to_lowercase().contains(&needle)
                || s.addr.to_lowercase().contains(&needle)
        })
        .collect();

    ScrollArea::vertical()
        .id_salt("servers_list")
        .max_height(280.0)
        .show(ui, |ui| {
            ui.heading(RichText::new("Lokalni servery").size(15.0));
            if lan_filtered.is_empty() {
                ui.label(
                    RichText::new("(zadne servery z LAN / master)")
                        .italics()
                        .small(),
                );
            } else {
                for s in &lan_filtered {
                    draw_lan_row(ui, s, selected_addr);
                }
            }
            ui.separator();
            ui.heading(RichText::new("Pridane servery").size(15.0));
            if saved_filtered.is_empty() {
                ui.label(
                    RichText::new("(klikni 'Pridat server' pro pridani)")
                        .italics()
                        .small(),
                );
            } else {
                for s in &saved_filtered {
                    draw_saved_row(ui, s, selected_addr);
                }
            }
        });

    // Bottom button bar.
    let has_selection = selected_addr.is_some();
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        if shared::big_button(ui, "Pripojit se", has_selection)
            && let Some(addr) = selected_addr.as_ref()
        {
            actions.push(MenuAction::JoinServer { addr: addr.clone() });
        }
        if shared::small_button(ui, "Prime spojeni") && direct_connect_open.is_none()
        {
            *direct_connect_open = Some(DirectConnectForm::default());
        }
    });
    ui.horizontal(|ui| {
        if shared::small_button(ui, "Pridat server") && add_server_open.is_none() {
            *add_server_open = Some(AddServerForm::default());
        }
        if shared::small_button(ui, "Upravit") {
            // TODO Phase G.7+: rename / re-IP a saved server.
        }
        if shared::small_button(ui, "Odstranit")
            && let Some(addr) = selected_addr.as_ref()
            && ctx.saved_servers.iter().any(|s| s.addr == *addr)
        {
            *confirm_remove = Some(addr.clone());
        }
        if shared::small_button(ui, "Obnovit") {
            actions.push(MenuAction::BrowseServersRefresh);
        }
        if shared::small_button(ui, "Zpet") {
            actions.push(MenuAction::SwitchScreen(MenuScreen::Main));
        }
    });

    // Direct-connect modal.
    if let Some(form) = direct_connect_open.as_mut() {
        let mut keep_open = true;
        let mut commit: Option<String> = None;
        egui::Window::new("Prime spojeni")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ui.ctx(), |ui| {
                ui.horizontal(|ui| {
                    ui.label("Adresa:");
                    ui.add_sized(
                        Vec2::new(200.0, 20.0),
                        egui::TextEdit::singleline(&mut form.addr)
                            .hint_text("host:port"),
                    );
                });
                let valid = !form.addr.trim().is_empty();
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(valid, egui::Button::new("Pripojit"))
                        .clicked()
                    {
                        commit = Some(form.addr.trim().to_string());
                        keep_open = false;
                    }
                    if ui.button("Zrusit").clicked() {
                        keep_open = false;
                    }
                });
            });
        if let Some(addr) = commit {
            actions.push(MenuAction::DirectConnect { addr });
        }
        if !keep_open {
            *direct_connect_open = None;
        }
    }

    // Add-server modal.
    if let Some(form) = add_server_open.as_mut() {
        let mut keep_open = true;
        let mut commit: Option<(String, String)> = None;
        egui::Window::new("Pridat server")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ui.ctx(), |ui| {
                ui.horizontal(|ui| {
                    ui.label("Nazev:");
                    ui.add_sized(
                        Vec2::new(200.0, 20.0),
                        egui::TextEdit::singleline(&mut form.name),
                    );
                });
                ui.horizontal(|ui| {
                    ui.label("Adresa:");
                    ui.add_sized(
                        Vec2::new(200.0, 20.0),
                        egui::TextEdit::singleline(&mut form.addr)
                            .hint_text("host:port"),
                    );
                });
                let valid =
                    !form.name.trim().is_empty() && !form.addr.trim().is_empty();
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(valid, egui::Button::new("Pridat"))
                        .clicked()
                    {
                        commit = Some((
                            form.name.trim().to_string(),
                            form.addr.trim().to_string(),
                        ));
                        keep_open = false;
                    }
                    if ui.button("Zrusit").clicked() {
                        keep_open = false;
                    }
                });
            });
        if let Some((name, addr)) = commit {
            actions.push(MenuAction::AddSavedServer { name, addr });
        }
        if !keep_open {
            *add_server_open = None;
        }
    }

    // Remove-saved-server confirm modal.
    if let Some(addr) = confirm_remove.clone() {
        let display = ctx
            .saved_servers
            .iter()
            .find(|s| s.addr == addr)
            .map(|s| s.name.clone())
            .unwrap_or_else(|| addr.clone());
        let mut keep_open = true;
        let mut do_remove = false;
        egui::Window::new("Odstranit server")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ui.ctx(), |ui| {
                ui.label(format!("Odstranit '{display}' ze seznamu?"));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    let red = ui.add(egui::Button::new(
                        RichText::new("Odstranit")
                            .color(Color32::from_rgb(255, 200, 200)),
                    ));
                    if red.clicked() {
                        do_remove = true;
                        keep_open = false;
                    }
                    if ui.button("Zrusit").clicked() {
                        keep_open = false;
                    }
                });
            });
        if do_remove {
            actions.push(MenuAction::RemoveSavedServer { addr });
            *selected_addr = None;
        }
        if !keep_open {
            *confirm_remove = None;
        }
    }

    shared::footer_identity(ui, &ctx.options.player_name);
    shared::footer_version(ui.ctx(), ctx.engine_version);
}

/// One-line status banner: in-flight query / error / count + age.
fn status_line(ctx: &MenuContext<'_>) -> RichText {
    if ctx.browser_querying {
        return RichText::new("[...] querying master")
            .color(Color32::from_rgb(140, 180, 220));
    }
    if let Some(err) = ctx.browser_error {
        return RichText::new(format!("[ERR] {err}"))
            .color(Color32::from_rgb(220, 100, 100));
    }
    RichText::new(format!(
        "[OK] {} server(s) z LAN / master, {} ulozenych",
        ctx.browser_servers.len(),
        ctx.saved_servers.len()
    ))
    .color(Color32::from_rgb(140, 220, 140))
}

/// LAN / master scan row.
fn draw_lan_row(
    ui: &mut egui::Ui,
    s: &ServerInfo,
    selected_addr: &mut Option<String>,
) {
    let is_selected = selected_addr.as_deref() == Some(s.addr.as_str());
    draw_server_row_inner(
        ui,
        &s.addr,
        &s.name,
        Some(format!("{}/{}", s.players, s.max_players)),
        if s.mode.is_empty() { None } else { Some(s.mode.clone()) },
        is_selected,
        selected_addr,
        false,
    );
}

/// User-saved row. Trimmed metadata since we haven't pinged it.
fn draw_saved_row(
    ui: &mut egui::Ui,
    s: &SavedServer,
    selected_addr: &mut Option<String>,
) {
    let is_selected = selected_addr.as_deref() == Some(s.addr.as_str());
    draw_server_row_inner(
        ui, &s.addr, &s.name, None, None, is_selected, selected_addr, true,
    );
}

#[allow(clippy::too_many_arguments)]
fn draw_server_row_inner(
    ui: &mut egui::Ui,
    addr: &str,
    name: &str,
    player_line: Option<String>,
    mode: Option<String>,
    is_selected: bool,
    selected_addr: &mut Option<String>,
    is_saved: bool,
) {
    let frame = if is_selected {
        egui::Frame::group(ui.style())
            .stroke(egui::Stroke::new(2.0, Color32::from_rgb(120, 200, 255)))
    } else {
        egui::Frame::group(ui.style())
    };
    let resp = frame
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                shared::placeholder_thumb(ui, addr);
                ui.vertical(|ui| {
                    ui.label(RichText::new(name).strong().size(15.0));
                    let mut meta_parts: Vec<String> = vec![addr.to_string()];
                    if let Some(p) = player_line.as_ref() {
                        meta_parts.push(p.clone());
                    }
                    if let Some(m) = mode.as_ref() {
                        meta_parts.push(m.clone());
                    }
                    if is_saved {
                        meta_parts.push("ulozeny".to_string());
                    }
                    ui.label(
                        RichText::new(meta_parts.join("  -  "))
                            .small()
                            .color(Color32::from_gray(180)),
                    );
                });
            });
        })
        .response
        .interact(egui::Sense::click());
    if resp.clicked() {
        *selected_addr = Some(addr.to_string());
    }
}

/// Saved-servers persistence path: `worlds_dir()/../servers.json`.
/// Sits next to the worlds tree so a backup of the user's data
/// directory captures both.
pub(crate) fn saved_servers_path() -> std::path::PathBuf {
    let worlds = crate::world_save::layout::worlds_dir();
    let mut parent = worlds
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    parent.push("servers.json");
    parent
}

/// Load the user-saved server list. Missing file = empty list;
/// parse failures fall back to empty + log once.
pub(crate) fn load_saved_servers() -> Vec<SavedServer> {
    let path = saved_servers_path();
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(_) => return Vec::new(),
    };
    match serde_json::from_slice::<Vec<SavedServer>>(&bytes) {
        Ok(v) => v,
        Err(err) => {
            tracing::warn!(
                error = %err,
                path = %path.display(),
                "servers.json parse failed; using empty list"
            );
            Vec::new()
        }
    }
}

/// Persist the user-saved server list via the existing atomic-save
/// helper. Errors are logged but not surfaced - the worst case is
/// a manually-added entry vanishing on next launch.
pub(crate) fn save_saved_servers(entries: &[SavedServer]) {
    let path = saved_servers_path();
    let payload = match serde_json::to_vec_pretty(entries) {
        Ok(p) => p,
        Err(err) => {
            tracing::warn!(
                error = %err,
                path = %path.display(),
                "servers.json serialize failed"
            );
            return;
        }
    };
    if let Err(err) = crate::world_save::atomic::atomic_save(&path, &payload) {
        tracing::warn!(
            error = %err,
            path = %path.display(),
            "servers.json save failed"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_servers_path_ends_with_servers_json() {
        let p = saved_servers_path();
        assert!(p.ends_with("servers.json"), "got {}", p.display());
    }

    #[test]
    fn save_and_load_roundtrip() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("servers.json");
        let entries = vec![
            SavedServer {
                name: "Home".to_string(),
                addr: "127.0.0.1:7777".to_string(),
            },
            SavedServer {
                name: "Friend".to_string(),
                addr: "10.0.0.5:7780".to_string(),
            },
        ];
        let payload = serde_json::to_vec_pretty(&entries).unwrap();
        crate::world_save::atomic::atomic_save(&path, &payload).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let read: Vec<SavedServer> = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(read, entries);
    }
}
