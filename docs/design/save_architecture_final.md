# Save architecture - final design

Status: **DRAFT - blocker for Phase B**. Locks the on-disk
format + the server -> client sync protocol so Phase B (server
CLI), Phase C (chests / furnaces), Phase F (dimensions) and the
multi-world main-menu UI all build against a stable foundation.

Replaces the earlier `world_save_load.md` design (which assumed
the demo binary owned saves directly).

## Source of truth

**The server owns the world.** All persistence happens
server-side. Clients are stateless w.r.t. world data - they
receive snapshots on join, apply broadcasts during play,
discard everything on disconnect.

This applies in all three runtime modes:

| Mode             | Server role                                  | Client role |
|------------------|----------------------------------------------|-------------|
| Offline single   | Same process is server + client. Server saves. | Stateless renderer. |
| Hosted MP        | Host is server. Host saves.                    | Stateless. |
| Dedicated server | Headless `infengine_server` bin. Server saves. | Stateless. |

In all three, the server holds the canonical world + does the
disk I/O. The client never writes saves.

## On-disk layout

```
%APPDATA%/InfEngine/worlds/<slug>/        (Windows)
~/.local/share/infengine/worlds/<slug>/   (Linux/macOS)

  meta.json                       # save metadata, human-readable
  world.json                      # worldgen variant + seed + format ver
  chunks/
    overworld/                    # one folder per dimension
      cx_cy_cz.chunk.bin          # bincode + zstd-compressed Chunk
    nether/
      cx_cy_cz.chunk.bin
    ...
  entities/
    overworld.bin                 # mobs + dropped items + future entities
  players/
    <player_name>.bin             # per-player state (inv, pos, air, hp)
  timers/
    scheduled.bin                 # furnace cook ticks, crop growth, ...
  history/
    edits.log                     # rolling tail of last N edits (debug + recovery)
```

`<slug>` = human-typed name canonicalised to
`[a-z0-9-]` + creation timestamp suffix so duplicate names don't
collide. Pure ASCII + lowercase.

### meta.json (schema v1)

```json
{
  "name": "First world",
  "slug": "first-world-2026-05-19",
  "format_version": 1,
  "created_at": "2026-05-19T10:21:33Z",
  "last_played": "2026-05-19T17:42:01Z",
  "play_time_seconds": 3825,
  "host_machine_id": "...",
  "engine_version": "0.0.1",
  "thumbnail_path": "thumbnail.png"
}
```

`thumbnail.png` = optional 320x180 screenshot from the most
recent quit (Phase G adds the capture step).

### world.json (schema v1)

```json
{
  "format_version": 1,
  "worldgen": "amplified",
  "seed": 17539214,
  "spawn": [12, 80, -8],
  "world_border_radius": 30000000,
  "default_dimension": "overworld",
  "dimensions": ["overworld", "nether"]
}
```

Pinning `worldgen` + `seed` per save means the same chunk
coords regenerate identically when the worldgen RNG is
deterministic - lets us only persist DIRTY chunks.

### chunks/<dim>/cx_cy_cz.chunk.bin

```
[u8;  4]   magic = b"INFC"
[u32]      format_version
[u8]       compression_kind (0 = none, 1 = zstd)
[u64]      uncompressed_payload_len
[bytes]    payload (bincode-serialised ChunkPayload)
```

`ChunkPayload`:

```rust
struct ChunkPayload {
    coord: [i32; 3],
    voxels: ChunkVoxels,           // RLE-compressed
    liquid_levels: Vec<([u8; 3], u8)>,  // local-coord + level
    torch_orient: Vec<([u8; 3], u8)>,
    snow_layers: Vec<([u8; 3], u8)>,
    // future: container contents, sign text, mob entities by chunk
}
```

`ChunkVoxels` is run-length encoded over the 4096 voxel array -
a 16^3 chunk uniformly filled with air compresses to ~10 bytes;
edited chunks compress 80-90% on top via zstd.

Only chunks whose `dirty` flag is set since worldgen get
persisted. On load, missing files = regenerate from
`worldgen + seed`. Save size scales with edit volume, not world
size.

### entities/<dim>.bin

All mob + dropped-item entities for a dimension in one file.
Cross-chunk roaming means per-chunk entity bins would constantly
need rebalancing; one file per dim is simpler. Compressed
bincode.

```rust
struct DimensionEntities {
    mobs: Vec<MobSave>,
    drops: Vec<DropSave>,
    next_mob_id: u32,
    next_drop_id: u32,
}
```

### players/<name>.bin

Per-player state. Survives across sessions for the same player
name. On dedicated server, multiple players coexist - one file
each.

