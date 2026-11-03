---
name: resinsupport-commands
description: "Use when adding or changing a ResinSupport command or any user-visible behaviour, or when deciding which crate code belongs in. Covers CommandSpec, command ids, programmatic calls versus dialogs, crate layering and the docs a command updates."
---

# Adding a command

User-visible behaviour is a command. The ribbon, dialogs, gizmo, tests and future scripting reach the same code through `Session::execute(id, params)`, which returns JSON.

## Steps
1. Add a `CommandSpec` (id, label, params doc, run) to the registry in `crates/engine/src/cmd.rs`, and implement `run` (`fn(&mut Session, &Value) -> Result<Value>`) in the area's file under `crates/engine/src/cmd/`. The list stays sorted by id (`commands_are_sorted_and_unique`).
2. The id is our own: dotted, lower-case `<area>.<thing>[.<variant>]` (`model.place`, `support.add`).
3. Validate inputs first and return `EngineError::BadParams` rather than half-applying. Parameters are hostile: `skills/resinsupport-crash-safety/SKILL.md`. Shared parsers live in `crates/engine/src/cmd.rs` (`vec3_param`, `ids_param`, `num_param`).
4. Engine tests measure the result against values derived by hand (`crates/engine/src/tests.rs`; `skills/resinsupport-tests/SKILL.md`).
5. Add its label to the English catalog as `cmd.<command id>`: `cargo xtask i18n --add-missing` writes it; `every_command_label_is_in_the_english_catalog` fails without it (`skills/resinsupport-ui-text/SKILL.md`).
6. Add a row to the commands table in `docs/architecture.md`, and tick `ROADMAP.md` when a milestone task lands.
7. A button or dialog for it: `skills/resinsupport-ui/SKILL.md`.

## Programmatic calls never open dialogs
A command takes its parameters as JSON and never opens a dialog or file picker. Dialogs and tools are front-end forms that build the parameters and call `app.run(id, params)`; continuous drags run one command per frame built from the values the drag began with (`crates/ui-egui/src/transform.rs`). Camera drags are the one exception: they move `session.camera` directly.

## Layering
`cargo xtask layers` fails when a lower layer depends on a higher one:

L0 `geom` → L1 `render` → L3 `io` → L4 `engine` → L5 `ui-egui` → apps.

- Nothing below L5 uses egui, eframe, wgpu, winit or rfd.
- No `std::fs` outside `io` and the apps, and file access behind `cfg(not(target_arch = "wasm32"))`.
- The UI is thin: panels read the session and act through commands.
