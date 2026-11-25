---
name: resinsupport-tests
description: "Use when writing, placing or running tests in ResinSupport: a regression test for a bug, an engine test for a command, a hostile-input test for a file format, a UI unit test, or when cargo xtask ci fails a test you did not touch."
---

# Where tests go and how to run them

## Placement
- Commands: `crates/engine/src/tests.rs`, measured against values derived by hand (bounds, volumes, where a ray hits). A regression test goes into the existing test for its area, not a new file named after the bug.
- Geometry and maths: unit tests next to the code (`crates/geom/src/transform.rs`, `crates/geom/src/bvh.rs`).
- File formats: hostile-input tests in the format's own file (`crates/io/src/stl.rs`): truncated, oversized, NaN, wrong counts. They return errors, never panic.
- UI logic: unit tests next to the code (`crates/ui-egui/src/gizmo.rs`, `crates/ui-egui/src/toolbar.rs`, `crates/ui-egui/src/i18n.rs`). Drawing itself is checked by running the app (`.agents/skills/resinsupport-running/SKILL.md`).
- Crash fixes: `.agents/skills/resinsupport-crash-safety/SKILL.md`.

## What earns a test
- A test guards a behaviour a plausible change could break and no other test catches. Don't test what a reader can check at a glance: wiring, constants, a library's own behaviour.
- Assert relations where the exact value is ours to choose ("inside the plate", "wider than", "changed"); assert exact values only when derived by hand (a volume, where a ray hits).
- A bug fix adds a test that fails without it, unless the fix is a few lines checkable by reading.
- Never delete, weaken, `#[ignore]` or skip a test to make a check pass. Changing an existing expectation needs its derivation, and the commit says `Test change: <why>` (`.agents/skills/resinsupport-file-pr/SKILL.md`).

## Writing them
- Tests run in parallel: no shared state, no wall-clock timing.
- A test that runs an egui frame without rendering it clears the frame's textures (`out.textures_delta.clear()`), or egui's debug assert fails on drop.
- Tests may read the repository (`env!("CARGO_MANIFEST_DIR")`), as the catalog sweep does.

## Running
- `cargo xtask fast` while you work and before a commit: fmt, the repository checks, clippy `-D warnings` over the workspace, then the tests of only the packages your change touches (against `origin/master`; `--list` shows them, `-p` names them, `-- gizmo` filters). It runs every step and lists all failures.
- `cargo xtask ci` is the full gate, once before the work is called done: `layers`, `crash-guards`, `lists`, `i18n --check`, `skills`, `design-tokens --check`, fmt, clippy `-D warnings`, then `cargo test --workspace`.
- `cargo test -p resin-ui-egui` alone builds winit without the window-system features the app enables, and fails to compile: test it with the app (`-p resin-ui-egui -p resinsupport`, which `cargo xtask fast` does) or filter the workspace (`cargo test --workspace gizmo`).
