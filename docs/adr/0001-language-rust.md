# ADR 0001: Engine core language is Rust

- **Status:** accepted
- **Date:** 2026-05-11
- **Deciders:** project owner

## Context

InfEngine needs a long-term implementation language for its core (renderer, ECS, asset pipeline, runtime). The project's goals are:

- AAA-comparable steady-state frame performance (no GC pauses);
- multi-platform from day one (Windows, Linux, later macOS, web, mobile);
- single-developer feasibility for at least the first 1-2 years;
- open-source contributor accessibility;
- room for a high-level scripting layer for game logic / mods.

Candidates considered: C++ 23, Rust, C# (.NET 9 NativeAOT), Zig, and a C++/C# hybrid.

## Decision

The engine core is written in **Rust on the stable channel**. Game scripting / plugin code will run in a separate language (currently planned: C# via embedded .NET runtime; see a future ADR).

## Consequences

### Positive

- Performance parity with C++ in real-world benchmarks (LLVM backend, zero-cost abstractions).
- Memory and data-race safety enforced at compile time, eliminating an entire class of bugs that historically dominate game-engine maintenance cost.
- Cargo provides a first-class build system, dependency management, test runner, and documentation generator out of the box.
- The Rust gamedev ecosystem (`wgpu`, `bevy_ecs`, `winit`, `egui`, `rapier`) is mature enough to build a 2D + 3D engine without writing every subsystem from scratch.
- `wgpu` gives us Vulkan, DirectX 12, Metal, and WebGPU from a single codebase, directly satisfying the cross-platform requirement.

### Negative

- Compile times are slower than C# and comparable to C++. Mitigation: workspace splitting, `cargo-watch`, `mold` linker on Linux, `lld` on Windows.
- The borrow checker has a real learning curve. The project owner is intermediate-level and still wrestling with it; this is the largest single risk to the project.
- Smaller pool of contributors familiar with both Rust and game engine architecture compared to C++.

### Risk mitigation

A formal **checkpoint** is scheduled at the end of Phase 1 (proof-of-concept sprite demo, 3-6 weeks). If borrow-checker friction makes engine development infeasible at that point, the project pivots to C++ 23 with `bgfx` + `SDL3` rather than continuing to fight the language.

## Alternatives rejected

- **C++ 23.** The classic choice. Rejected because the safety benefits of Rust outweigh the ecosystem advantage of C++ for a single-developer project where every memory bug is a multi-day investigation.
- **C# (.NET 9 NativeAOT).** Excellent ergonomics and iteration speed, but GC pauses introduce frame-time variance that is unacceptable for an engine targeting 144 Hz+ workloads. FlaxEngine demonstrates that pure-C# engines are possible, but only by aggressively avoiding allocations on the hot path - at which point the ergonomic advantage is lost.
- **Zig.** Technically attractive but the ecosystem is too small to build a 2D + 3D engine without writing significant infrastructure ourselves.
- **C++ core + C# scripting hybrid (Unity model).** Powerful but requires two runtimes, a stable FFI boundary, and two toolchains. Not feasible for a single developer in Phase 0-4. The hybrid model is still planned, but as **Rust core + C# scripting** rather than C++ core.
