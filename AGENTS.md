# ResinSupport: instructions for agents

ResinSupport is a pure-Rust, multiplatform app that prepares STL models for resin (MSLA) printers: place, support, slice. Its development philosophy follows SolidCraft's: layered crates, everything is a command, never crash, and every rule has a check that enforces it. Architecture: `docs/architecture.md`. Work: `ROADMAP.md`. Each rule below is one line; its skill has the how and the why.

## Hard rules
- Other slicers: GPLv3-compatible open-source ones may be ported, nothing from proprietary ones (Chitubox, Lychee); printer formats come from public documentation or such a port; every port (SolidCraft's too) keeps a header and an `ATTRIBUTION.md` row. `skills/resinsupport-ground-rules/SKILL.md`.
- GPL-3.0-or-later: code taken in must be GPLv3-compatible (no GPL-2.0-only; AGPL-3.0 UVtools allowed, other AGPL only on the owner's say), and keeps a header and an `ATTRIBUTION.md` row. `skills/resinsupport-ground-rules/SKILL.md`.
- Never crash: no unwrap/expect/panic/unreachable/todo in non-test code, no `unsafe`; files and command parameters are hostile input. `cargo xtask crash-guards`, clippy `-D warnings`, `hostile_params_never_panic`: `skills/resinsupport-crash-safety/SKILL.md`.
- Everything is a command, through `Session::execute(id, params)`; ids are sorted, dotted, lower-case; programmatic calls never open dialogs. `skills/resinsupport-commands/SKILL.md`.
- Every scene change is undoable: state the user edits lives in `engine::Scene`, changed only through its methods (which bump the revision); a gesture is one step (`Session::begin_step`); a tool's Cancel leaves the undo history alone. `every_scene_change_is_undoable`: `skills/resinsupport-commands/SKILL.md`.
- Layering: `geom` (L0) → `render` (L1) → `slice` (L2) → `io` (L3) → `engine` (L4) → `ui-egui` (L5) → apps; nothing below L5 uses egui, eframe, wgpu, winit or rfd. `cargo xtask layers`.
- Application code is Rust only and wasm-friendly: no `std::fs` outside `io` and the apps. `skills/resinsupport-performance/SKILL.md`.
- The UI is SolidCraft's design system: colours from `theme::Tokens`, dialogs are `ui_kit::Card`s, one camera path `navigation::handle`. `skills/resinsupport-ui/SKILL.md`.
- Every word on screen is a catalog message with a stable id. `cargo xtask lists`, `cargo xtask i18n`: `skills/resinsupport-ui-text/SKILL.md`.
- A changed default keeps the old behaviour behind a preference, and the commit says `Changes a default:`. `skills/resinsupport-preferences/SKILL.md`.
- Model units are millimetres, Z up.
- Run `cargo xtask ci` before every commit. `cargo xtask skills` keeps this file and `skills/` honest.

## Where things go
| Working on | Read |
|---|---|
| Licences, assets, porting, wording | `skills/resinsupport-ground-rules/SKILL.md` |
| Non-test Rust, hostile input, a crash | `skills/resinsupport-crash-safety/SKILL.md` |
| A command, a crate boundary | `skills/resinsupport-commands/SKILL.md` |
| A dialog, ribbon button, panel, camera, gizmo, icon, colour | `skills/resinsupport-ui/SKILL.md` |
| Labels, messages, languages | `skills/resinsupport-ui-text/SKILL.md` |
| Preferences, a changed default | `skills/resinsupport-preferences/SKILL.md` |
| Writing or running tests | `skills/resinsupport-tests/SKILL.md` |
| Slow work, the window thread, wasm | `skills/resinsupport-performance/SKILL.md` |
| Running the app, screenshots | `skills/resinsupport-running/SKILL.md` |
| Import, export, printer formats | `skills/resinsupport-file-formats/SKILL.md` |
| Camera maths (keep in step with SolidCraft) | `crates/render/src/camera.rs` |
