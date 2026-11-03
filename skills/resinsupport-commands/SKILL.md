---
name: resinsupport-commands
description: "Use when adding or changing a ResinSupport command or any user-visible behaviour, when adding state the user edits, or when deciding which crate code belongs in. Covers CommandSpec, command ids, undo and redo, programmatic calls versus dialogs, crate layering and the docs a command updates."
---

# Adding a command

User-visible behaviour is a command. The ribbon, dialogs, gizmo, tests and future scripting reach the same code through `Session::execute(id, params)`, which returns JSON.

## Steps
1. Add a `CommandSpec::new(id, label, run).params("…")` to the registry in `crates/engine/src/cmd.rs`, and implement `run` (`fn(&mut Session, &Value) -> Result<Value>`) in the area's file under `crates/engine/src/cmd/`. The list stays sorted by id (`commands_are_sorted_and_unique`).
2. The id is our own: dotted, lower-case `<area>.<thing>[.<variant>]` (`model.place`, `support.add`).
3. Validate inputs first and return `EngineError::BadParams` rather than half-applying. Parameters are hostile: `skills/resinsupport-crash-safety/SKILL.md`. Shared parsers live in `crates/engine/src/cmd.rs` (`vec3_param`, `ids_param`, `num_param`).
4. Engine tests measure the result against values derived by hand (`crates/engine/src/tests.rs`; `skills/resinsupport-tests/SKILL.md`). Give it a row in `sample` there, valid parameters on the fixture: `every_scene_change_is_undoable` fails for a command without one, and checks it undoes and redoes (below).
5. Add its label to the English catalog as `cmd.<command id>`: `cargo xtask i18n --add-missing` writes it; `every_command_label_is_in_the_english_catalog` fails without it (`skills/resinsupport-ui-text/SKILL.md`).
6. Add a row to the commands table in `docs/architecture.md`, and tick `ROADMAP.md` when a milestone task lands.
7. A button or dialog for it: `skills/resinsupport-ui/SKILL.md`.

## Undo and redo
Ported from SolidCraft. `Session::execute` makes the history: when a command succeeds and the scene's revision moved, the scene from before is pushed as one step (`MAX_UNDO`, 200, are kept) and redo is cleared. `edit.undo` and `edit.redo` swap the scene with a step through `Scene::restore`, which keeps ids and the revision growing, so the GPU never reuses a mesh for another model and the pick cache never mistakes the restored scene. The camera is not in a step.

So a new command is undoable with no work of its own, as long as:
- What the user edits lives in `Scene` (`crates/engine/src/session.rs`). State kept in `Session` beside it would be left out of every step.
- It changes the scene only through `Scene` methods, and each method that changes something bumps `revision`, and one that changes nothing does not (an empty step). `models` and `supports` are private for that reason; read them through `models()` and `supports()`. Selection is not a step of its own; it comes back with the step it was in.
- A large collection in `Scene` is `Arc`-shared (as meshes and indexes are), so a step copies a pointer: a step is a whole `Scene`, and `execute` copies one on every call, every frame of a drag.
- It stays `undoable` (the default). `.noundo()` is for the history commands only: a command that changes no scene makes no step anyway.

In the front end, one gesture is one step. A gizmo drag or a dragged or typed field calls `session.begin_step()` before each command it runs; the app ends the step once no button is held and no field is typed in (`ResinApp::ui`). Several commands for one action (deleting several models, importing several files) sit between `begin_step` and `end_step`. A tool whose Cancel puts the scene back takes `history_mark()` when it opens and calls `forget_since(mark, command)` after restoring, so the undo history reads as though it never ran (redo, cleared by its first edit, does not come back) (`crates/ui-egui/src/transform.rs`). Undo and redo put the open tool away.

## Programmatic calls never open dialogs
A command takes its parameters as JSON and never opens a dialog or file picker. Dialogs and tools are front-end forms that build the parameters and call `app.run(id, params)`; continuous drags run one command per frame built from the values the drag began with (`crates/ui-egui/src/transform.rs`). Camera drags are the one exception: they move `session.camera` directly.

## Layering
`cargo xtask layers` fails when a lower layer depends on a higher one:

L0 `geom` → L1 `render` → L3 `io` → L4 `engine` → L5 `ui-egui` → apps.

- Nothing below L5 uses egui, eframe, wgpu, winit or rfd.
- No `std::fs` outside `io` and the apps, and file access behind `cfg(not(target_arch = "wasm32"))`.
- The UI is thin: panels read the session and act through commands.
