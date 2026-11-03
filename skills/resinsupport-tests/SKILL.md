---
name: resinsupport-tests
description: "Use when writing, placing or running tests in ResinSupport: a regression test for a bug, an engine test for a command, a hostile-input test for a file format, a UI unit test, or when cargo xtask ci fails a test you did not touch."
---

# Where tests go and how to run them

## Placement
- Commands: `crates/engine/src/tests.rs`, measured against values derived by hand (bounds, volumes, where a ray hits). A regression test goes into the existing test for its area, not a new file named after the bug.
- Geometry and maths: unit tests next to the code (`crates/geom/src/transform.rs`, `crates/geom/src/bvh.rs`).
- File formats: hostile-input tests in the format's own file (`crates/io/src/stl.rs`): truncated, oversized, NaN, wrong counts. They return errors, never panic.
- UI logic: unit tests next to the code (`crates/ui-egui/src/gizmo.rs`, `crates/ui-egui/src/toolbar.rs`, `crates/ui-egui/src/i18n.rs`). Drawing itself is checked by running the app (`skills/resinsupport-running/SKILL.md`).
- Crash fixes: `skills/resinsupport-crash-safety/SKILL.md`.

## Writing them
- Tests run in parallel: no shared state, no wall-clock timing.
- A test that runs an egui frame without rendering it clears the frame's textures (`out.textures_delta.clear()`), or egui's debug assert fails on drop.
- Tests may read the repository (`env!("CARGO_MANIFEST_DIR")`), as the catalog sweep does.

## Running
- `cargo xtask ci` is the gate before every commit: `layers`, `crash-guards`, `lists`, `i18n --check`, `skills`, fmt, clippy `-D warnings`, then `cargo test --workspace`.
- Run the workspace's tests, not one crate's: `cargo test -p resin-ui-egui` alone builds winit without the window-system features the workspace enables, and fails to compile. Filter instead: `cargo test --workspace gizmo`.