```rust
struct PlayerSave {
    format_version: u32,
    pos: [f32; 3],
    yaw: f32,
    pitch: f32,
    inventory: [Option<(BlockId, u8)>; INVENTORY_SLOTS],
    craft_grid: [Option<(BlockId, u8)>; 9],   // future Phase D
    selected_slot: u8,
    air: f32,
    health: f32,
    home_pos: Option<[f32; 3]>,
    spawn_pos: Option<[f32; 3]>,
    permission_level: u8,   // 0..=4 from chat::perms
    play_time_seconds: u64,
    last_dimension: String,
}
```

### timers/scheduled.bin

```rust
struct ScheduledTimers {
    format_version: u32,
    items: Vec<Timer>,
}

enum Timer {
    Furnace { pos: [i32; 3], dim: String, fuel_left_ticks: u32, cook_progress_ticks: u32 },
    Crop { pos: [i32; 3], dim: String, stage: u8, due_at_wall_ms: u64 },
    LiquidScheduled { pos: [i32; 3], dim: String, due_at_wall_ms: u64 },
    // future kinds
}
```

Time-warp catch-up uses `due_at_wall_ms` against the current
wall clock on load (with a `MAX_OFFLINE_SECS` cap so a year-old
save doesn't simulate for hours).

## Save / load lifecycle

### Server startup (any mode)

1. Resolve world directory from args (`--world <slug>`) or
   menu pick.
2. Read `meta.json` + `world.json`. Reject on
   `format_version` mismatch unless migration is registered.
3. Set worldgen + seed from `world.json`.
4. Load `timers/scheduled.bin`. Compute
   `elapsed = now - meta.last_played`. Catch-up loop runs every
   timer whose `due_at` is in the past. Output items collect.
5. Load `entities/<dim>.bin` for each dimension.
6. Player files load lazily on connect.
7. Chunks load on demand from the streaming layer (active
   radius around any player).

### Client join (offline = local-loopback, MP = network)

1. Client sends `vx.join` with player name.
2. Server responds with `vx.world_meta` (seed, worldgen,
   spawn, current time, day_time, weather).
3. Server reads `players/<name>.bin` if exists, otherwise
   creates fresh. Server sends `vx.player_state` to client.
4. Server begins streaming chunks within view radius around
   the player's saved pos. Each chunk = one `vx.chunk_snapshot`
   message carrying the same `ChunkPayload` the disk format
   uses (so the wire format and disk format share a struct).
5. Client renders as chunks arrive.

### Server autosave loop

- Every 60s wall clock: iterate dirty chunks + dirty
  players + dirty timers. Write each touched file.
- After write: clear dirty flags.
- Background thread for the disk I/O; main tick keeps running.
- Write-to-temp + atomic rename for crash safety. Keep a
  rolling 1-deep backup (`<file>.bak`) so half-written saves
  don't brick the slot.

### Client / shutdown

- Server flushes everything dirty + writes `meta.json` with
  bumped `last_played` + `play_time_seconds`.
- Server takes thumbnail (Phase G).
- Server exits.

## Wire format alignment

The on-disk `ChunkPayload`, `PlayerSave`, `MobSave`, `DropSave`,
`Timer` structs ARE the wire-format types. One struct, two
codecs:

- Disk: `bincode + zstd` for compression.
- Wire: `bincode` raw (TCP already chunks; per-packet gzip if
  message > 1 KB).

Reusing the structs across both removes a translation layer
(no `ServerStateV2` -> wire conversion). Adding a field = one
serde change in one place.

Versioning: every struct carries `format_version: u32` at the
top. Server checks on deserialize; refuses unknown versions
unless a migration is registered.

## Multi-world UI flow (Phase G)

Main menu reads the worlds directory:

```
fn list_worlds() -> Vec<WorldSummary>
```

Each entry: name + slug + worldgen + last_played + thumbnail.
Sorted by `last_played` desc.

Buttons:

- **New World**: dialog for name + worldgen + seed. Creates
  the directory + writes initial `meta.json` + `world.json`.
- **Load World**: pick from list. Spawns the offline singleplayer
  server pointing at that slot.
- **Delete World**: confirm + remove the directory. Backed-up
  to `~/.local/share/infengine/worlds_trash/<slug>-<ts>/` for
  30 days (configurable).
- **Connect to Server**: existing direct-connect dialog. Saves
  go on the server, never local.

## Time-warp catch-up

On server startup, after loading `timers/scheduled.bin`:

```rust
let elapsed_ms = (wall_now - meta.last_played_ms).min(MAX_OFFLINE_MS);
for timer in timers.items {
    if timer.due_at_wall_ms <= wall_now {
        let result = match timer.kind {
            Furnace { ... } => furnace::catch_up(..., elapsed_ms),
            Crop { ... } => crop::catch_up(..., elapsed_ms),
            // ...
        };
        apply_result(result);
    }
}
```

`MAX_OFFLINE_MS = 7 days` (configurable). Beyond that we cap
the simulation so a year-old save doesn't spend minutes
catching up.

Each `*::catch_up` is a pure function: given timer state +
elapsed time, return the new state + emitted events (output
items into the furnace output slot, harvest the wheat into the
crop's drop pile, etc).

## Format versioning + migration

Top-level `format_version: u32` in each of:
- `meta.json`
- `world.json`
- `ChunkPayload`
- `PlayerSave`
- `DimensionEntities`
- `ScheduledTimers`

On load mismatch:
1. Look up registered migration `from_v -> to_v`.
2. If none, refuse load + write `meta.json.error` with details.
3. If found, run migration before deserialise, write back at
   new version, continue.

Migrations live under `voxel_demo_mp/src/world_save/migrate/`,
one file per `from_v -> to_v` arrow. Tests cover each
migration's input/output.

## Atomicity + corruption recovery

For every save file `path`:

```rust
fn atomic_save(path: &Path, payload: &[u8]) {
    let tmp = path.with_extension("tmp");
    let bak = path.with_extension("bak");
    fs::write(&tmp, payload)?;
    if path.exists() {
        fs::rename(path, &bak)?;
    }
    fs::rename(&tmp, path)?;
    // bak stays as 1-deep rolling backup; future write overwrites it.
}
```

On load:
1. Try `path`. If success, done.
2. If parse error, try `<path>.bak`. Log corruption.
3. If both fail, mark dimension/player as broken; refuse load
   gracefully.

## Folder layout in code

Per `file_size_policy.md`. New module under
`examples/voxel_demo_mp/src/world_save/` (existing folder
gets expanded):

```
world_save/
  mod.rs            # entry points: save_world, load_world, list_worlds
  layout.rs         # paths, slugs, world directory resolution
  meta.rs           # meta.json read/write
  world_json.rs     # world.json read/write
  chunks.rs         # ChunkPayload encode/decode + zstd
  entities.rs       # DimensionEntities save/load
  players.rs        # PlayerSave save/load
  timers.rs         # ScheduledTimers + catch-up dispatch
  history.rs        # rolling edit log
  atomic.rs         # atomic_save helper + crash recovery
  migrate/
    mod.rs          # registry + dispatch
    chunk_v1_to_v2.rs  # one file per migration arrow (future)
  tests.rs          # round-trip + format-version edge cases
```

Each file under the 500-LOC soft cap; `chunks.rs` will be the
heaviest at ~300 LOC.

## What this design unblocks

- **Phase B** (dedicated server CLI): server boots from
  `--world <slug>`, owns saves, can run headless.
- **Phase C** (chests / furnaces): chest contents persist via
  `ChunkPayload.container_contents` field; furnaces use the
  `Timer::Furnace` variant.
- **Phase F** (dimensions): adding a dim = adding a folder
  under `chunks/<dim>/` + an entry in `world.json.dimensions`.
- **Phase G** (multi-world UI): reads the worlds directory,
  shows the list, handles New / Load / Delete.

## Phasing the implementation

Inside Phase B (server CLI), the save layer lands in this
order:

1. `layout.rs` + `meta.rs` + `world_json.rs` (path resolution +
   metadata) - **2 days**.
2. `chunks.rs` (chunk encode/decode + zstd) - **2 days**.
3. `players.rs` (per-player save + load) - **1 day**.
4. `atomic.rs` + crash recovery wiring - **1 day**.
5. `entities.rs` + `timers.rs` (entities + scheduled timers) -
   **2 days**.
6. Migration framework - **1 day**.
7. Multi-world UI in main menu - **2 days**.

Total ~11 days for the full save layer alongside server CLI.

## Open questions

- Compression library: `zstd` vs `lz4`. `zstd` smaller files,
  `lz4` faster decode. Recommendation: zstd at level 3 (default).
- Chunk file granularity: per-Y-stack (16x256x16) vs per-cubic-
  chunk (16^3). Current code uses cubic; sticking with it.
- Player file rotation: do we keep history of past player
  states? Recommendation: no, just `players/<name>.bin` + a
  `.bak`. Players who want history can use Git.
- Cross-platform save portability: Linux + Windows + macOS
  must read each other's saves. Pin little-endian bincode +
  use ISO 8601 UTC timestamps in JSON. Done implicitly.
- Save while another save is in progress: lock file
  `<slug>/.lock` containing host process pid. Refuse load
  if lock exists + pid is alive.
