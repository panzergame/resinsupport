---
name: resinsupport-file-formats
description: "Use when touching model import (STL today), project files (.rsup), or adding a printer file format or any export in ResinSupport. Covers where readers live, hostile input, public specifications and how imports reach the scene."
---

# File formats: import and export

## Where they live
- Readers and writers are in `crates/io/src/`, one file per format (`crates/io/src/stl.rs`), with their hostile-input tests in the same file.
- They take bytes or a path and return data (`Mesh`), never touch the scene: the engine adds the result through a command (`file.import_stl` in `crates/engine/src/cmd/model.rs`).

## Project files
- `.rsup` is SolidCraft's design container, reduced (`crates/io/src/project.rs`): keep it in step with SolidCraft's when changing the package layout, and its format notes in `docs/architecture.md`.
- A change to what `document.json` holds bumps `resinsupport/N`, and the reader brings older versions up to date; a newer one is refused, never guessed at.
- It keeps what the user made (models, placements, supports), never a setting or anything derived.
- Files are written through `io::write_atomic`, never `std::fs::write`.

## Input is hostile
- Cap sizes and counts before allocating; check every number is finite; a triangle that cannot be used is dropped and counted, and the count is reported to the user.
- Never panic on a file: return `IoError` (`skills/resinsupport-crash-safety/SKILL.md`).

## Printer formats
- From public documentation or a GPLv3-compatible open-source implementation, never from a proprietary slicer's code or output (`skills/resinsupport-ground-rules/SKILL.md`). Name the document or source file you followed in the format file's header.
- The first export will be layer images in a zip (`ROADMAP.md`).

## The browser
A web build imports from bytes, not a path; keep format code free of `std::fs` so it can (`skills/resinsupport-performance/SKILL.md`).
