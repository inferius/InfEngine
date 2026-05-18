# World save / load + time-warp catch-up

Status: **DRAFT — not implemented**. Filed at user request after
two related complaints:

- Builds disappear when the player walks away from a chunk and
  returns - chunks regenerate from worldgen instead of restoring
  their edited state.
- Time-bound systems (furnaces cooking, crops growing, brewing
  stands) should keep running while the player is offline /
  out-of-range. When they return, the simulation must catch up
  to wall-clock time instead of resuming where it left off.

The user also asked for an MC-style new-world flow: pick "New
World" from the main menu, name it, play, exit, return to the
main menu, pick the named save to resume.

## Surface goals

- Multiple named saves coexist. Main-menu lists them.
- "New World" creates a fresh slot (worldgen + name + seed +
  creation date).
- "Load World" resumes an existing slot.
- "Delete World" removes a slot (with confirmation).
- Edits persist across sessions, mob positions persist, player
  position + inventory + air + health persist.
- Time-bound systems (furnaces, crops, brewing) catch up after
  load by computing elapsed wall-clock seconds and stepping the
  relevant sims that many ticks.

## On-disk layout

```
~/.infengine/worlds/<slug>/
  meta.json            # human-readable save header
  player.bin           # bincode: player state
  world/
    chunks/
      cx_cy_cz.bin     # bincode: edited chunks only
    timers.bin         # bincode: scheduled time-bound events
  history/
    edits.log          # append-only EditMsg log (debug + recovery)
```

- `<slug>` derived from user-typed name + creation timestamp so
  identical names don't collide.
- `meta.json` is hand-readable so the user can edit save names
  / inspect what's there without us writing a viewer:

  ```json
  {
    "name": "First world",
    "slug": "first-world-2026-05-18",
    "worldgen": "classic",
    "seed": 17539214,
    "created_at": "2026-05-18T10:21:33Z",
    "last_played": "2026-05-18T17:42:01Z",
    "play_time_seconds": 3825,
    "format_version": 1
  }
  ```

- Only edited chunks are persisted. Pristine chunks are
  re-generated from `worldgen` + `seed` on load - keeps the save
  size small and supports worldgen version upgrades that change
  un-edited terrain without invalidating the player's builds.
- Chunk file marker: bincode header `{magic: "INFV", version: 1,
  chunk_coord: [i32;3], dirty: u32}` + the chunk's voxel grid
  (compressed RLE).

## Save / load lifecycle

### Save triggers

- Periodic auto-save: every 60 seconds during play.
- On quit-to-menu: full save.
- On `/save` chat command (debug).

The auto-save is incremental - only dirty chunks (set in
`world_set_tracked`'s `dirty_chunks: HashSet<ChunkCoord>`) are
serialised. Player + timers always save. After a successful
write, the dirty set clears.

### Load flow

1. Main menu lists every directory under `~/.infengine/worlds/`
   that has a valid `meta.json`. Sort by `last_played` desc.
2. Click a save → spawn screen with the slot's `worldgen` + seed
   + player position.
3. Streaming layer asks `world::ChunkSource` for each chunk. The
   source tries `slug/world/chunks/cx_cy_cz.bin` first; on miss,
   falls back to deterministic worldgen with the saved seed.
4. Player state restored from `player.bin` (position, inventory,
   air, health, day_time).
5. Time catch-up: load `timers.bin`, compute `elapsed = now() -
   meta.last_played`, run catch-up step (see below).

### Time catch-up

Every time-bound entry is a `(ScheduledKind, BlockPos, due_at)`
tuple. On load:

```
elapsed = now - meta.last_played
for each timer where timer.due_at <= now:
    fire that timer (smelt the iron, harvest the wheat, ...)
    if it produces a follow-up timer (e.g. fuel still burning),
        schedule the follow-up
```

For furnaces specifically: a furnace stores (`fuel_left_ticks`,
`cook_progress_ticks`). On load, subtract `min(elapsed,
fuel_left_ticks)` from both. If `cook_progress_ticks` crosses
the cook threshold, emit the output item (drop into the
furnace's output slot). Repeat until either no fuel or no input.

Cap the catch-up: refuse to advance more than `MAX_OFFLINE_SECS`
(say 7 days). Beyond that, just resume "as if it had been
running that long" - prevents a one-year-old save from spending
minutes catching up.

Crops, brewing, kiln, ... follow the same pattern: a tick
function that takes `(state, elapsed_ticks) -> (new_state,
events_to_emit)`.

## What needs to change in code

1. **`Voxel::world_set_tracked`** already mutates the chunk;
   add a `dirty_chunks: HashSet<ChunkCoord>` insert here.
2. **Chunk serialisation**: implement `Chunk::serialize` ->
   bincode bytes + `Chunk::deserialize`. Currently chunks are
   plain `Vec<BlockId>`, so bincode is straightforward; add a
   light RLE pass before encoding to shrink uniform chunks.
3. **`ChunkSource` trait**: new abstraction in
   `infengine_voxel`. Variants: `Worldgen { seed, variant }`,
   `OnDisk { dir, fallback: Box<dyn ChunkSource> }`. The
   streaming loop calls `source.get(coord)`.
4. **Main-menu UI**: add Worlds list + New / Load / Delete.
   Replace the current "Play offline" button.
5. **Player save**: `PlayerSave { pos, vel, yaw, pitch,
   inventory, air, health, day_time, fly_mode }`. Bincode.
6. **Timer system**: new `time_bound_events: Vec<Timer>` with
   `Timer { kind, pos, due_at_wall_clock_ms }`. Saved + restored.
7. **`Voxel::save_to_disk` + `Voxel::load_from_disk`**: the
   actual write/read entry points. Async on a worker thread so
   they don't stutter the main loop.
8. **Quit handling**: intercept window close + main-menu return
   to fire the final save before unloading.

## Phasing

1. **A — Save format on disk**: implement `meta.json` + chunk
   serialisation + load/save plumbing. Don't wire to UI yet.
   Test via `/save` command in chat.
2. **B — Auto-save loop**: 60 s periodic save during play. Quit
   triggers save. Dirty-chunk tracking.
3. **C — Main-menu Worlds list**: New / Load / Delete buttons.
   Worldgen dropdown moves into the New dialog.
4. **D — Player save**: position + inventory + air + health.
5. **E — Timers (furnace MVP)**: skeleton furnace block with
   cooking timer. Verify the catch-up runs on load.
6. **F — Other time-bound systems**: crops, brewing, kilns -
   each is one PR.
7. **G — Migration tools**: on format-version mismatch, attempt
   migrate or warn the user before play.

## Risk + open questions

- **Save corruption**: write-to-temp + atomic rename for every
  file. Keep a rolling 1-deep backup so a half-written save
  doesn't brick the slot.
- **Cross-machine compatibility**: avoid host-specific bincode
  quirks. Pin `bincode` config in the save header.
- **Mob persistence**: mobs across chunks - their positions
  saved in their chunk's file? Or in a separate `mobs.bin` per
  save? Recommendation: separate file - mobs roam between
  chunks all the time.
- **Multiplayer**: a host's save is the authoritative world.
  Clients don't have local saves. Save lives on the host's
  disk; client just streams chunks like today.
- **MAX_OFFLINE_SECS**: arbitrary cap (a week feels right) so a
  save left for a year doesn't spend minutes simulating.
- **Worldgen version upgrades**: a future "Amplified v2"
  worldgen would alter un-edited terrain at chunk boundaries.
  Mitigation: pin `worldgen.variant` + `worldgen.version` in
  the save and use the same version on reload even when the
  binary has a newer one.
