# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- `infengine_window`: minimal winit-based event loop wrapping `EngineWindow`, `AppHandler` trait, and an engine-side `Key` enum that does not leak winit types.
- `examples/sprite_demo`: Phase 1.1 proof of concept that opens a window, logs key events, and exits on Escape.
- Initial Cargo workspace with `infengine_core`, `infengine_window`, `infengine_render`, and `infengine_assets` crate placeholders.
- Dual MIT / Apache-2.0 licensing.
- GitHub Actions CI matrix for Windows and Linux on stable Rust.
- Initial ADRs: language choice (Rust), renderer (`wgpu`), ECS (`bevy_ecs` standalone).
- Project README, CONTRIBUTING guide, Code of Conduct, and Security policy.
