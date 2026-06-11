//! W47.3 - render submodule.
//!
//! Hosts free render functions extracted out of the main.rs monolith.
//! Each submodule takes explicit `&mut Renderer` + the slices/maps it
//! needs as parameters so the App struct's borrow state stays simple.

pub mod mobs;
// W47.9 - sun + moon disc render + sun-intensity uniform.
pub mod celestial;
// W47.10 - emitter point-light upload.
pub mod point_lights;
// W47.11 - snow / portal / fire / liquid decoration draws.
pub mod decor;
// W47.15 - sky tint + weather/lightning mix + distance fog setup.
pub mod sky;
// W47.16 - floating dropped-item render (bob + magnet attract).
pub mod drops;
// W47.17 - remote + local player cube render (body + head, hashed tint).
pub mod players;
// W126 - chest block-entity renderer (entity sheets + lid anim).
pub mod chests;
// W134 - minecart entity renderer (MC MinecartModel + minecart.png).
pub mod minecarts;
