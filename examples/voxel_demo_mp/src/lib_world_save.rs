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
// W479: the v6 -> v7 player arrow + frozen v6 shape (split out of
// players.rs for the file-size budget; `players.rs` + `migrate`
// reference it as a sibling).
#[path = "world_save/players_v7.rs"]
pub mod players_v7;
#[path = "world_save/entities.rs"]
pub mod entities;
// W479: the v8 -> v9 entities arrow + frozen v8 shapes (same split).
#[path = "world_save/entities_v9.rs"]
pub mod entities_v9;
#[path = "world_save/containers.rs"]
pub mod containers;
#[path = "world_save/timers/mod.rs"]
pub mod timers;
