# Attribution

Code ported from other projects, with its licence. Every ported file names its source in a header comment.

| Here | Source | Licence |
|---|---|---|
| `crates/geom/src/vec.rs`, `crates/geom/src/aabb.rs` | SolidCraft `crates/geom/src/vec.rs`, `crates/geom/src/mesh.rs` at `4bdffef0` (https://github.com/bherbruck/solidcraft) | MIT OR Apache-2.0, © SolidCraft contributors |
| `crates/geom/src/bvh.rs` (reduced to ray queries) | SolidCraft `crates/geom/src/bvh.rs` at `4bdffef0` | MIT OR Apache-2.0, © SolidCraft contributors |
| `crates/geom/src/ray.rs` | SolidCraft `crates/geom/src/mesh.rs` (`RayFrame`, watertight ray/triangle test) at `4bdffef0` | MIT OR Apache-2.0, © SolidCraft contributors |
| `crates/render/src/camera.rs` | SolidCraft `crates/render/src/camera.rs` at `4bdffef0` | MIT OR Apache-2.0, © SolidCraft contributors |
| `crates/ui-egui/src/prefs.rs` (mouse mappings) | SolidCraft `crates/ui-egui/src/prefs.rs` at `4bdffef0` | MIT OR Apache-2.0, © SolidCraft contributors |
| `crates/ui-egui/src/navigation.rs` (reduced) | SolidCraft `crates/ui-egui/src/navigation.rs` at `4bdffef0` | MIT OR Apache-2.0, © SolidCraft contributors |
| `crates/ui-egui/src/gizmo.rs` (move/rotate triad, `snap_step`; scale handles are ours) | SolidCraft `crates/ui-egui/src/gizmo.rs`, `crates/ui-egui/src/canvas.rs` (`snap_step`) at `4bdffef0` | MIT OR Apache-2.0, © SolidCraft contributors |
| `crates/ui-egui/src/icons.rs` (reduced: no accents, colour-vision variants or aliases) | SolidCraft `crates/ui-egui/src/icon_svg.rs`, `crates/ui-egui/src/icons.rs`, palette roles from `assets/icons/ui/design/palette.toml` at `4bdffef0` | MIT OR Apache-2.0, © SolidCraft contributors |
| `crates/ui-egui/icons/*.svg` (artwork, unchanged) | SolidCraft `assets/icons/ui/design/svg/{flat,model,workspace}/` at `4bdffef0` | MIT OR Apache-2.0, © SolidCraft contributors |
| `crates/engine/src/cmd/edit.rs` (undo, redo), the undo step in `Session::execute` | SolidCraft `crates/engine/src/cmd/edit.rs`, `crates/engine/src/lib.rs` (`Session::execute`, `MAX_UNDO`) at `4bdffef0` | MIT OR Apache-2.0, © SolidCraft contributors |
| `crates/ui-egui/src/ui_kit.rs` (feature card, form rows) | SolidCraft `crates/ui-egui/src/ui_kit/panel.rs`, `actions.rs`, `layout.rs` at `4bdffef0` | MIT OR Apache-2.0, © SolidCraft contributors |
| `crates/ui-egui/src/toolbar.rs` (application bar, ribbon) | SolidCraft `crates/ui-egui/src/toolbar.rs` at `4bdffef0` | MIT OR Apache-2.0, © SolidCraft contributors |
| `crates/ui-egui/src/theme.rs` (chrome colours, `metrics`, `visuals`, `apply`) | SolidCraft `assets/styles/ui/palettes.rs`, `crates/ui-egui/src/theme.rs` at `4bdffef0` | MIT OR Apache-2.0, © SolidCraft contributors |
| `crates/ui-egui/src/ui_kit/choice.rs` (`segmented`, reduced), `crates/ui-egui/src/ui_kit/figure.rs` (`figure_rows`, adapted to number fields), `crates/ui-egui/src/ui_kit/confirm.rs` (`Confirm`, reduced) | SolidCraft `crates/ui-egui/src/ui_kit/choice.rs`, `crates/ui-egui/src/ui_kit/layout.rs`, `crates/ui-egui/src/ui_kit/confirm.rs` at `4bdffef0` | MIT OR Apache-2.0, © SolidCraft contributors |
| `prefs::lenient` in `crates/ui-egui/src/prefs.rs` (reduced: no migrations), tolerant field reading in `crates/engine/src/support_settings.rs` (pattern of `hole_presets::Library`'s `Deserialize`) | SolidCraft `crates/ui-egui/src/prefs.rs`, `crates/ui-egui/src/hole_presets.rs` at `4bdffef0` | MIT OR Apache-2.0, © SolidCraft contributors |
| `crates/ui-egui/src/prefs_writer.rs` (`PrefsWriter`, `changes_settings` and their tests) | SolidCraft `apps/solidcraft/src/main.rs` at `4bdffef0` | MIT OR Apache-2.0, © SolidCraft contributors |
| `skills/` (layout, `AGENTS.md` routing, adapted wording), `crates/ui-egui/src/i18n.rs` (reduced), `preference_settings!` in `crates/ui-egui/src/prefs.rs` | SolidCraft `skills/`, `AGENTS.md`, `crates/ui-egui/src/i18n.rs`, `crates/ui-egui/src/prefs.rs` at `4bdffef0` | MIT OR Apache-2.0, © SolidCraft contributors |
