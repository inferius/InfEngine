//! W47.3 - render submodule.
//!
//! Hosts free render functions extracted out of the main.rs monolith.
//! Each submodule takes explicit `&mut Renderer` + the slices/maps it
//! needs as parameters so the App struct's borrow state stays simple.

pub mod mobs;
// W47.9 - sun + moon disc render + sun-intensity uniform.
pub mod celestial;
// W47.10 - emitter point-light upload.
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
// W140 - arrow projectile renderer (crossed quads + arrow.png).
pub mod projectiles;
// W153 - textured fire + nether portal meshes (frame-0 strip crops).
pub mod effect_meshes;
// W157 - first-person held-item viewmodel.
pub mod held_item;
// W232 - precomputed per-block draw params for the chunk draw queue.
pub mod draw_params;
// W240 - per-layer submesh merge (one draw per chunk per layer).
pub mod chunk_merge;
