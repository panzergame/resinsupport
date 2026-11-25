---
name: resinsupport-shared-controls
description: "Use when adding or changing how a ResinSupport tool, edition or card handles selection, picking, keys, Esc, Delete, undo or the tool's start and finish, or when a change wants a new option. Covers the shared controls and where a choice people switch while they work goes."
---

# Shared controls

Every edition and tool behaves like the rest of the app, so people learn the controls once. A control that works differently in one place is a bug, however good the idea.

## Use the shared pieces
- **Picking and selection:** a press in the viewport goes through `supports::picking_press` (`crates/ui-egui/src/supports.rs`), so a navigating press or one on the gizmo picks nothing; box selection (window and crossing) is `crates/ui-egui/src/box_select.rs`. Never a second picker for one tool.
- **Keys:** the app's keys are read in one place, `ResinApp::keys` (`crates/ui-egui/src/lib.rs`). Esc puts the open tool away, Delete and Backspace delete the selection, Ctrl+Z undoes and Ctrl+Y or Ctrl+Shift+Z redoes (`UNDO`, `REDO`), everywhere. A tool adds its key there; it does not read keys the app already handles. Keys are not read while a field is being typed in.
- **Tool lifecycle:** a tool is an `ActiveTool` variant in `ResinApp::tool`. It opens (putting away the one open), shows its card, takes picks, applies or cancels, and Esc leaves it, as Move and the line tool do. A card with `Buttons::OkCancel`: Enter is OK, Esc Cancel, and Cancel leaves the undo history as if the tool never ran (`.agents/skills/resinsupport-commands/SKILL.md`). The work is a command.
- **Camera:** every view with a camera calls `navigation::handle` (`crates/ui-egui/src/navigation.rs`; `.agents/skills/resinsupport-ui/SKILL.md`).
- **Typed values** work the way they do on the Move card: a typed number applies on Enter, Esc puts back the value it had.
- If a shared piece lacks what you need, extend it for everyone; don't copy it into a private version.

## Where a choice goes
- A choice people switch while they work (a tool's behaviour, a support shape, a snap step) goes on **that tool's card**, remembered as tool data in `Prefs` on no page. It never goes in Preferences (`.agents/skills/resinsupport-preferences/SKILL.md`).
- Prefer no new option when the tool can choose well by itself.
- A new row, option, icon or button in an existing card, ribbon or menu: look at it running, the whole flow, before saying it works (`.agents/skills/resinsupport-running/SKILL.md`).

## Check it
Drive the change in the running app as a user would: open the tool, pick, press Esc, Delete, undo and redo, switch edition and back. Compare with the same steps in the Move tool and the line tool. A test that runs the frame with those keys (as `crates/ui-egui/src/support_line.rs` does) keeps it that way (`.agents/skills/resinsupport-tests/SKILL.md`).

Adapted from `solidcraft:.agents/skills/solidcraft-shared-controls/SKILL.md` at `cd1d2449b` (MIT OR Apache-2.0).
