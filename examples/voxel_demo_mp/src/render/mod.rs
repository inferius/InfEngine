//! W47.3 - render submodule.
//!
//! Hosts free render functions extracted out of the main.rs monolith.
//! Each submodule takes explicit `&mut Renderer` + the slices/maps it
//! needs as parameters so the App struct's borrow state stays simple.

pub mod mobs;
