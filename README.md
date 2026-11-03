# ResinSupport

Prepare STL models for resin printers: view, place, support and slice. Pure Rust, egui + wgpu.

Step 1 (this): a 3D viewer with SolidCraft-style navigation, an origin grid and STL import.

```sh
cargo run -p resinsupport -- part.stl   # files on the command line are imported; --help lists the options
cargo xtask ci                           # what every change must pass
```

Navigation (default Fusion mapping, others under View › Mouse Mapping): middle drag pans, Shift+middle or right drag orbits, the wheel zooms toward the cursor, middle double-click goes home. F fits, P toggles perspective, arrows turn the view, Ctrl+I imports. See `docs/architecture.md`.

Licensed MIT OR Apache-2.0. Parts are ported from SolidCraft; see `ATTRIBUTION.md`.
