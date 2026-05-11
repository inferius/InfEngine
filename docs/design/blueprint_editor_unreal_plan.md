# Blueprint editor - Unreal-parity roadmap

## Why this doc exists

The current InfEngine blueprint editor renders a panning canvas with
typed pins, a search-driven context menu, drag-and-drop variable /
function spawning, and a stack-based VM that compiles graphs into
bytecode. It works, but the author-time experience still feels
clunky compared to Unreal Engine's Blueprint editor, which sets the
genre standard. This document inventories what UE Blueprints do that
InfEngine's editor does not yet do, and lays out a multi-phase plan
to close the gap.

The plan is incremental: every phase ships a usable editor and a
green test suite. We never wait on a "big bang" rewrite.

## Glossary

- **Inline pin default**: a small widget on an unconnected input
  pin that lets the user type a constant directly. Unreal shows a
  text box next to a float pin, a checkbox next to a bool pin, an
  XYZ triplet next to a vector pin.
- **Pure node**: a data-only node with no exec pins. Pure nodes are
  evaluated on-demand by their consumers.
- **Knot / reroute node**: a tiny invisible node that lets the user
  bend a wire around obstacles without changing semantics.
- **Promote to variable**: right-click a pin -> creates a new
  variable of that pin's type and rewires the pin to read from it.
- **Watch value**: during Play-In-Editor, the editor shows the live
  value flowing through a marked pin / variable each frame.

## Where we are today

What already works:

- Typed pins (Float / Bool / Vec3 / Text / EntityRef / Exec).
- Categorised + searchable context menu, pin-aware filter.
- Drag-and-drop variables (Get / Set picker) and functions
  (CallFunction).
- Side-panel inspector for the selected node's params.
- Per-tab workspace, sub-graphs (events / functions / macros).
- Compile + VM with stack ops, branching, jumps, variables,
  function inlining, sequences, math / logic / comparison.
- Engine API nodes: set_position / rotation / scale, play_sound,
  delta_time, time_seconds.

What does not exist yet:

- Inline pin defaults (the user must wire a `const_float` node to
  set "5.0" on a math input). This is the loudest pain point.
- Visually distinct node headers (every node looks identical except
  for pin shapes).
- Wire manipulation (no break-link, no hover highlight, no knots).
- Comment boxes / group selection.
- Refactor support (rename variable / function does not refactor
  references).
- Watch values during Play; no per-pin live-readout.
- Find-in-graph (no Ctrl+F).
- Bookmarks / minimap / focus.
- "Promote to variable" / "Convert to function".
- Hot-reload of blueprint changes without leaving Play mode.

## Phase 1 - Inline pin defaults (current)

**Goal**: every unconnected input pin shows an inline editor so the
user can type a value without dropping a `const_*` node.

Scope:

1. Data: store per-pin defaults under `node.params["pin_<name>"]`.
   No new fields; no breaking changes to saved blueprints.
2. Compile: when no edge feeds an input pin, parse the default from
   `params` instead of pushing a hardcoded zero. Each numeric / bool
   site that currently emits `Op::PushFloat(0.0)` / `Op::PushBool(...)`
   becomes a call into a `pin_default_*` helper.
3. UI: while rendering a node, walk its input pins; for each pin
   that has no incoming edge AND is a literal-friendly type
   (Float / Bool / Vec3 / Text), put an inline widget next to the
   pin label.
4. Exec pin inputs never show an inline editor.
5. Test: extend exec.rs tests with a "default on unconnected pin"
   case.

Out of scope for Phase 1: changing wire visuals, refactoring how
nodes are drawn, gizmo-style drag for floats. Those land later.

## Phase 2 - Visually distinct nodes

- Colored header bar per category (Events red, Math green, Control
  orange, Variables yellow, Engine API blue, Functions purple).
- Node icon glyph in the header (optional).
- Selected node gets an outline glow.
- Pin row hover tooltip showing type and current connection state.

## Phase 3 - Wire interaction

- Wire hover highlight (draw thicker / brighter on hover).
- Right-click on a wire opens "Break Link" / "Reroute Here".
- "Reroute Here" inserts a `knot` node at the cursor and rewires
  through it. Knot nodes are pure exec-or-data passthroughs.
- Drag a wire's midpoint onto another pin to relocate the wire's
  endpoint without deleting and recreating.

## Phase 4 - Selection, comments, groups

- Box-select with Shift+drag on empty canvas.
- Multi-node move / delete / copy / paste.
- Comment boxes: drag a colored rectangle around any group of
  nodes; moving the rectangle moves contained nodes; rectangle text
  is editable.
- Bookmarks: Ctrl+1..9 to mark a viewport position; Shift+1..9 to
  jump.

## Phase 5 - Refactor + Promote / Convert

- Rename variable / function refactors every reference in the
  blueprint (and across blueprints that import it, once that
  exists).
- "Promote to Variable" on any data output: creates a new variable
  of that type, replaces the output's consumer with a `set_var` +
  `get_var` pair.
- "Convert to Function": collapse the selected nodes into a new
  function, replacing them in the original graph with a
  `call_function` node.
- "Expand Function": inverse of the above; inlines a function back
  into its caller (one-shot, not destructive).

## Phase 6 - Play-time integration

- Watch values: right-click any pin / variable -> "Watch". During
  Play, the editor shows the latest value as a small text overlay
  next to the pin.
- Breakpoints: right-click any exec pin -> "Toggle Breakpoint". On
  hit, Play freezes and the editor highlights the current op.
- Step / Continue controls in the toolbar while paused on a
  breakpoint.

## Phase 7 - Discoverability + scale

- Find-in-graph (Ctrl+F): regex / fuzzy search node labels, pin
  names, comments; jumps the viewport to each hit.
- Minimap (collapsible, bottom-right corner): outline + viewport
  rect.
- Auto-arrange: walk the graph topologically and lay nodes out so
  exec flows left-to-right.
- Performance: bucket nodes by spatial hash so hit-testing scales
  past a couple hundred nodes.

## Phase 8 - Cross-blueprint integration

- "Function libraries": separate blueprint type that exposes its
  functions globally. Other blueprints can call them via
  `call_function` with a `library::function_name` syntax.
- Interface blueprints: a list of function signatures other
  blueprints can implement; allows polymorphic dispatch at runtime.
- Event dispatchers: a multicast pattern. A blueprint declares an
  event; other blueprints subscribe; the dispatcher fires it.

## Phase 9 - Live editing

- Hot-reload: modifying a blueprint while in Play recompiles and
  swaps the running program at the next tick, preserving variable
  state where the variable name + type matches.

## What we intentionally skip vs Unreal

- Animation Blueprints, Anim Graph, State Machines - belongs in a
  separate animation editor, not the general BP editor.
- Materials Blueprints / Niagara Blueprints - separate authoring
  surface; the general BP editor stays focused on logic.
- Native C++ inheritance "Blueprintable" plumbing - InfEngine
  scripting is C# via CoreCLR; the BP editor compiles to its own
  VM, not C++ thunks. Cross-language interop happens at the entity
  / asset boundary, not inside the BP graph.
