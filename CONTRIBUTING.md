# Contributing to Oxidraft

Thanks for helping! A few guidelines keep things smooth.

## Before you open a PR

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

CI runs the same checks on Linux, macOS and Windows.

## Adding a command

1. Create `crates/oxidraft-commands/src/<name>.rs` with a type implementing `Command`.
   `line.rs` is the reference implementation.
2. Register it in `COMMANDS` in `engine.rs` with its name, aliases and description.
3. Make all edits through `ctx.doc` so they land in the command's undo transaction.
4. Add tests in `engine.rs` that drive the command through `Engine::submit`/`click`,
   the same way a user would.

Match the prompts, options and behavior of established CAD tools where it is
reasonable — users rely on muscle memory.

## Geometry

Keep geometry code in `oxidraft-geom` free of UI and document types, and give
it unit tests, including degenerate inputs (zero-length segments, coincident points).

## Licensing

By contributing you agree that your contributions are dual-licensed under
MIT OR Apache-2.0, as the rest of the project.
