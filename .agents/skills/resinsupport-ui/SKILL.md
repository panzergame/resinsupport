---
name: resinsupport-ui
description: "Use when adding or changing a ResinSupport dialog, ribbon button, panel, overlay, viewport drawing, camera input, gizmo, icon or colour in crates/ui-egui, giving a card presets, or working with the FigCraft design file. Covers the design system ported from SolidCraft: theme tokens and the design file that owns them, the UI kit's card and rows, the preset row, the ribbon, icons and the one camera path."
---

# Dialogs and UI

The UI is thin: it reads the session and acts through `app.run(id, params)` (`.agents/skills/resinsupport-commands/SKILL.md`). Its look is SolidCraft's, ported reduced; when a piece is missing, port it from `solidcraft:crates/ui-kit/src/` and `solidcraft:docs/ui-kit.md` at `a34756830`, never later (the Brand and Interface Licence, `.agents/skills/resinsupport-ground-rules/SKILL.md`), rather than inventing one.

## Colours, sizes, style
- Colours come from `theme::Tokens` (`crates/ui-egui/src/theme.rs`), in light and dark pairs; sizes and spacing from `theme::metrics`. No literal colours or magic sizes in feature code: add a token.
- The chrome's colours, the metrics and the icon roles' colours are owned by the design file (below): their values sit between `// design-tokens:` markers and are never edited by hand.
- `theme::apply` gives egui's own widgets the same style in both themes, once at start-up.

## The design file
`design/resinsupport.figcraft` is the UI in FigCraft: its variables are the tokens, its `icon/<name>` components the icon masters, its other components the kit's controls as they paint, and its screens every edition, card and menu in both themes, built from instances of those components.
- It owns the token values: `design/tokens.json` is its `variable.export`. `cargo xtask design-pull` exports the tokens and the changed icons from it, then writes the tokens into the code's marked regions (the chrome in `theme.rs`, the roles in `icons.rs`). `cargo xtask design-tokens` does only that last step; `cargo xtask ci` runs its `--check`.
- A name the code uses that the design file lacks refuses the whole write: give the design file the variable, or remove its use from the code. A variable the code does not use yet is only reported: add its field or constant (inside the region), then pull again.
- A new token: add it in the design file (Colour: Light and Dark; Metrics; Icon: Light, Dark and the colour masters write for it in Source), add its field or constant inside the region with any value, then pull. The viewport's colours stay outside the regions: the GPU draws them, not the design file.
- Icons: an `icon/<name>` component edited in FigCraft comes back as `crates/ui-egui/icons/<name>.svg` on a pull; only one whose export changed since `design/icons.lock` is rewritten. Its colours must be bound to the Icon variables; FigCraft stretches dash patterns to fit their path, so a dashed icon edited there comes back with its dashes slightly longer. A new icon component also needs its `MASTERS` row and an `ATTRIBUTION.md` row.
- Layouts are a reference, not a source: a change to a component or screen is ported to the kit by hand. Mark the frame ready for dev in FigCraft first; its compare view (`dev.compare`) then lists each property changed.
- `cargo xtask design-export` builds the file from the code, once (`--force` rebuilds it, losing its edits): it runs `design_capture` (`crates/ui-egui/src/design_capture.rs`, an ignored test; the same screens are swept for accessibility on every test run), which drives the app headlessly into each screen in both themes and records what each layer painted and which kit control painted it (`ui_kit::mark`, nothing outside tests). A control new to the kit gets a `mark` call; a new card or menu gets a screen in `screens()`.
- FigCraft runs without a window: `FIGCRAFT_CLI` names `figcraft-cli` or the FigCraft AppImage. Only the export and the pull need it; `cargo xtask ci` does not.

## Dialogs are cards
- Every dialog is a `ui_kit::Card` (`crates/ui-egui/src/ui_kit.rs`): a 40 pt header (icon, title, close), a padded body that scrolls when the viewport is too short, a 48 pt footer. Do not build a raw `egui::Window`, frame or shadow in a feature module.
- Footers: `Buttons::OkCancel` for a tool (OK keeps, Cancel puts back what the tool opened with; Enter is OK, Esc Cancel) or `Buttons::ResetClose` for settings that apply as they change.
- The body is rows (`ui_kit/layout.rs`): `row` puts a label (in `dialog_label`, cut short to fit, whole on hover) in a label column and a 28 pt control after an 8 pt gap. The column is 70 pt, 104 pt with a 10 pt gap in a card wider than 312 pt, and every row of a card shares it, widened for the card's longest label up to 1.6 times, never past half the row. `sized_row` is a taller row, `value_row` read-only text that wraps, `action_row` a one-shot button across the control column (Restore defaults, Reset, Delete), `status_line` a wrapping warning or error, `hint` dim text, `caption` and `warning_caption` a line under a row's control.
- Controls (`ui_kit/fields.rs`, `checks.rs`, `status.rs`): `number_field` (and `number_fields` side by side), `text_field`, `multiline_text_field`, `dropdown`, `dropdown_menu` with `dropdown_item`, `choice_row`, `slider`, `checkbox`, `check_row`, `mixed_check_row`, `segmented`, `progress`, `icon_button`. `ui_kit::section` folds rows under a heading, as SolidCraft's cards do (a preset's settings, then Options). A card stays inside the viewport (`bounds`).
- Menus are `ui_kit::menu_list` of `MenuItem`s and `menu_separator`s; a list of things (the models) is `ui_kit::browser_row`s on a `Buttons::Fold` card, `ui_kit::rename_row` while a name is typed; Preferences' pages are `ui_kit::page_list`.
- Feature modules never draw egui's own controls (`ComboBox`, `Button`, `Slider`, `Checkbox`, `TextEdit`, `DragValue`, `ProgressBar`, `.checkbox`, `.button`, `.selectable_label`, …): `raw_widgets_stay_in_the_kit` fails on them. A control the kit lacks is ported into it.
- Tooltips go through `ui_kit::Tip` (`.tip(text)` while the control can act, `.disabled_tip(why)` while it is dimmed, `.tip_always(text)` for both); egui's own `on_hover_*` and `on_disabled_hover_*` are not called outside the kit, so how every tooltip looks and what it gives a screen reader change in one place (`crates/ui-egui/src/ui_kit/tip.rs`). `raw_widgets_stay_in_the_kit` fails on them too.
- Tool values apply live; a drag runs one command per frame from the values it began with.
- One tool at a time: a tool (its card, gizmo or gesture in the view) is an `ActiveTool` variant in `ResinApp::tool`, so opening one puts away the one open, keeping what it did, and leaving its edition puts it away. An edition's settings card (support, print, lift) is not a tool and stays beside it.

