---
name: resinsupport-ui
description: "Use when adding or changing a ResinSupport dialog, ribbon button, panel, overlay, viewport drawing, camera input, gizmo, icon or colour in crates/ui-egui, or giving a card presets. Covers the design system ported from SolidCraft: theme tokens, the UI kit's card and rows, the preset row, the ribbon, icons and the one camera path."
---

# Dialogs and UI

The UI is thin: it reads the session and acts through `app.run(id, params)` (`skills/resinsupport-commands/SKILL.md`). Its look is SolidCraft's, ported reduced; when a piece is missing, port it from `solidcraft:crates/ui-kit/src/` and `solidcraft:docs/ui-kit.md` at `a34756830`, never later (the Brand and Interface Licence, `skills/resinsupport-ground-rules/SKILL.md`), rather than inventing one.

## Colours, sizes, style
- Colours come from `theme::Tokens` (`crates/ui-egui/src/theme.rs`), in light and dark pairs; sizes and spacing from `theme::metrics`. No literal colours or magic sizes in feature code: add a token.
- `theme::apply` gives egui's own widgets the same style in both themes, once at start-up.

## Dialogs are cards
- Every dialog is a `ui_kit::Card` (`crates/ui-egui/src/ui_kit.rs`): a 40 pt header (icon, title, close), a padded body that scrolls when the viewport is too short, a 48 pt footer. Do not build a raw `egui::Window`, frame or shadow in a feature module.
- Footers: `Buttons::OkCancel` for a tool (OK keeps, Cancel puts back what the tool opened with; Enter is OK, Esc Cancel) or `Buttons::ResetClose` for settings that apply as they change.
- The body is rows (`ui_kit/layout.rs`): `row` puts a label (in `dialog_label`, cut short to fit, whole on hover) in a label column and a 28 pt control after an 8 pt gap. The column is 70 pt, 104 pt with a 10 pt gap in a card wider than 312 pt, and every row of a card shares it, widened for the card's longest label up to 1.6 times, never past half the row. `sized_row` is a taller row, `value_row` read-only text that wraps, `action_row` a one-shot button across the control column (Restore defaults, Reset, Delete), `status_line` a wrapping warning or error, `hint` dim text, `caption` and `warning_caption` a line under a row's control.
- Controls (`ui_kit/fields.rs`, `checks.rs`, `status.rs`): `number_field` (and `number_fields` side by side), `text_field`, `multiline_text_field`, `dropdown`, `dropdown_menu` with `dropdown_item`, `choice_row`, `slider`, `checkbox`, `check_row`, `mixed_check_row`, `segmented`, `progress`, `icon_button`. `ui_kit::section` folds rows under a heading, as SolidCraft's cards do (a preset's settings, then Options). A card stays inside the viewport (`bounds`).
- Menus are `ui_kit::menu_list` of `MenuItem`s and `menu_separator`s; a list of things (the models) is `ui_kit::list_item`s; Preferences' pages are `ui_kit::page_list`.
- Feature modules never draw egui's own controls (`ComboBox`, `Button`, `Slider`, `Checkbox`, `TextEdit`, `DragValue`, `ProgressBar`, `.checkbox`, `.button`, `.selectable_label`, …): `raw_widgets_stay_in_the_kit` fails on them. A control the kit lacks is ported into it.
- Tool values apply live; a drag runs one command per frame from the values it began with.
- One tool at a time: a tool (its card, gizmo or gesture in the view) is an `ActiveTool` variant in `ResinApp::tool`, so opening one puts away the one open, keeping what it did, and leaving its edition puts it away. An edition's settings card (support, print, lift) is not a tool and stays beside it.

## Presets on a card
User presets are generic (`docs/architecture.md`, Presets): one library per kind, one state per row. Label presets are the worked example (`crates/engine/src/label_presets.rs`, `crates/engine/src/cmd/label_preset.rs`, the label card in `crates/ui-egui/src/object_label.rs`). For a new kind:
1. Engine, the settings: a `PresetKind` (`KIND`, the `Settings` type, `check`) beside the settings it holds, and a field of `PresetLibraries` (`crates/engine/src/presets.rs`) read in its `from_stored` and checked in its `validate`.
2. Engine, the followers: what follows a preset gets the preset's name to store (in `Scene`, so a project format bump, `skills/resinsupport-file-formats/SKILL.md`; on the session, beside the settings). A `Follow` impl in `crates/engine/src/cmd/` gives the target (`TARGET_PARAMS`), its settings, `follow` and `relink`; one followed from two places takes an enum `Target`, never a second kind.
3. Engine, the commands: `<area>.preset.{delete,list,rename,save}` in `cmd.rs`, each `preset::<name>::<Kind>` with its own documented parameters, and following a preset by name in the command that edits the settings, which calls `presets::edited` with what it changed, for the live edit.
4. UI: a `ui_kit::PresetRow` on the card with a row id of its own (two rows of one kind shown together take two ids), its state from `app.preset_rows.state(row)`, forgotten in the card's `close`; map each `PresetAction` to the kind's commands. The row's words are `ui.preset.*`; the card adds its row label and tooltip, no hint under the row (`skills/resinsupport-ui-text/SKILL.md`).
5. Tests: a `sample` row and junk parameters for each command (`crates/engine/src/tests.rs`), an engine test of save, edit, rename and delete with their undo steps, and a card test driving the row.
6. Docs: the command table, the Presets section's list of kinds, `docs/features.md`.

## Ribbon and application bar
- `crates/ui-egui/src/toolbar.rs`: the application bar, then the ribbon's workspace card, a tab per edit mode and the tab's groups. A new button is a `Cmd` variant with a label id, an icon, a `state` (shortcut, the catalog id of why it can't act, pressed) and a `run`, placed in `groups`.
- A button that can't act is dimmed and its tooltip says why; a tool in use shows pressed.

## Icons
- `crates/ui-egui/src/icons.rs` draws the SVG masters in `crates/ui-egui/icons`, SolidCraft's artwork, copied unchanged. Colours are palette roles resolved for the theme, never caller tints.
- A new icon: copy the closest SolidCraft master, add it to `MASTERS` and its row to `ATTRIBUTION.md`. `every_master_parses_and_draws_in_both_themes` checks it.

## One camera path and the gizmo
- All pointer, wheel and trackpad camera input goes through `navigation::handle` (`crates/ui-egui/src/navigation.rs`); never read drags or the wheel for the camera elsewhere.
- Picking in the viewport goes through `supports::picking_press`, so a navigating press or one on the gizmo picks nothing. The gizmo is `crates/ui-egui/src/gizmo.rs`; a press on a handle is claimed before navigation (`ResinApp::gizmo_grab`).

## Text and settings
- Every word on screen is a catalog message: `skills/resinsupport-ui-text/SKILL.md`.
- A change to what users see or do by default: `skills/resinsupport-preferences/SKILL.md`.

## Look at the result
Run the app, drive it and read the screenshot: `skills/resinsupport-running/SKILL.md`.
