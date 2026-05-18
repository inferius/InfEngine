//! Headless-safe slice of the multi-world save layer. See
//! `lib.rs` for the rationale - this file is the lib's wrapper
//! around the `world_save/` source tree so the `#[path]`
//! attributes inside resolve next to the actual `.rs` files
//! instead of doubling the `world_save/` segment.

#[path = "world_save/atomic.rs"]
pub mod atomic;
#[path = "world_save/layout.rs"]
pub mod layout;
#[path = "world_save/migrate/mod.rs"]
pub mod migrate;
#[path = "world_save/meta.rs"]
pub mod meta;
#[path = "world_save/world_json.rs"]
pub mod world_json;
#[path = "world_save/chunks.rs"]
pub mod chunks;
#[path = "world_save/players.rs"]
pub mod players;
#[path = "world_save/entities.rs"]
pub mod entities;
#[path = "world_save/timers/mod.rs"]
pub mod timers;
