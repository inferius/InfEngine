# Session roadmap - planning record

Status: **DRAFT - planning only**. Filed at user request after
the water + save round so the next session has a single anchor
that lists every committed direction. Each entry links to its
own detailed design doc when one exists.

## The 9 items the user pinged

| # | Goal | Detail doc | Phase |
|---|------|------------|-------|
| 1 | Split the 20 kLOC `voxel_demo_mp/src/main.rs` into focused modules | `voxel_demo_mp_module_split.md` | A |
| 2 | Dedicated CLI server: headless host that owns the authoritative world. Solves "one source of truth" + offline save in one stroke | _new (see below)_ | B |
| 3 | MC features: chests, furnaces, crafting + UI icons for the materials | _new_ | C |
| 4 | In-game menu rework, more QoL | _new_ | G |
| 5 | Make the world "alive" - mobs that act, weather, ambience | _new_ | H |
| 6 | Pipe / conduit mod system, BuildCraft-style | _new_ | I |
| 7 | Multi-dimension support (Nether / End analogues) | _new_ | F |
| 8 | Biome system + procedural epic trees | _new_ | E |
| 9 | GUI styling support (theme presets, animations) | _new_ | G |

## Intermezzo (user side question)

> Nas engine umi vlastne kompilovat rust? ... Muzeme proste dat v
> enginu play nebo export, aby nam to pak zkomiploval do rustu?

= can the engine itself compile Rust like Unity compiles C#? A
"Play" / "Export" button in an editor.

That's the original roadmap's Phase 8 (scripting / plugin layer)
plus an editor frontend. **Filed as Phase J** - intentionally
last because it's a 2-3 month project on its own and the
hot-reload pipeline only pays off once there are domain APIs to
script against. See `0001-language-rust.md` ADR for the existing
language stance; the scripting decision is in `Faze 8` of the
original plan file.

## Dependencies (read top-to-bottom)

```
A (split) ─┬─> B (server CLI)
           ├─> C (chest/furnace/crafting + items)
           ├─> D (recipe DSL)
           ├─> G (menu UX + theming)
           ├─> E (biomes + trees)
           ├─> F (multi-dim)
           └─> H (alive world)

C ─> I (pipes / conduits)  -- need items first
B ─> J (engine SDK)         -- server is the headless target
                                the editor "Play" loop reuses
```

## Phase descriptions

### Phase A - module split

Detail: `voxel_demo_mp_module_split.md`. 10-phase plan: blocks,
liquid, net, decorations, drops, inventory, chat, mining,
player, mob_ai + worldgen + ui. Each phase = one PR, behaviour
must stay byte-identical. **First concrete action**: extract
`blocks.rs` - lowest coupling, the BlockType table + flag
helpers + tool-tier table.

### Phase B - dedicated server CLI

New binary at `examples/infengine_server/`. Strips out renderer
+ UI + audio. Reuses `NetRole::Server` from the demo (which
already exists). Loop:

```
let mut world = build_world(seed, gen);
let mut server = NetServer::bind(addr)?;
loop {
    let dt = clock.tick();
    server.poll();          // accept connections, drain messages
    world.tick_liquids(dt);
    world.tick_mobs(dt);
    autosave_if_due();
    if shutdown.requested() { save_and_exit(); }
}
```

Reads CLI flags: `--port 4242 --world my-world --seed 42
--worldgen amplified`. Console output: connection log, world
events, occasional perf summary. Optionally support a small
JSON REPL on stdin (`{"cmd":"save"}` etc).

Multi-save lives here too - the world dir is `~/.infengine/
worlds/<slug>/`, host opens / creates one of those.

### Phase C - chests + furnaces + crafting

3 sub-PRs:

1. **Item registry separate from block registry**. `ItemType {
   id, name, icon_atlas_uv, max_stack }`. Blocks can map to an
   item ("place this block"), but items can exist without
   placeable form (raw_iron, stick, etc).
2. **Chest block**. New `F_CONTAINER` flag. Inventory grid
   (27 slots) opens when player RMB's it. Same drag-drop UI as
   the existing player inventory. State persists in the save
   (currently lost - need a `containers: HashMap<[i32;3],
   Inventory>` side table).
3. **Furnace block**. Three slots: fuel / input / output. Cook
   timer ticks server-side. **This unblocks the time-warp
   catch-up from the save-design doc** - on load, the furnace
   reads `last_played - now` and runs N cook steps to catch up.

### Phase D - crafting table

3x3 craft grid block. Recipe definition format:

```rust
Recipe {
    pattern: ["WWW", " S ", " S "],  // none = empty
    ingredients: { 'W': WoodPlank, 'S': Stick },
    result: (Pickaxe, 1),
    shaped: true,
}
```

Recipes loaded from `assets/recipes/*.ron`. Crafting UI mirrors
the chest panel layout. Shaped + shapeless support.

