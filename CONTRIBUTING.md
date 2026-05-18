# Contributing to InfEngine

Thank you for your interest in contributing! InfEngine is a long-running open-source effort and we welcome contributions of all sizes.

> **Status note.** The project is currently in **Phase 0/1 (bootstrap and proof-of-concept)** and the repository is private. Once a working sprite demo lands, the repo becomes public and this document becomes the canonical contributor guide. For now, this file establishes the conventions we want to follow from the very first commit.

## Code of Conduct

By participating in this project you agree to abide by the [Code of Conduct](CODE_OF_CONDUCT.md).

## Ground rules

- **All code, comments, identifiers, commit messages, issues, and PR descriptions are in English.** This is non-negotiable for an OSS project.
- **No `unsafe` without justification.** Any `unsafe` block must be accompanied by a `// SAFETY:` comment explaining the invariants the caller must uphold.
- **No new top-level dependencies without an ADR.** If you want to add a non-trivial crate (a renderer, a physics lib, a scripting host), open an ADR in `docs/adr/` first.
- **Public APIs should not re-export types from `wgpu`, `winit`, `bevy_ecs`, etc.** Wrap them. Upstream breaking changes should not cascade through our API.
- **File-size policy.** New files keep functions under 100 lines, match arms under 20 lines, and the file itself under 500 lines (hard cap 1000). Plan a module folder + sub-modules BEFORE writing code, not after. Pre-existing oversized files are grandfathered but cannot grow further. See [`docs/design/file_size_policy.md`](docs/design/file_size_policy.md) for the per-phase folder layouts the project commits to.

## Development workflow

1. Fork the repo (once public) and create a feature branch off `main`.
2. Make your changes. Keep commits focused and atomic.
3. Run the local checks before pushing:
   ```sh
   cargo fmt --all
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --workspace
   ```
4. Push and open a pull request against `main`.
5. Wait for CI to be green and for a review.

## Commit messages — Conventional Commits

We follow [Conventional Commits](https://www.conventionalcommits.org/) so that we can automate changelog generation.

Format:

```
<type>(<scope>): <short summary>

<optional longer body>

<optional footer(s)>
```

Allowed types:

| Type       | When to use it                                                         |
|------------|------------------------------------------------------------------------|
| `feat`     | A user-visible new feature                                              |
| `fix`      | A user-visible bug fix                                                  |
| `perf`     | A performance improvement with no API change                            |
| `refactor` | Restructuring code without changing behavior                            |
| `docs`     | Documentation-only changes                                              |
| `test`     | Adding or refactoring tests                                             |
| `build`    | Build system, dependencies, Cargo.toml                                  |
| `ci`       | CI configuration                                                        |
| `chore`    | Maintenance tasks that don't fit elsewhere                              |
| `revert`   | Reverts a previous commit                                               |

Scopes correspond to crate names without the `infengine_` prefix (`core`, `render`, `window`, `assets`, etc.) or to areas like `ci`, `docs`, `adr`.

Breaking changes are marked with `!` after the type/scope and a `BREAKING CHANGE:` footer:

```
feat(render)!: replace forward renderer with forward+

BREAKING CHANGE: `Renderer::new` no longer takes a `MsaaConfig`. Pass it
via `RendererBuilder::msaa` instead.
```

## Architecture Decision Records (ADRs)

Significant design decisions (renderer architecture, scripting host, asset format, etc.) are recorded as ADRs in `docs/adr/`. Format follows [MADR](https://adr.github.io/madr/) with a numeric prefix:

```
docs/adr/0004-asset-cache-strategy.md
```

If your change implies a design decision worth preserving, write the ADR in the same PR.

## Style

- `rustfmt` is the source of truth. Don't reformat unrelated code in your PR.
- Prefer `///` doc comments on public items.
- Avoid `print!`/`println!` outside of examples. Use `tracing` macros.
- `Result`-returning functions in libraries should use `thiserror` for their error types; binaries can use `anyhow`.

## Testing

- Unit tests live alongside the code they test (`#[cfg(test)] mod tests`).
- Integration tests go in each crate's `tests/` folder.
- GPU-touching tests should be gated behind a `--features gpu-tests` flag (these can't run on CI runners without a GPU).

## Reporting bugs and asking for features

Once the repo is public, use the GitHub issue tracker. Before opening an issue:

- Search existing issues to avoid duplicates.
- Include the OS, GPU, and `rustc --version` output.
- Provide a minimal reproduction if possible.

## License

By contributing to InfEngine, you agree that your contributions will be dual-licensed under the MIT and Apache-2.0 licenses, as described in the [README](README.md#license).
