# ResinSupport: instructions for agents

ResinSupport is a pure-Rust, multiplatform app that prepares STL models for resin (MSLA) printers: place, support, slice. Its development philosophy follows SolidCraft's: layered crates, everything is a command, never crash, and every rule has a check that enforces it. Architecture: `docs/architecture.md`. Work: `ROADMAP.md`. Each rule below is one line; its skill has the how and the why.

## Hard rules
- Other slicers: GPLv3-compatible open-source ones may be ported, nothing from proprietary ones (Chitubox, Lychee); printer formats come from public documentation or such a port; every port (SolidCraft's too) keeps a header and an `ATTRIBUTION.md` row. `.agents/skills/resinsupport-ground-rules/SKILL.md`.
- GPL-3.0-or-later: code taken in must be GPLv3-compatible (no GPL-2.0-only; AGPL-3.0 allowed), and keeps a header and an `ATTRIBUTION.md` row. `.agents/skills/resinsupport-ground-rules/SKILL.md`.
- Never crash: no unwrap/expect/panic/unreachable/todo in non-test code, no `unsafe`; files and command parameters are hostile input. `cargo xtask crash-guards`, clippy `-D warnings`, `hostile_params_never_panic`: `.agents/skills/resinsupport-crash-safety/SKILL.md`.
- Everything is a command, through `Session::execute(id, params)`; ids are sorted, dotted, lower-case; programmatic calls never open dialogs. `.agents/skills/resinsupport-commands/SKILL.md`.
- Every scene change is undoable: state the user edits lives in `engine::Scene`, changed only through its methods (which bump the revision); a gesture is one step (`Session::begin_step`); a tool's Cancel leaves the undo history alone. `every_scene_change_is_undoable`: `.agents/skills/resinsupport-commands/SKILL.md`.
- Layering: `geom` (L0) → `render` (L1) → `slice` (L2) → `io` (L3) → `engine` (L4) → `ui-egui` (L5) → apps; nothing below L5 uses egui, eframe, wgpu, winit or rfd. `cargo xtask layers`.
- Application code is Rust only and wasm-friendly: no `std::fs` outside `io` and the apps. `.agents/skills/resinsupport-performance/SKILL.md`.
- The UI is SolidCraft's design system: colours from `theme::Tokens` (their values owned by `design/resinsupport.figcraft`, `cargo xtask design-tokens`), dialogs are `ui_kit::Card`s, controls and tooltips (`ui_kit::Tip`) from `ui_kit` only, one camera path `navigation::handle`, the same selection, keys and tool lifecycle everywhere, every control named and 4.5:1 text (`every_clickable_control_has_a_name`, `text_and_controls_meet_contrast_in_both_themes`). `raw_widgets_stay_in_the_kit`: `.agents/skills/resinsupport-ui/SKILL.md`.
- Every word on screen is a catalog message with a stable id. `cargo xtask lists`, `cargo xtask i18n`: `.agents/skills/resinsupport-ui-text/SKILL.md`.
- A changed default: old projects keep their result through a format legacy value, a choice switched while working goes on the tool's card (not Preferences), and the commit says `Changes a default:`. `.agents/skills/resinsupport-preferences/SKILL.md`.
- Never delete, weaken or `#[ignore]` a test to pass a check; a changed expectation carries its derivation and `Test change:`. `.agents/skills/resinsupport-tests/SKILL.md`.
- Model units are millimetres, Z up.
- `cargo xtask fast` before each commit, `cargo xtask ci` before calling work done. `cargo xtask skills` checks this router and `.agents/skills/` (`.claude/skills` links there).

## Where things go
| Working on | Read |
|---|---|
| What the app already does, a feature prompt | `docs/features.md` |
| Licences, assets, porting, wording | `.agents/skills/resinsupport-ground-rules/SKILL.md` |
| Non-test Rust, hostile input, a crash | `.agents/skills/resinsupport-crash-safety/SKILL.md` |
| A command, a crate boundary | `.agents/skills/resinsupport-commands/SKILL.md` |
| A dialog, ribbon button, panel, camera, gizmo, icon, colour | `.agents/skills/resinsupport-ui/SKILL.md` |
| Presets on a card (labels', and later supports', slicing's) | `.agents/skills/resinsupport-ui/SKILL.md`, Presets on a card |
| The design file (FigCraft), a token's value, an icon's artwork | `.agents/skills/resinsupport-ui/SKILL.md`, The design file |
| Labels, messages, languages | `.agents/skills/resinsupport-ui-text/SKILL.md` |
| Selection, picking, keys, a tool's start and finish | `.agents/skills/resinsupport-shared-controls/SKILL.md` |
| A changed default, a new option or setting | `.agents/skills/resinsupport-preferences/SKILL.md` |
| Writing or running tests | `.agents/skills/resinsupport-tests/SKILL.md` |
| Slow work, the window thread, wasm | `.agents/skills/resinsupport-performance/SKILL.md` |
| Running the app, screenshots | `.agents/skills/resinsupport-running/SKILL.md` |
| Import, export, printer formats | `.agents/skills/resinsupport-file-formats/SKILL.md` |
| A bug only on some OS, GPU, driver or scaling | `.agents/skills/resinsupport-repro-first/SKILL.md` |
| A change end to end, nobody watching | `.agents/skills/resinsupport-full-stop/SKILL.md` |
| A commit message, a pull request | `.agents/skills/resinsupport-file-pr/SKILL.md` |
| Camera maths (keep in step with SolidCraft) | `crates/render/src/camera.rs` |
