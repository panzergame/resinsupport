# Architecture

| Layer | Crate | Role |
|---|---|---|
| L0 | `geom` | vectors, bounding boxes, triangle meshes and their measures (area, volume), ray hits (watertight test, BVH), spheres |
| L1 | `render` | headless view math: the camera, projections, view animations, grid and axis geometry |
| L3 | `io` | model file import (STL binary and ASCII) |
| L4 | `engine` | the session (scene: models, support tips, selection; camera) and the command registry |
| L5 | `ui-egui` | the egui front end: viewport renderer (wgpu), navigation, menus, panels |
| apps | `resinsupport` | the desktop app |

L2 is kept free for the print job (printer profile, placement, supports), which will sit between the view math and the file formats. Support tips live in `engine::Scene` until supports get settings and generation, then move there.

## Commands
`Session::execute(id, params)` looks the id up in `crates/engine/src/cmd.rs`, runs it on a clone-protected session (a failing or panicking command leaves the session as it was), and returns JSON. The UI only calls commands, apart from continuous camera drags, which move `session.camera` directly as SolidCraft's do.

| Id | Does |
|---|---|
| `file.import_stl` | read an STL from `path`, add it to the scene, frame it unless `fit: false` |
| `model.delete` | remove model `id` and its supports |
| `model.list` | the models, their triangle counts, bounds and whether selected |
| `model.move` | move models `ids` (by default the selected ones) `by` [x, y, z] mm |
| `model.place` | set `models: [{ id, position?, rotation?, scale? }]`, each measured from the model as imported |
| `model.rotate` | turn models `degrees` [x, y, z], about X then Y then Z, around their joint bounds' centre |
| `model.scale` | scale models by `factors` [x, y, z] (or one number), each 0.001 to 1000, along each model's own axes |
| `model.select` | select models `ids`: `replace` (default; empty clears), `add` or `toggle` |
| `scene.clear` | remove every model (and its supports) |
| `support.add` | a tip on model `model` at `point`, with outward `normal` |
| `support.delete` | remove supports `ids`, by default the selection |
| `support.list` | the supports: tip, normal, model, whether selected |
| `support.select` | select `ids`: `replace` (default; empty clears), `add` or `toggle` |
| `view.fit` | frame everything for a view `aspect` wide |
| `view.projection` | `perspective`, `orthographic` or `toggle` |
| `view.standard` | `front`, `back`, `left`, `right`, `top`, `bottom`, `iso` |
| `view.turn` | turn the view `degrees` (default 15) `left`, `right`, `up` or `down` |

## Viewport
`viewport::show` allocates the view, runs `navigation::handle`, builds the overlay (`render::Overlay`) and hands a `ViewFrame` to an egui paint callback. `gpu::Renderer` draws into egui's own render pass, which the app opens with a 24-bit depth buffer and 4× MSAA (`DEPTH_BITS`, `MSAA_SAMPLES`): background gradient, models (flat-shaded; inside-out faces tinted red), then support tips (an instanced sphere), then grid and axes, which fade toward the grid's edge. Meshes upload once per model id, as imported, split under 128 MiB per buffer; each model's placement (3×4 rows, the cofactor rows for its normals) and colour are a one-instance vertex attribute.

## Navigation
Ported from SolidCraft. The mouse mapping is a preset table (`prefs::NAV_PRESETS`); the default, Fusion, pans with the middle button, orbits with Shift+middle or the right button, and zooms with the wheel toward the model point under the cursor. Ctrl+left drag orbits in every mapping, Space+left pans, a middle double-click goes home, the arrows turn the view 15° (Shift: 90°), Ctrl+arrows pan. Two-finger trackpad scroll pans, Shift+two fingers orbits, pinch zooms. Other keys: F fits, P toggles perspective, Ctrl+I imports, Tab switches edit mode, T / R / S choose Move / Rotate / Scale, Esc puts the tool away, Delete (or Backspace) removes what is selected in the current mode. Shortcuts stand down only while a text field is being typed in: a button keeps egui's focus after a click, and Tab takes the focus off it.

