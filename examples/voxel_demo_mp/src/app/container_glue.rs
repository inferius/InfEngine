//! W47.28 - container_glue impl block (relocated from app/mod.rs).
#![allow(unused_imports)]
use crate::*;
use crate::lighting::HaloLighting;

impl Voxel {
    /// W5.3a: handle a batch of `vx.chest_open` requests. Each
    /// open registers the sender as the chest's viewer (single-
    /// viewer for Wave 5: a new opener displaces the previous)
    /// and ships an immediate `ChestStateMsg` so the client paints
    /// the host's contents instead of an empty grid.
    pub(crate) fn apply_chest_open_msgs(&mut self, msgs: &[(u32, crate::net::ChestOpenMsg)]) { crate::net::container_apply::apply_chest_open_msgs(self, msgs); }
    pub(crate) fn apply_chest_act_msgs(&mut self, msgs: &[(u32, crate::net::ChestActMsg)]) { crate::net::container_apply::apply_chest_act_msgs(self, msgs); }
    pub(crate) fn apply_chest_close_msgs(&mut self, msgs: &[(u32, crate::net::ChestCloseMsg)]) { crate::net::container_apply::apply_chest_close_msgs(self, msgs); }
    pub(crate) fn apply_furnace_open_msgs(&mut self, msgs: &[(u32, crate::net::FurnaceOpenMsg)]) { crate::net::container_apply::apply_furnace_open_msgs(self, msgs); }
    pub(crate) fn apply_furnace_act_msgs(&mut self, msgs: &[(u32, crate::net::FurnaceActMsg)]) { crate::net::container_apply::apply_furnace_act_msgs(self, msgs); }
    pub(crate) fn apply_furnace_close_msgs(&mut self, msgs: &[(u32, crate::net::FurnaceCloseMsg)]) { crate::net::container_apply::apply_furnace_close_msgs(self, msgs); }
    // W470: drag-paint + inventory-op appliers.
    pub(crate) fn apply_chest_drag_msgs(&mut self, msgs: &[(u32, crate::net::ContainerDragMsg)]) { crate::net::container_apply::apply_chest_drag_msgs(self, msgs); }
    pub(crate) fn apply_furnace_drag_msgs(&mut self, msgs: &[(u32, crate::net::ContainerDragMsg)]) { crate::net::container_apply::apply_furnace_drag_msgs(self, msgs); }
    pub(crate) fn apply_inv_op_msgs(&mut self, msgs: &[(u32, crate::net::InvOpMsg)]) { crate::net::container_apply::apply_inv_op_msgs(self, msgs); }
    pub(crate) fn apply_inv_drag_msgs(&mut self, msgs: &[(u32, crate::net::InvDragMsg)]) { crate::net::container_apply::apply_inv_drag_msgs(self, msgs); }
    pub(crate) fn broadcast_chest_state(&mut self, pos: [i32; 3]) { crate::net::container_apply::broadcast_chest_state(self, pos); }
    pub(crate) fn broadcast_furnace_state(&mut self, pos: [i32; 3]) { crate::net::container_apply::broadcast_furnace_state(self, pos); }

    /// W5.3b: 5 Hz throttle decision for furnace state broadcasts.
    /// Returns `true` when the previous broadcast was > 200ms ago
    /// (or never) and stamps `furnace_state_last_sent_ms[pos]` to
    /// the supplied `now_ms`.
    pub(crate) fn should_send_furnace_state(&mut self, pos: [i32; 3], now_ms: u64) -> bool {
        let last = self.furnace_state_last_sent_ms.get(&pos).copied();
        match last {
            Some(t) if now_ms.saturating_sub(t) < 200 => false,
            _ => {
                self.furnace_state_last_sent_ms.insert(pos, now_ms);
                true
            }
        }
    }
}
