//! Chat command permission levels + sender identity.
//!
//! Mirrors MC's op-level scheme (`level.json` permission_level
//! 0..4) so dedicated-server admins recognise the model:
//!
//! - **0** = guest: read-only / social. `/help`, `/players`,
//!   `/me`, `/whisper`, `/pos`, `/info`. Anyone connected can
//!   run these.
//! - **1** = trusted player: personal QoL that doesn't change
//!   the world. `/setname`, `/fov`.
//! - **2** = creative / movement: self-affecting gameplay
//!   shortcuts. `/tp`, `/fly`, `/give`, `/godmode`, all the
//!   emotes. Servers in survival mode would gate these to
//!   trusted ops.
//! - **3** = world editor: block-level edits + global state.
//!   `/setblock`, `/fill`, `/explode`, `/weather`, `/summon`,
//!   `/kick`, `/mute`, host-side gameplay commands.
//! - **4** = admin: irreversible server actions. `/save`,
//!   `/clearedits`, `/worldgen`, `/seed`. Console-equivalent.
//!
//! The gate lives in [`super::dispatch_as`]. Each
//! [`CmdMeta::name`] string lower-cases to match the verb the
//! dispatch match arms compare against.

/// Permission tier in [0, 4]. Higher = more privilege. Stored
/// as a plain `u8` because the only operations we do on it are
/// `<` / `>=` comparisons against the per-command requirement.
pub(crate) type PermLevel = u8;

/// One row in the command registry. Adding a new chat command
/// = adding the cmd fn in a `cmds_*.rs` file + one row here +
/// one arm in `dispatch`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct CmdMeta {
    /// Lowercase command verb, no leading `/`. Matched
    /// case-insensitively by `dispatch` after `.to_lowercase()`.
    pub name: &'static str,
    /// Required `Sender::level` to run this command. A sender
    /// with `level < required` gets a "permission denied"
    /// feedback line and the command body is never executed.
    pub level: PermLevel,
}

/// Identity + privilege of whoever invoked a chat command.
/// Offline / host path uses [`Sender::host`]; remote clients
/// build their own with their connection's permission level so
/// the dedicated server can audit who ran what.
#[derive(Debug, Clone)]
pub(crate) struct Sender {
    /// Player name for logging. Host = `"host"`. Empty string
    /// is the "system" sender used by automated dispatches.
    /// Read by the audit-log path that Phase B will wire up;
    /// currently set + never inspected so a `#[allow(dead_code)]`
    /// keeps the warning quiet until then.
    #[allow(dead_code)]
    pub name: String,
    /// Resolved permission level. See module docs for the
    /// 0..=4 scheme.
    pub level: PermLevel,
}

impl Sender {
    /// Local host / offline player - full admin level.
    pub(crate) fn host() -> Self {
        Self {
            name: "host".to_string(),
            level: 4,
        }
    }

    /// Remote player at the default gameplay level (creative
    /// shortcuts allowed; no world edits). Servers can promote
    /// individual names via /op (Phase B).
    #[allow(dead_code)]
    pub(crate) fn player(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            level: 2,
        }
    }
}

