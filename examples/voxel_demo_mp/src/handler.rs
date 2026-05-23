//! W47.27 - impl AppHandler for Voxel (relocated from main.rs).
//!
//! Most AppHandler methods are already one-line wrappers that
//! delegate into per-domain helpers (bootstrap::on_start, app_on_update::on_update,
//! ui::render::render). The remaining body (on_window_event,
//! on_device_event, on_resize) lives here.

#![allow(unused_imports)]

use crate::*;

impl AppHandler for Voxel {
    fn on_start(&mut self, window: &EngineWindow) {
        crate::bootstrap::on_start(self, window);
    }



    fn on_window_event(&mut self, window: &EngineWindow, event: &WindowEvent) {
        if let Some(ui) = self.ui.as_mut() {
            ui.handle_engine_event(window, event);
        }
        // Window close: send a `vx.leave` first so the server can
        // reap our pose instantly + broadcast the removal to the
        // other peers. The actual process exit happens on the next
        // `on_update` via `requested_quit`.
        if matches!(event, WindowEvent::CloseRequested) {
            // Final save before exit so the player doesn't lose
            // edits made since the last 30 s autosave.
            self.save_server_state();
            self.notify_server_leaving();
            self.requested_quit = true;
        }
        // Focus tracking. Clicking another window on the same machine
        // shifts focus; without this guard `apply_cursor_settings`
        // would keep wrestling the cursor away from whichever window
        // the user actually clicked.
        if let WindowEvent::Focused(focused) = event {
            self.has_focus = *focused;
            if !*focused {
                self.lmb_pending = false;
                self.rmb_pending = false;
                self.mouse_dx = 0.0;
                self.mouse_dy = 0.0;
                self.last_mouse = None;
            }
        }
        // Track cursor moved deltas only when no grab is active (i.e. in
        // menus this is the only source; in Play raw DeviceEvent
        // supplies the deltas).
        if let WindowEvent::CursorMoved { position, .. } = event
            && matches!(self.state, AppState::Playing)
            && !(self.settings.lock_cursor || self.settings.hide_cursor)
        {
            let now = [position.x as f32, position.y as f32];
            if let Some(prev) = self.last_mouse {
                self.mouse_dx += now[0] - prev[0];
                self.mouse_dy += now[1] - prev[1];
            }
            self.last_mouse = Some(now);
        }
        // Edge-triggered LMB / RMB while in Play state. Both flags are
        // consumed in `on_update` next frame. Ignored when the window
        // doesn't have focus so a click on a sibling instance never
        // mines a block here.
        if matches!(self.state, AppState::Playing)
            && self.has_focus
            && let WindowEvent::MouseInput { state, button, .. } = event
        {
            match (state, button) {
                (ElementState::Pressed, MouseButton::Left) => {
                    self.lmb_pending = true;
                    // W30 fix: only set just_pressed on the rising edge
                    // (transition from up to down). winit may fire multiple
                    // Pressed events on a single click; this guard makes
                    // creative insta-break fire exactly once per physical
                    // click regardless.
                    if !self.lmb_held {
                        self.lmb_just_pressed = true;
                    }
                    self.lmb_held = true;
                }
                (ElementState::Released, MouseButton::Left) => {
                    self.lmb_held = false;
                    // Releasing the button cancels any active
                    // mining session.
                    self.mining = None;
                }
                (ElementState::Pressed, MouseButton::Right) => {
                    self.rmb_pending = true;
                    self.rmb_held = true;
                }
                (ElementState::Released, MouseButton::Right) => {
                    self.rmb_held = false;
                }
                _ => {}
            }
        }
        // Mouse wheel cycles through hotbar slots.
        if matches!(self.state, AppState::Playing)
            && let WindowEvent::MouseWheel { delta, .. } = event
        {
            use infengine_window::raw_winit::event::MouseScrollDelta;
            let y = match delta {
                MouseScrollDelta::LineDelta(_, y) => *y,
                MouseScrollDelta::PixelDelta(p) => (p.y as f32) / 50.0,
            };
            self.wheel_accum += y;
        }
    }

    fn on_device_event(&mut self, event: &infengine_window::raw_winit::event::DeviceEvent) {
        use infengine_window::raw_winit::event::DeviceEvent;
        if !matches!(self.state, AppState::Playing) {
            return;
        }
        // Device events fire globally - any other instance with cursor
        // lock would otherwise rotate its camera off our motion. Gate
        // on focus to keep input owner-local.
        if !self.has_focus {
            return;
        }
        if !(self.settings.lock_cursor || self.settings.hide_cursor) {
            return;
        }
        if let DeviceEvent::MouseMotion { delta } = event {
            self.mouse_dx += delta.0 as f32;
            self.mouse_dy += delta.1 as f32;
        }
    }

    fn on_update(&mut self, window: &EngineWindow, ctx: &FrameContext<'_>) {
        crate::app_on_update::on_update(self, window, ctx);
    }

    fn on_resize(&mut self, w: u32, h: u32) {
        self.viewport = (w, h);
        if let Some(r) = self.renderer.as_mut() {
            r.resize(w, h);
        }
    }
}