## Chrome
Ported from SolidCraft, reduced. `toolbar::app_bar` is the always-dark application bar: brand, Home (iso view), File (Import STL…, Clear Scene, Quit), Preferences and the version. `toolbar::ribbon` is under it: the workspace card (Prepare, the only one yet), a tab per edit mode, and the tab's groups of 60 pt buttons (icon over label, group name underneath; View's name opens the standard views). A button whose tool is in use shows pressed; one that can't act is dimmed and its tooltip says why. Icons (`icons.rs`) are SolidCraft's SVG masters in `crates/ui-egui/icons`, parsed once, rasterised with tiny-skia at the size shown and cached as textures; their colours are palette roles resolved for the light or dark theme. Tool dialogs are `ui_kit::Card`s: a 40 pt header (icon, title, close), a padded body of `ui_kit::row`s (70 pt label column, 28 pt controls) and a 48 pt footer with Cancel and OK; Enter is OK, Esc Cancel; the card stays inside the viewport. Colours and sizes come from `theme::Tokens` and `theme::metrics`; `theme::apply` gives egui's own widgets the same style for both themes.

## Text and preferences
Every word on screen is a message in `assets/locales/<language>.json` (English only for now), keyed by a stable id and embedded at build time; `i18n::tr`, `i18n::format` (named placeholders) and `i18n::count` (`.one`/`.other`) look it up in the language the `language` preference names (`system` follows the OS). Tests check that every id-shaped literal in the UI's source, every command label (`cmd.<id>`) and every complete language's message is in place; `cargo xtask lists` keeps the catalogs sorted and `cargo xtask i18n` reports gaps. Settings are rows of the `preference_settings!` table in `prefs.rs` (field, default, page, control), which generates `Prefs`, its defaults, each page's Restore defaults and the Preferences window's rows (`preferences.rs`, a `ui_kit::Card` with a page list).

## Edit modes
A left click in the viewport acts on what the edit mode says (`EditMode`, toggled by Tab or the toolbar). In **Object edition** (`objects::handle`) a click on a model runs `model.select` (Shift toggles), elsewhere clears; selected models draw in `model_selected`. In **Support edition** clicks place and select tips (below). Both share `supports::picking_press`, so a navigating press, or one on the gizmo, picks nothing.

## Placement
A model keeps its mesh and BVH as imported and a `Placement`: the imported bounds' centre (`Model::pivot`) moved to `position`, after `scale` along the model's own axes and `rotation` degrees about X, then Y, then Z. `Model::transform` is that as an `Affine3`; nothing is rebuilt when it changes: the GPU draws the imported mesh through it, `Scene::ray_hit` sends the ray into model coordinates through its inverse, and only the scene bounds are recomputed from the vertices. Supports keep their tip and normal in model coordinates too, so they follow any placement exactly. `Scene::revision` changes with any of it and invalidates the pointer's pick cache.

## Move, Rotate, Scale
T, R, S choose a tool (`transform.rs`) on the selection in Object edition: a gizmo and a panel, both live. The gizmo (`gizmo.rs`, ported from SolidCraft) is a constant size on screen, at a lone model's pivot or at the centre of several: arrows, plane squares and a centre ball (free drag in the view plane) for Move; rings for Rotate; square-ended handles along the model's own axes, the ball for all of them, for Scale. Distances snap to round steps at the zoom (`snap_step`), angles to 5°, scale factors to 1 %; Ctrl or Alt drags free. A press on a handle is hit-tested before `navigation::handle`, which then leaves it alone (`ResinApp::gizmo_grab`). A drag reports its change since it began, and each frame runs one `model.place` built from the placements it began with, so nothing piles up. The panel, a `ui_kit::Card`, shows a lone model's placement, measured from the import (position, rotation, scale as % or target mm, *Uniform*), applies each edit at once, and has Reset buttons; OK keeps the changes and puts the tool away, Cancel puts back the placements the selection had when the tool opened. With several models selected, the gizmo acts on all of them: rotating turns them about their joint centre, scaling scales each about its own pivot.

## Support tips
In Support edition. Every model keeps a `MeshIndex` (BVH over its triangles) built at import; `Scene::ray_hit` picks through it, for navigation and supports alike. `supports::handle` runs after `navigation::handle`: with no button down, the pointer's pick (cached until the pointer, camera or scene changes) shows a preview tip on the surface; a plain left press on the surface runs `support.add`, on a tip `support.select` (Shift toggles), elsewhere clears the selection. It acts on the press, picked where it happened: egui turns a click whose pointer moves over 6 px before the release into a drag, which lost tips placed with a moving hand. A press the mouse mapping navigates with (Ctrl+left, Space, a preset's left binding) places nothing; Delete or Backspace runs `support.delete`. Tips are `Support::TIP_DIAMETER` (0.5 mm) spheres centred on the surface; preview, placed and selected tips take `support_preview`, `support` and `support_selected` from the theme.