/// Per-command permission table. Order has no functional
/// effect; kept roughly by category for readability.
pub(crate) const CMD_META: &[CmdMeta] = &[
    // Level 0 - everyone.
    CmdMeta { name: "help", level: 0 },
    CmdMeta { name: "?", level: 0 },
    CmdMeta { name: "players", level: 0 },
    CmdMeta { name: "who", level: 0 },
    CmdMeta { name: "clear", level: 0 },
    CmdMeta { name: "me", level: 0 },
    CmdMeta { name: "w", level: 0 },
    CmdMeta { name: "whisper", level: 0 },
    CmdMeta { name: "tell", level: 0 },
    CmdMeta { name: "msg", level: 0 },
    CmdMeta { name: "wave", level: 0 },
    CmdMeta { name: "shrug", level: 0 },
    CmdMeta { name: "dance", level: 0 },
    CmdMeta { name: "pos", level: 0 },
    CmdMeta { name: "uptime", level: 0 },
    CmdMeta { name: "version", level: 0 },
    CmdMeta { name: "ver", level: 0 },
    CmdMeta { name: "info", level: 0 },
    CmdMeta { name: "stats", level: 0 },
    CmdMeta { name: "listblocks", level: 0 },
    CmdMeta { name: "blocks", level: 0 },
    CmdMeta { name: "listmobs", level: 0 },
    CmdMeta { name: "mobs", level: 0 },
    CmdMeta { name: "find", level: 0 },
    CmdMeta { name: "biome", level: 0 },
    CmdMeta { name: "findbiome", level: 0 },
    CmdMeta { name: "whois", level: 0 },
    CmdMeta { name: "recipe", level: 0 },
    CmdMeta { name: "grid", level: 0 },
    CmdMeta { name: "chunkinfo", level: 0 },
    CmdMeta { name: "serverinfo", level: 0 },
    // Level 1 - personal QoL.
    CmdMeta { name: "setname", level: 1 },
    CmdMeta { name: "fov", level: 1 },
    CmdMeta { name: "rd", level: 1 },
    CmdMeta { name: "renderdistance", level: 1 },
    // Level 2 - creative / movement / self-gameplay.
    CmdMeta { name: "tp", level: 2 },
    CmdMeta { name: "tppos", level: 2 },
    CmdMeta { name: "goto", level: 2 },
    CmdMeta { name: "home", level: 2 },
    CmdMeta { name: "sethome", level: 2 },
    CmdMeta { name: "spawn", level: 2 },
    CmdMeta { name: "spawnpoint", level: 2 },
    CmdMeta { name: "stuck", level: 2 },
    CmdMeta { name: "unstuck", level: 2 },
    CmdMeta { name: "jump", level: 2 },
    CmdMeta { name: "face", level: 2 },
    CmdMeta { name: "randomtp", level: 2 },
    CmdMeta { name: "rtp", level: 2 },
    CmdMeta { name: "fly", level: 2 },
    CmdMeta { name: "godmode", level: 2 },
    CmdMeta { name: "god", level: 2 },
    CmdMeta { name: "heal", level: 2 },
    CmdMeta { name: "give", level: 2 },
    CmdMeta { name: "restock", level: 2 },
    CmdMeta { name: "tool", level: 2 },
    CmdMeta { name: "blob", level: 2 },
    CmdMeta { name: "clearinventory", level: 2 },
    CmdMeta { name: "clearinv", level: 2 },
    CmdMeta { name: "resetinventory", level: 2 },
    CmdMeta { name: "resetinv", level: 2 },
    CmdMeta { name: "clearcraft", level: 2 },
    CmdMeta { name: "fw", level: 2 },
    CmdMeta { name: "firework", level: 2 },
    // Level 3 - world edits + host moderation.
    CmdMeta { name: "dim", level: 3 },
    CmdMeta { name: "kill", level: 3 },
    CmdMeta { name: "setblock", level: 3 },
    CmdMeta { name: "fill", level: 3 },
    CmdMeta { name: "explode", level: 3 },
    CmdMeta { name: "tree", level: 3 },
    CmdMeta { name: "summon", level: 3 },
    CmdMeta { name: "spawnmob", level: 3 },
    CmdMeta { name: "killmobs", level: 3 },
    CmdMeta { name: "horde", level: 3 },
    CmdMeta { name: "dmg", level: 3 },
    CmdMeta { name: "damage", level: 3 },
    CmdMeta { name: "weather", level: 3 },
    CmdMeta { name: "day", level: 3 },
    CmdMeta { name: "night", level: 3 },
    CmdMeta { name: "time", level: 3 },
    CmdMeta { name: "sleep", level: 3 },
    CmdMeta { name: "freezetime", level: 3 },
    CmdMeta { name: "freeze", level: 3 },
    CmdMeta { name: "mute", level: 3 },
    CmdMeta { name: "unmute", level: 3 },
    CmdMeta { name: "kick", level: 3 },
    CmdMeta { name: "announce", level: 3 },
    CmdMeta { name: "motd", level: 3 },
    CmdMeta { name: "lock", level: 3 },
    CmdMeta { name: "mobcap", level: 3 },
    CmdMeta { name: "giveall", level: 3 },
    CmdMeta { name: "healall", level: 3 },
    CmdMeta { name: "setspawn", level: 3 },
    // Level 4 - admin / irreversible.
    CmdMeta { name: "save", level: 4 },
    CmdMeta { name: "worldgen", level: 4 },
    CmdMeta { name: "gen", level: 4 },
    CmdMeta { name: "clearedits", level: 4 },
    CmdMeta { name: "wipe", level: 4 },
    CmdMeta { name: "seed", level: 4 },
];

/// Permission level required to run `name`. Returns `None`
/// when the command is unknown (dispatch should reject with
/// "unknown command", NOT a permission error).
pub(crate) fn cmd_level(name: &str) -> Option<PermLevel> {
    CMD_META.iter().find(|m| m.name == name).map(|m| m.level)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_outranks_everything() {
        let host = Sender::host();
        for cmd in CMD_META {
            assert!(
                host.level >= cmd.level,
                "host should be allowed /{}",
                cmd.name
            );
        }
    }

    #[test]
    fn default_player_can_run_basics() {
        let player = Sender::player("alice");
        assert!(player.level >= cmd_level("help").unwrap());
        assert!(player.level >= cmd_level("tp").unwrap());
        assert!(player.level >= cmd_level("give").unwrap());
    }

    #[test]
    fn default_player_cannot_admin() {
        let player = Sender::player("alice");
        assert!(player.level < cmd_level("save").unwrap());
        assert!(player.level < cmd_level("setblock").unwrap());
        assert!(player.level < cmd_level("worldgen").unwrap());
        assert!(player.level < cmd_level("kick").unwrap());
    }

    #[test]
    fn unknown_command_returns_none() {
        assert_eq!(cmd_level("xyzzy_no_such_command"), None);
    }

    #[test]
    fn no_command_table_duplicates() {
        for (i, a) in CMD_META.iter().enumerate() {
            for b in &CMD_META[i + 1..] {
                assert_ne!(a.name, b.name, "duplicate cmd {}", a.name);
            }
        }
    }

    #[test]
    fn biome_command_is_level_zero() {
        // `/biome` shows the current biome at the player's
        // feet - safe info, no side effects, every player gets it.
        assert_eq!(cmd_level("biome"), Some(0));
    }
}
