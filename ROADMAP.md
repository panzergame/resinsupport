# Roadmap

## 1. Viewer (done)
- [x] Workspace layering and `cargo xtask ci` (layers, crash guards, fmt, clippy, tests)
- [x] Camera ported from SolidCraft: orbit, pan, zoom to the cursor, standard views, perspective/orthographic, animated view changes
- [x] Mouse mappings ported from SolidCraft (Fusion default, Blender, SolidWorks, …), trackpad pan/orbit/pinch
- [x] Origin: ground grid with adaptive spacing, X/Y/Z axes, axis gizmo
- [x] STL import, binary and ASCII, hostile-input safe; file dialog, drag and drop, command line

Left from SolidCraft's navigation for later: navigation cube and clickable gizmo, continuous orbit, orbit snapping, 3D mouse, macOS inertia handling, held-arrow continuous turning.

## 2. Printer and placement
- [x] Printer profile (build volume, resolution, pixel size, mirroring), in the print settings
- The grid becomes the build plate
- [x] Object edition and Support edition (Tab); model selection by click (through each model's BVH) or the Models panel
- [x] Move / Rotate / Scale tools (T, R, S): a gizmo ported from SolidCraft and a live panel, measured from the model as imported; supports follow their model
- [x] Typed transforms: with Move, Rotate or Scale open, X/Y/Z and an amount (`T X 50`, `R 90`, `S X 2`), shown live; Enter applies and closes the tool, Esc puts back
- [x] SolidCraft's chrome: application bar, ribbon (workspace card, a tab per edit mode, icon buttons), tool dialog cards, theme
- [x] UI text in catalogs (English), a Preferences window from SolidCraft's settings table, agent skills
- [x] Undo and redo (Ctrl+Z, Ctrl+Y): every scene change is a step, a drag or a typed field one step, Cancel leaves the undo history alone
- [x] Lift: models kept a global height (5 mm) above the plate through import, rotation and scaling, until moved up or down; an object settings card in Object edition
- [x] Arrange (`model.arrange`, and on import unless the `arrange_on_import` preference is off): each model at the free spot nearest the plate's centre, moved and turned a quarter about Z, never scaled
- Lay flat, an Arrange button, mirroring
- Mesh checks: open edges, inverted normals, self-intersections

## 3. Hollowing and drain holes

## 4. Supports
- [x] Manual tips, first step: preview under the pointer, left click places a 0.5 mm tip, click on a tip selects (Shift toggles), Delete removes
- [x] Support settings card: Light, Medium and Heavy presets of tip, body, base and raft sizes, validated, kept in the preferences, never undo steps
- [x] Supports from tip to plate: straight down or with one knee within the max tilt, clear of every model by `body.clearance`; unreachable tips kept and flagged
- [x] Support parts selectable: body segments and joints (a handle drawn over everything at the head and each knee)
- [x] Move arrows on selected joints and segments; an edited support keeps its placed corners (only its tip follows the model), checked and flagged, never routed again
- [x] Manual branches: a double press on a tip, then a press on a surface, places a straight tip to that support's head, sharing its body; any tip deletable, the body going with the last
- A button for `support.reset_route`; moving the tip along the surface
- Overhang and island detection, automatic generation, raft

## 5. Slicing
- [x] Print settings: crate `resin-slice` (printer profile, print settings, the sliced job's types), `print.configure`, `print.reset`, `print.settings`, a print settings card, kept in the preferences, never undo steps
- [x] Slice edition and the slicer: a rasterizer ported from mslicer (nonzero fill, anti-aliasing, clipping), supports printed as drawn, `print.slice`, a background job with progress and cancel, the plate's outline
- [x] Layer preview: the layer slider, the 3D view cut at the shown layer and capped with its image, the 2D layer view (tiles and mipmaps under the device's texture limit), the layer's card (exposure, lit area, lit regions)
- Layer checks, later (UVtools `LayerIssueConfiguration`): islands, overhangs, resin traps, touching bounds, print height, empty layers; a cap under a raised bottom handle; decoding on the GPU where compute shaders exist (mslicer `slice_preview/decompress.rs`), once measured against the CPU decode
- Printer profile, later: portrait orientation (PrusaSlicer `display_orientation`), gamma or exposure remap (PrusaSlicer `gamma_correction`, mslicer `ExposureRemap`), XY absolute and XYZ scale correction (PrusaSlicer `absolute_correction`, `relative_correction`)
- Print settings, later: two-stage lift and retract, waits before cure, after cure and after lift (CTB v4 and up); elephant-foot compensation and minimum width (PrusaSlicer `elefant_foot_compensation`, mslicer `elephant_foot_fixer.rs`); hollowing, Z bleed correction (PrusaSlicer `hollowing_*`, `zcorrection_layers`)

## 6. Export
- Layer images in a zip first, then printer formats from public documentation or GPLv3-compatible implementations

## Languages
- French, then others: one catalog each (`skills/resinsupport-ui-text/SKILL.md`)

## Platforms
- Web build (`apps/resinsupport-web`): import from bytes (`file.import_stl` takes a path today), wasm CI check
