# ADR 0003: Use `bevy_ecs` as a standalone ECS library

- **Status:** accepted
- **Date:** 2026-05-11
- **Deciders:** project owner

## Context

The engine needs an Entity-Component-System (ECS) implementation to manage game state. Writing a custom ECS is a multi-year project on its own and largely solved by existing libraries in the Rust ecosystem.

Candidates considered:

- **`bevy_ecs`** - the ECS extracted from the Bevy engine, usable as a standalone crate.
- **`hecs`** - lightweight, single-file ECS focused on simplicity.
- **`legion`** - mature ECS that predates `bevy_ecs`, now largely unmaintained.
- **`specs`** - older ECS, no longer actively developed.
- **Custom implementation.**

## Decision

InfEngine uses **`bevy_ecs` as a standalone library**, without depending on the rest of the Bevy engine framework (`bevy_app`, `bevy_render`, etc.).

## Consequences

### Positive

- The most mature, performant, and actively developed ECS in the Rust ecosystem.
- Strong type system: components are arbitrary `Send + Sync + 'static` types, queries are expressed in the function signature.
- Built-in parallel system scheduler with automatic dependency tracking from query borrows.
- Change detection, sparse storage, relationship support already implemented.
- Reusing `bevy_ecs` directly avoids the maintenance burden of an in-house ECS.

### Negative

- We follow `bevy_ecs` release cadence and breaking changes (it iterates aggressively).
- Some Bevy idioms leak through (e.g. `Schedule`, `World`) and we need to decide which to expose vs wrap.
- We don't get the rest of the Bevy framework, so things like its asset server, render graph, and input system must be built or sourced separately - which is intentional, but requires clarity about what we're using and what we're not.

### Implementation rules

- Depend on `bevy_ecs` only. Do **not** add `bevy`, `bevy_app`, or `bevy_internal` to the dependency graph.
- Do not re-export `bevy_ecs::*` from public APIs. Wrap the types we expose (e.g. `World`, `Schedule`, `Resource`) so that future migrations remain feasible.
- Pin `bevy_ecs` to a workspace-level version; coordinate upgrades across crates.

## Alternatives rejected

- **`hecs`.** Simpler API, but lacks the parallel scheduler and change detection we will need.
- **`legion` / `specs`.** Both effectively unmaintained.
- **Custom ECS.** Not justifiable when `bevy_ecs` already exists, has thousands of users, and outperforms most hand-rolled implementations.
