//! Sample Rust plugin for the InfEngine plugin system.
//!
//! Demonstrates the typical Plugin trait shape:
//! - Registers a "rainbow_brick" block on init.
//! - Listens for chat lines starting with `!rainbow `; on a
//!   match, places a rainbow_brick at the speaker's chat
//!   position (the host fills in (0,0,0) for now since the
//!   chat-event payload doesn't yet carry a pose; future
//!   wave can extend ChatEvent).
//! - Logs the active dimension every ~10 seconds in on_tick.
//!
//! Build as a cdylib and drop the resulting `sample_plugin_rs.{dll,so,dylib}`
//! into `<engine_exec_dir>/plugins/` to have it picked up at
//! startup.

use infengine_sdk_rs::{Engine, Plugin, declare_plugin};

/// Sample plugin state. `Default` is required by the
/// `declare_plugin!` macro - the macro default-constructs a
/// static instance on first registration.
#[derive(Default)]
struct RainbowPlugin {
    /// Block id the engine assigned us on register. `None`
    /// until `on_init` completes.
    brick_id: Option<u16>,
    /// Seconds since the last "current dim" log line. The
    /// plugin throttles to ~10 s intervals so it doesn't spam.
    tick_accum: f32,
}

impl Plugin for RainbowPlugin {
    fn name() -> &'static str {
        "rainbow_plugin"
    }

    fn version() -> &'static str {
        "0.1.0"
    }

    fn on_init(&mut self, engine: &mut Engine) {
        // Register the block; color is a warm orange.
        let id = engine.block_register("rainbow_brick", [1.0, 0.5, 0.0]).ok();
        self.brick_id = id;
        if id.is_some() {
            engine.log_info("rainbow_plugin loaded");
        } else {
            engine
                .log_warn("rainbow_plugin: block_register failed; chat command disabled");
        }
    }

    fn on_shutdown(&mut self, engine: &mut Engine) {
        engine.log_info("rainbow_plugin unloaded");
    }

    fn on_tick(
        &mut self,
        engine: &mut Engine,
        dt: f32,
        _day_time: f32,
        current_dim: &str,
    ) {
        self.tick_accum += dt;
        if self.tick_accum >= 10.0 {
            self.tick_accum = 0.0;
            let msg = format!("rainbow_plugin: active dim = {current_dim}");
            engine.log_info(&msg);
        }
    }

    fn on_chat_msg(
        &mut self,
        engine: &mut Engine,
        _speaker: u32,
        msg: &str,
    ) {
        // Look for `!rainbow <x> <y> <z>` and place a brick
        // there. The chat event doesn't currently carry the
        // speaker's pose, so we accept coords inline. Robust
        // parsing keeps a single space between args; bad input
        // just no-ops with a friendly log line.
        let Some(rest) = msg.strip_prefix("!rainbow ") else {
            return;
        };
        let Some(brick_id) = self.brick_id else {
            engine.log_warn("rainbow_plugin: brick_id unavailable");
            return;
        };
        let parts: Vec<&str> = rest.split_whitespace().collect();
        if parts.len() != 3 {
            let _ = engine.chat_send(
                "[rainbow] usage: !rainbow <x> <y> <z>",
            );
            return;
        }
        let coords = (
            parts[0].parse::<i32>(),
            parts[1].parse::<i32>(),
            parts[2].parse::<i32>(),
        );
        let (Ok(x), Ok(y), Ok(z)) = coords else {
            let _ = engine.chat_send(
                "[rainbow] bad coords; need integers",
            );
            return;
        };
        engine.world_set(x, y, z, brick_id);
        let _ = engine.chat_send(&format!(
            "[rainbow] placed rainbow_brick at ({x}, {y}, {z})"
        ));
    }
}

declare_plugin!(RainbowPlugin);

// Unit tests inside a cdylib aren't easy because the test
// harness expects an executable, not a dynamic library. We
// exercise the plugin's logic from the engine side via
// integration tests in voxel_demo_mp.

#[cfg(test)]
mod tests {
    use super::*;

    /// `Plugin::name` + `version` return the constants we
    /// advertise to the host.
    #[test]
    fn plugin_name_version() {
        assert_eq!(RainbowPlugin::name(), "rainbow_plugin");
        assert_eq!(RainbowPlugin::version(), "0.1.0");
    }

    /// Default-constructed state matches our expectations.
    #[test]
    fn plugin_default_state() {
        let p = RainbowPlugin::default();
        assert!(p.brick_id.is_none());
        assert_eq!(p.tick_accum, 0.0);
    }
}
