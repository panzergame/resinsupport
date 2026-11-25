---
name: resinsupport-performance
description: "Use when a ResinSupport change is slow, blocks the window, runs long work on large meshes, or must keep building for the browser. Covers the window thread, per-frame work, measuring a speed-up and staying wasm-friendly."
---

# Performance, threads and wasm

## Do not block the window
- Work that can take long on a big mesh (import, a whole-model pass, later slicing) must not stall the frame. Keep per-frame work proportional to what changed: the renderer uploads a mesh once per model (`crates/ui-egui/src/gpu.rs`), a placement only changes a matrix, picking goes through the BVH in model coordinates, and the pointer's pick is cached until the pointer, camera or scene revision changes (`crates/ui-egui/src/supports.rs`).
- A gizmo drag runs `model.place` every frame; what it costs (the scene bounds from every vertex) must stay linear and allocation-light.

## Measure before you claim a speed-up
Time a release build (`cargo run --release -p resinsupport -- model.stl`) on the same model before and after, alternating runs, and report medians with the range, the machine and the model's triangle count, with proof nothing changed (same bounds, volumes, counts).

## The browser build
- Everything below the apps stays buildable for `wasm32-unknown-unknown`: no `std::fs` outside `io` and the apps, file access behind `cfg(not(target_arch = "wasm32"))`, and no dependency that cannot build there (tiny-skia and sys-locale can).
- Application code is Rust only.
