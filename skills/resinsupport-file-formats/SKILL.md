---
name: resinsupport-file-formats
description: "Use when touching model import (STL today) or adding a printer file format or any export in ResinSupport. Covers where readers live, hostile input, public specifications and how imports reach the scene."
---

# File formats: import and export

## Where they live
- Readers and writers are in `crates/io/src/`, one file per format (`crates/io/src/stl.rs`), with their hostile-input tests in the same file.
- They take bytes or a path and return data (`Mesh`), never touch the scene: the engine adds the result through a command (`file.import_stl` in `crates/engine/src/cmd/model.rs`).

## Input is hostile
- Cap sizes and counts before allocating; check every number is finite; a triangle that cannot be used is dropped and counted, and the count is reported to the user.
- Never panic on a file: return `IoError` (`skills/resinsupport-crash-safety/SKILL.md`).

## Printer formats
- From public documentation only, never from another slicer's code or output (`skills/resinsupport-ground-rules/SKILL.md`). Name the document you followed in the format file's header.
- The first export will be layer images in a zip (`ROADMAP.md`).

## The browser
A web build imports from bytes, not a path; keep format code free of `std::fs` so it can (`skills/resinsupport-performance/SKILL.md`).
