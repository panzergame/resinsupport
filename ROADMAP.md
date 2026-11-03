# Roadmap

## 1. Viewer (done)
- [x] Workspace layering and `cargo xtask ci` (layers, crash guards, fmt, clippy, tests)
- [x] Camera ported from SolidCraft: orbit, pan, zoom to the cursor, standard views, perspective/orthographic, animated view changes
- [x] Mouse mappings ported from SolidCraft (Fusion default, Blender, SolidWorks, …), trackpad pan/orbit/pinch
- [x] Origin: ground grid with adaptive spacing, X/Y/Z axes, axis gizmo
- [x] STL import, binary and ASCII, hostile-input safe; file dialog, drag and drop, command line

Left from SolidCraft's navigation for later: navigation cube and clickable gizmo, continuous orbit, orbit snapping, 3D mouse, macOS inertia handling, held-arrow continuous turning.

## 2. Printer and placement
- Printer profiles (build volume, resolution, pixel size); the grid becomes the build plate
- [x] Object edition and Support edition (Tab); model selection by click (through each model's BVH) or the Models panel
- [x] Move / Rotate / Scale tools (T, R, S): a gizmo ported from SolidCraft and a live panel, measured from the model as imported; supports follow their model
- [x] SolidCraft's chrome: application bar, ribbon (workspace card, a tab per edit mode, icon buttons), tool dialog cards, theme
- [x] UI text in catalogs (English), a Preferences window from SolidCraft's settings table, agent skills
- Lay flat, drop to the plate, auto-arrange, mirroring, undo
- Mesh checks: open edges, inverted normals, self-intersections

## 3. Hollowing and drain holes

## 4. Supports
- [x] Manual tips, first step: preview under the pointer, left click places a 0.5 mm tip, click on a tip selects (Shift toggles), Delete removes
- Tip settings (diameter, depth), supports from tip to plate, overhang and island detection, automatic generation, raft

## 5. Slicing
- Layer images from the mesh, layer preview, exposure settings

## 6. Export
- Layer images in a zip first, then printer formats from public documentation

## Languages
- French, then others: one catalog each (`skills/resinsupport-ui-text/SKILL.md`)

## Platforms
- Web build (`apps/resinsupport-web`): import from bytes (`file.import_stl` takes a path today), wasm CI check