### Phase E - biomes + trees

1. **Biome map**: low-frequency perlin/voronoi over (x, z)
   picks one of a set: Forest / Plains / Desert / Tundra /
   Jungle / Ocean / Mountains / Swamp. Each biome ships with:
   - height curve coefficients
   - surface block (grass, sand, snow, ...)
   - tree density + species
   - mob spawn weights
2. **Epic tree generator**: parametric L-system. Trunk segment
   length, branch angle distribution, branch density,
   canopy radius. Species presets: oak (current), pine /
   spruce (tall conifer), giant sequoia (epic - matches the
   screen reference), willow, jungle bigleaf. Each grows
   procedurally inside the chunk generator.

Reference screenshots filed: 30-block giant sequoias, massive
oak with hanging leaves. Tree generator should be tweakable
enough to grow both with parameter changes.

### Phase F - multi-dimension

`Dimension { id, name, worldgen, gravity, sky_palette,
ambient_track, ... }`. Default dim = current overworld.
Portal block: stand on it, the server teleports you to the
linked dimension's matching coord. World save splits into
`worlds/<slug>/dim-<id>/chunks/`.

This naturally supports the "fun custom worlds" mods the user
mentioned - a mod plugin registers a new `Dimension` entry +
its worldgen + its sky.

### Phase G - menu UX + GUI theming

1. **Theme module**: `GuiTheme { font, scale, color_palette,
   button_radius, ... }`. egui style override applied once at
   startup. Presets: Default / Dark / High Contrast / "MC
   Classic".
2. **Worlds menu polish**: thumbnail preview per save (last
   screenshot taken on quit), last-played timestamp, "New
   World" dialog with name + seed + worldgen + biome-set.
3. **Pause menu cleanup**: in-game ESC opens a single overlay
   with Resume / Settings / Save / Quit-to-Menu.

### Phase H - alive world

Open ended. First moves:
- Mob AI improvements: pathfinding (A* up to 16 voxels),
  flee-from-player on damage, target-player-on-line-of-sight
  for hostile.
- Weather: rain / snow particle emitters tied to biome temp +
  current weather state.
- Ambient SFX: birdsong in forest, wind on mountain, surf at
  beach.
- Day/night events: sleep when in bed, hostile mobs spawn at
  night.

### Phase I - pipe / conduit mod

References: BuildCraft, Mekanism, Applied Energistics. A
"pipe" block transports items from one inventory to another
along a connected pipe network.

Needs:
- Block adjacency network builder (BFS along pipe blocks).
- Item-in-pipe entity that tweens through the network.
- Insertion API on inventories (chest accepts via top side,
  emits via bottom).
- Filter pipe variants (pickup only matching items).

Pivots on Phase J's plugin trait if we want this as a mod
instead of baked-in code.

### Phase J - engine SDK (intermezzo)

The original roadmap's Phase 8. Two flavours both possible:

1. **WASM plugins**: pipes compile to WASM, loaded at runtime
   via `wasmtime`. Sandboxed. Smaller ecosystem but easier
   safety story.
2. **C# / .NET hosting**: CoreCLR embedded in Rust, mods as
   .NET DLLs. Larger gamedev ecosystem, harder hosting story.

The "engine compiles Rust" question is a different layer: the
EDITOR (egui frontend) shells out to `cargo build` on a
mod-source-dir to produce a hot-reloadable .dll. Doable but
needs an editor in the first place. Filed for after the demo
solidifies.

## Order of operations (committed)

1. **A1**: extract `blocks.rs`. Single-file mechanical move +
   `pub(super)` flagging. Lowest risk, immediate kLOC drop in
   main.rs.
2. **A2**: extract `liquid.rs` (+ `water_tests` inside it).
3. **A3**: extract `net.rs`.
4. **A4**: decorations / drops / inventory.
5. **A5**: chat / mining / player / worldgen / mob_ai / ui.
6. **B**: dedicated server bin + multi-save dirs.
7. **C**: items + chests + furnaces.
8. **D**: crafting recipes.
9. **E**: biomes + trees.
10. **F**: dimensions.
11. **G**: menu + theming.
12. **H**: alive world (parallel after F).
13. **I**: pipe plugin (after J starts).
14. **J**: engine SDK (parallel late-stage).

Each numbered step = one PR with green build / clippy / tests.

## Open questions (parked)

Asked but the user dismissed the answer prompts - re-ask when
they pick the next step:

- Phase B priority: server first vs chests / furnaces /
  crafting first vs biomes first vs dimensions first.
- Plugin layer timing: brzy (po B), middle (po E), or konec
  (per orig roadmap).
- Engine Rust-compile (J) timing: after stability, parallel,
  or skip for now.
- Save format: switch to per-chunk snapshots now, or keep
  EditMsg log until pools grow large.
