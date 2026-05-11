//! Per-frame input state.
//!
//! Tracks which keys are currently held. The engine updates the state in
//! response to winit keyboard events; application code reads it via
//! `FrameContext::input` once per frame inside `AppHandler::on_update`.

use std::collections::HashSet;

use crate::Key;

/// Snapshot of which keys are currently held.
///
/// "Currently held" means the last observed event for the key was a press
/// without a subsequent release. The engine resets nothing between frames;
/// the state mirrors the OS view of the keyboard until the window loses
/// focus, at which point all keys are cleared.
#[derive(Default, Debug, Clone)]
pub struct InputState {
    pressed: HashSet<Key>,
}

impl InputState {
    /// Returns whether `key` is currently held.
    pub fn is_pressed(&self, key: Key) -> bool {
        self.pressed.contains(&key)
    }

    /// Returns whether any of `keys` is currently held.
    pub fn any_pressed(&self, keys: &[Key]) -> bool {
        keys.iter().any(|k| self.pressed.contains(k))
    }

    pub(crate) fn set(&mut self, key: Key, pressed: bool) {
        if pressed {
            self.pressed.insert(key);
        } else {
            self.pressed.remove(&key);
        }
    }

    pub(crate) fn clear(&mut self) {
        self.pressed.clear();
    }
}
