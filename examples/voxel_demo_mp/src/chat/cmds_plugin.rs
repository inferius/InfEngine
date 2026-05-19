//! `/plugins` chat command + sub-commands.
//!
//! Sub-verbs:
//! - `/plugins` (perm 0): list every loaded plugin with name +
//!   version + disabled state.
//! - `/plugins reload <name>` (perm 4): unload + reload a plugin.
//! - `/plugins disable <name>` (perm 4): mark a plugin disabled
//!   so its callbacks are skipped on future events.
//!
//! Perm gating: `/plugins` itself is level 0 so any player can
//! audit who's running what. The mutating sub-verbs require an
//! explicit elevated level check INSIDE the cmd fn (the
//! top-level dispatcher only sees `/plugins` and can't tell which
//! sub-verb the user typed).

use crate::Voxel;

/// `/plugins [reload|disable] [name]`. See the module docs.
pub(crate) fn cmd_plugins(
    voxel: &mut Voxel,
    arg: &str,
    sender_level: u8,
    lines: &mut Vec<String>,
) -> bool {
    let arg = arg.trim();
    if arg.is_empty() {
        // Plain `/plugins` - list everyone.
        return list_plugins(voxel, lines);
    }
    // Parse `<verb> <name>`.
    let mut parts = arg.splitn(2, char::is_whitespace);
    let verb = parts.next().unwrap_or("").to_lowercase();
    let target = parts.next().unwrap_or("").trim();
    match verb.as_str() {
        "list" | "ls" => list_plugins(voxel, lines),
        "reload" => {
            if sender_level < 4 {
                lines.push(
                    "/plugins reload requires permission level 4."
                        .to_string(),
                );
                return true;
            }
            reload_plugin(voxel, target, lines)
        }
        "disable" => {
            if sender_level < 4 {
                lines.push(
                    "/plugins disable requires permission level 4."
                        .to_string(),
                );
                return true;
            }
            disable_plugin(voxel, target, lines)
        }
        _ => {
            lines.push(
                "Usage: /plugins [list | reload <name> | disable <name>]"
                    .to_string(),
            );
            true
        }
    }
}

fn list_plugins(voxel: &mut Voxel, lines: &mut Vec<String>) -> bool {
    let Some(reg) = voxel.plugins.as_ref() else {
        lines.push("(plugin registry not initialised)".to_string());
        return true;
    };
    if reg.plugins.is_empty() {
        lines.push("(no plugins loaded)".to_string());
        return true;
    }
    lines.push(format!("Loaded plugins ({}):", reg.plugins.len()));
    for p in &reg.plugins {
        let state = if p.disabled {
            let reason = p
                .disable_reason
                .as_deref()
                .unwrap_or("disabled");
            format!(" [DISABLED: {reason}]")
        } else {
            String::new()
        };
        lines.push(format!("  {} v{}{}", p.name, p.version, state));
    }
    true
}

fn reload_plugin(
    voxel: &mut Voxel,
    name: &str,
    lines: &mut Vec<String>,
) -> bool {
    if name.is_empty() {
        lines.push("Usage: /plugins reload <name>".to_string());
        return true;
    }
    let Some(reg) = voxel.plugins.as_mut() else {
        lines.push("(plugin registry not initialised)".to_string());
        return true;
    };
    let Some(idx) = reg.find_index(name) else {
        lines.push(format!("Plugin not found: {name}"));
        return true;
    };
    match reg.reload(idx) {
        Ok(()) => lines.push(format!("Reloaded plugin '{name}'.")),
        Err(e) => lines.push(format!("Reload failed: {e}")),
    }
    true
}

fn disable_plugin(
    voxel: &mut Voxel,
    name: &str,
    lines: &mut Vec<String>,
) -> bool {
    if name.is_empty() {
        lines.push("Usage: /plugins disable <name>".to_string());
        return true;
    }
    let Some(reg) = voxel.plugins.as_mut() else {
        lines.push("(plugin registry not initialised)".to_string());
        return true;
    };
    let Some(idx) = reg.find_index(name) else {
        lines.push(format!("Plugin not found: {name}"));
        return true;
    };
    let plug = &mut reg.plugins[idx];
    if plug.disabled {
        lines.push(format!("Plugin '{name}' already disabled."));
    } else {
        plug.disable("disabled via /plugins disable");
        lines.push(format!("Disabled plugin '{name}'."));
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Voxel;

    fn make_voxel() -> Voxel {
        Voxel::new()
    }

    #[test]
    fn list_plugins_empty_registry() {
        let mut voxel = make_voxel();
        let mut lines = Vec::new();
        assert!(cmd_plugins(&mut voxel, "", 0, &mut lines));
        assert_eq!(lines, vec!["(no plugins loaded)".to_string()]);
    }

    #[test]
    fn reload_requires_level_4() {
        let mut voxel = make_voxel();
        let mut lines = Vec::new();
        assert!(cmd_plugins(&mut voxel, "reload foo", 3, &mut lines));
        assert!(lines.iter().any(|l| l.contains("permission level 4")));
    }

    #[test]
    fn disable_requires_level_4() {
        let mut voxel = make_voxel();
        let mut lines = Vec::new();
        assert!(cmd_plugins(&mut voxel, "disable foo", 3, &mut lines));
        assert!(lines.iter().any(|l| l.contains("permission level 4")));
    }

    #[test]
    fn reload_missing_name_usage() {
        let mut voxel = make_voxel();
        let mut lines = Vec::new();
        assert!(cmd_plugins(&mut voxel, "reload", 4, &mut lines));
        assert!(lines.iter().any(|l| l.starts_with("Usage:")));
    }

    #[test]
    fn disable_missing_name_usage() {
        let mut voxel = make_voxel();
        let mut lines = Vec::new();
        assert!(cmd_plugins(&mut voxel, "disable", 4, &mut lines));
        assert!(lines.iter().any(|l| l.starts_with("Usage:")));
    }

    #[test]
    fn reload_missing_plugin_reports() {
        let mut voxel = make_voxel();
        let mut lines = Vec::new();
        assert!(cmd_plugins(&mut voxel, "reload xyzzy", 4, &mut lines));
        assert!(lines.iter().any(|l| l.contains("not found")));
    }

    #[test]
    fn disable_missing_plugin_reports() {
        let mut voxel = make_voxel();
        let mut lines = Vec::new();
        assert!(cmd_plugins(&mut voxel, "disable xyzzy", 4, &mut lines));
        assert!(lines.iter().any(|l| l.contains("not found")));
    }

    #[test]
    fn list_verb_alias_works() {
        let mut voxel = make_voxel();
        let mut lines = Vec::new();
        assert!(cmd_plugins(&mut voxel, "list", 0, &mut lines));
        assert_eq!(lines, vec!["(no plugins loaded)".to_string()]);
    }

    #[test]
    fn unknown_subverb_prints_usage() {
        let mut voxel = make_voxel();
        let mut lines = Vec::new();
        assert!(cmd_plugins(&mut voxel, "bogus", 4, &mut lines));
        assert!(lines.iter().any(|l| l.starts_with("Usage:")));
    }
}