## Presets on a card
User presets are generic (`docs/architecture.md`, Presets): one library per kind, one state per row. Label presets are the worked example (`crates/engine/src/label_presets.rs`, `crates/engine/src/cmd/label_preset.rs`, the label card in `crates/ui-egui/src/object_label.rs`). For a new kind:
1. Engine, the settings: a `PresetKind` (`KIND`, the `Settings` type, `check`) beside the settings it holds, and a field of `PresetLibraries` (`crates/engine/src/presets.rs`) read in its `from_stored` and checked in its `validate`.
2. Engine, the followers: what follows a preset gets the preset's name to store (in `Scene`, so a project format bump, `.agents/skills/resinsupport-file-formats/SKILL.md`; on the session, beside the settings). A `Follow` impl in `crates/engine/src/cmd/` gives the target (`TARGET_PARAMS`), its settings, `follow` and `relink`; one followed from two places takes an enum `Target`, never a second kind.
3. Engine, the commands: `<area>.preset.{delete,list,rename,save}` in `cmd.rs`, each `preset::<name>::<Kind>` with its own documented parameters, and following a preset by name in the command that edits the settings, which calls `presets::edited` with what it changed, for the live edit.
4. UI: a `ui_kit::PresetRow` on the card with a row id of its own (two rows of one kind shown together take two ids), its state from `app.preset_rows.state(row)`, forgotten in the card's `close`; map each `PresetAction` to the kind's commands. The row's words are `ui.preset.*`; the card adds its row label and tooltip, no hint under the row (`.agents/skills/resinsupport-ui-text/SKILL.md`).
5. Tests: a `sample` row and junk parameters for each command (`crates/engine/src/tests.rs`), an engine test of save, edit, rename and delete with their undo steps, and a card test driving the row.
6. Docs: the command table, the Presets section's list of kinds, `docs/features.md`.

## Ribbon and application bar
- `crates/ui-egui/src/toolbar.rs`: the application bar, then the ribbon's workspace card, a tab per edit mode and the tab's groups. A new button is a `Cmd` variant with a label id, an icon, a `state` (shortcut, the catalog id of why it can't act, pressed) and a `run`, placed in `groups`.
- A button that can't act is dimmed and its tooltip says why; a tool in use shows pressed.

## Icons
- `crates/ui-egui/src/icons.rs` draws the SVG masters in `crates/ui-egui/icons`, SolidCraft's artwork, copied unchanged. Colours are palette roles resolved for the theme, never caller tints.
- A new icon: copy the closest SolidCraft master, add it to `MASTERS` and its row to `ATTRIBUTION.md`. `every_master_parses_and_draws_in_both_themes` checks it. An icon drawn in the design file comes back through `cargo xtask design-pull` (The design file, above).

## One camera path and the gizmo
- All pointer, wheel and trackpad camera input goes through `navigation::handle` (`crates/ui-egui/src/navigation.rs`); never read drags or the wheel for the camera elsewhere.
- Picking in the viewport goes through `supports::picking_press`, so a navigating press or one on the gizmo picks nothing. The gizmo is `crates/ui-egui/src/gizmo.rs`; a press on a handle is claimed before navigation (`ResinApp::gizmo_grab`).

## Accessibility
- A card works by keyboard alone, with no trap: Tab moves egui's focus (`request_focus`), not only a painted highlight; Enter and Esc end it (`.agents/skills/resinsupport-shared-controls/SKILL.md`).
- Every control a screen reader can click has a role and a name. A kit control in a `row` is named by the row's label; one whose caption is painted calls `layout::named`; a control drawn by hand gives `response.widget_info(…)` with its words; a card is a dialog named by its title. `every_clickable_control_has_a_name` sweeps every screen of `design_capture` with accesskit on, and names the kit mark of what it finds. A new screen of the design file is swept too.
- A tip (`ui_kit::Tip`) is its control's accessible description, and its name when it has none: start the visible label's words first.
- Text has 4.5:1 contrast with its background and controls 3:1, in both themes: `text_and_controls_meet_contrast_in_both_themes` (`crates/ui-egui/src/theme.rs`) checks the pairs; a new ink or surface adds its own. Change a value in the design file, then `cargo xtask design-pull`. Colour is never the only signal (a warning also has its icon or word).

## Text and settings
- Every word on screen is a catalog message: `.agents/skills/resinsupport-ui-text/SKILL.md`.
- A change to what users see or do by default: `.agents/skills/resinsupport-preferences/SKILL.md`.

## Look at the result
Run the app, drive it and read the screenshot: `.agents/skills/resinsupport-running/SKILL.md`.
