---
name: resinsupport-crash-safety
description: "Use when writing or reviewing non-test Rust in ResinSupport, handling model files or command parameters, or investigating a crash. A crash loses someone's work, so this outranks feature work."
---

# Never crash

## In non-test code
- No `unwrap()`, `expect()`, `panic!`, `unreachable!`, `todo!` or `unimplemented!`. Every crate root (and `xtask/src/main.rs`) carries `#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]`; `cargo xtask crash-guards` checks it and `cargo xtask ci` runs clippy with `-D warnings`.
- No `unsafe`: `unsafe_code = "forbid"` in the workspace lints, with no exception.
- Errors are `Result<T, E>` through the crate's error type (`EngineError`, `IoError`) and `?`. An unfinished feature returns an error ("not supported yet: …"); it never panics.
- Input-derived numbers are hostile (command parameters, STL files): `get()` not `[i]`, finite checks, caps on sizes and counts. A placement is checked before it is applied (`MAX_EXTENT`, `SCALE_RANGE` in `crates/engine/src/session.rs`).
- Last resort: `Session::execute` (`crates/engine/src/session.rs`) runs every command under `catch_unwind`; a failing or panicking command restores the session and reports `EngineError::Internal`. Do not rely on it.
- Text from a catalog, a file name or a model name is data: placeholders are filled literally (`i18n::format`), never re-read as a template.

## Proving a crash fix
- Every crash fix lands with a regression test in the area's existing test file (`skills/resinsupport-tests/SKILL.md`).
- `hostile_params_never_panic` in `crates/engine/src/tests.rs` runs every command with junk parameters; a new command gets it for free. Add a junk row when a command takes a new parameter shape.
- A file format's hostile-input tests sit in its own file (`crates/io/src/stl.rs`).
