//! Window creation and input handling for InfEngine.
//!
//! Wraps `winit` 0.30+ behind small `AppHandler` and `EngineWindow` types
//! so engine and example code do not depend on winit directly. Phase 1.1
//! covers window creation and the event-loop skeleton; rendering hookup
//! and richer input dispatch land in later Phase 1 steps.

#![forbid(unsafe_code)]

use std::sync::Arc;

use tracing::{debug, info};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowAttributes, WindowId};

mod key;
pub use key::Key;

/// Configuration used to create the main window.
#[derive(Debug, Clone)]
pub struct WindowConfig {
    pub title: String,
    pub width: u32,
    pub height: u32,
    pub resizable: bool,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            title: "InfEngine".to_owned(),
            width: 1280,
            height: 720,
            resizable: true,
        }
    }
}

/// Opaque handle to the engine's main window.
///
/// External code interacts with the window through this wrapper; the
/// underlying winit `Window` is reachable via `raw()` for crates inside
/// the workspace that need it (currently only the renderer).
#[derive(Clone)]
pub struct EngineWindow {
    inner: Arc<Window>,
}

impl EngineWindow {
    /// Current window inner size, in physical pixels.
    pub fn size(&self) -> (u32, u32) {
        let size = self.inner.inner_size();
        (size.width, size.height)
    }

    /// Scale factor (DPI ratio) of the monitor the window is on.
    pub fn scale_factor(&self) -> f64 {
        self.inner.scale_factor()
    }

    /// Ask the windowing system to redraw the window.
    pub fn request_redraw(&self) {
        self.inner.request_redraw();
    }

    /// Borrow the underlying winit window.
    ///
    /// Reserved for the renderer crate, which needs the raw handle to
    /// create a wgpu surface. Not part of the long-term public API.
    pub fn raw(&self) -> &Arc<Window> {
        &self.inner
    }
}

impl std::fmt::Debug for EngineWindow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (w, h) = self.size();
        f.debug_struct("EngineWindow")
            .field("width", &w)
            .field("height", &h)
            .finish()
    }
}

/// Trait implemented by application code that wants to receive window and
/// input events from the engine loop.
pub trait AppHandler {
    /// Called once after the window has been created.
    fn on_start(&mut self, _window: &EngineWindow) {}

    /// Called every time the event loop is about to redraw a frame.
    fn on_update(&mut self, _window: &EngineWindow) {}

    /// Called when a key transitions between pressed and released.
    fn on_key(&mut self, _key: Key, _pressed: bool) {}

    /// Called when the surface is resized (in physical pixels).
    fn on_resize(&mut self, _width: u32, _height: u32) {}

    /// Called once just before the window is destroyed.
    fn on_exit(&mut self) {}
}

/// Errors that can occur while running the engine event loop.
#[derive(Debug, thiserror::Error)]
pub enum WindowError {
    #[error("failed to create event loop: {0}")]
    EventLoop(#[from] winit::error::EventLoopError),
    #[error("failed to create window: {0}")]
    Os(#[from] winit::error::OsError),
}

/// Start the engine event loop with the provided handler.
///
/// Blocks the calling thread until the window is closed. Must be invoked
/// from `main` on the main thread.
pub fn run<A: AppHandler + 'static>(config: WindowConfig, app: A) -> Result<(), WindowError> {
    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Poll);

    let mut state = EngineApp::<A> {
        config,
        window: None,
        handler: Some(app),
    };

    event_loop.run_app(&mut state)?;
    Ok(())
}

struct EngineApp<A: AppHandler> {
    config: WindowConfig,
    window: Option<EngineWindow>,
    handler: Option<A>,
}

impl<A: AppHandler> ApplicationHandler for EngineApp<A> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        let attrs = WindowAttributes::default()
            .with_title(&self.config.title)
            .with_inner_size(LogicalSize::new(
                self.config.width as f64,
                self.config.height as f64,
            ))
            .with_resizable(self.config.resizable);

        let window = event_loop
            .create_window(attrs)
            .expect("failed to create main window");
        let window = EngineWindow {
            inner: Arc::new(window),
        };

        info!(
            title = %self.config.title,
            width = self.config.width,
            height = self.config.height,
            "window created"
        );

        if let Some(handler) = self.handler.as_mut() {
            handler.on_start(&window);
        }
        self.window = Some(window);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(handler) = self.handler.as_mut() else {
            return;
        };
        let Some(window) = self.window.as_ref() else {
            return;
        };

        match event {
            WindowEvent::CloseRequested => {
                debug!("close requested");
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                handler.on_resize(size.width, size.height);
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(code) = event.physical_key {
                    let pressed = event.state.is_pressed();
                    if code == KeyCode::Escape && pressed {
                        info!("escape pressed -> exit");
                        event_loop.exit();
                        return;
                    }
                    if let Some(key) = Key::from_winit(code) {
                        handler.on_key(key, pressed);
                    }
                }
            }
            WindowEvent::RedrawRequested => {
                handler.on_update(window);
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(handler) = self.handler.as_mut() {
            handler.on_exit();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_window_config_has_reasonable_values() {
        let c = WindowConfig::default();
        assert_eq!(c.width, 1280);
        assert_eq!(c.height, 720);
        assert!(c.resizable);
        assert!(!c.title.is_empty());
    }
}
