---
name: resinsupport-ui
description: "Use when adding or changing a ResinSupport dialog, ribbon button, panel, overlay, viewport drawing, camera input, gizmo, icon or colour in crates/ui-egui. Covers the design system ported from SolidCraft: theme tokens, the UI kit's card and rows, the ribbon, icons and the one camera path."
---

# Dialogs and UI

The UI is thin: it reads the session and acts through `app.run(id, params)` (`skills/resinsupport-commands/SKILL.md`). Its look is SolidCraft's, ported reduced; when a piece is missing, port it from `solidcraft:crates/ui-egui/src/ui_kit/` and `solidcraft:docs/ui-kit.md` rather than inventing one (`skills/resinsupport-ground-rules/SKILL.md`).

## Colours, sizes, style
- Colours come from `theme::Tokens` (`crates/ui-egui/src/theme.rs`), in light and dark pairs; sizes and spacing from `theme::metrics`. No literal colours or magic sizes in feature code: add a token.
- `theme::apply` gives egui's own widgets the same style in both themes, once at start-up.

## Dialogs are cards
- Every dialog is a `ui_kit::Card` (`crates/ui-egui/src/ui_kit.rs`): a 40 pt header (icon, title, close), a padded body, a 48 pt footer. Do not build a raw `egui::Window`, frame or shadow in a feature module.
- Footers: `Buttons::OkCancel` for a tool (OK keeps, Cancel puts back what the tool opened with; Enter is OK, Esc Cancel) or `Buttons::ResetClose` for settings that apply as they change.
- The body uses `ui_kit::row` (70 pt label column, 28 pt controls), `wide_row`, `check_row`, `choice_row`, `hint` and `button`. A card stays inside the viewport (`bounds`).
- Tool values apply live; a drag runs one command per frame from the values it began with.

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
