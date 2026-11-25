# Agent skills

Skills for working on ResinSupport, in the [Agent Skills](https://agentskills.io) format: one folder per skill, a `SKILL.md` whose `name` and `description` tell the agent when to load it. `AGENTS.md` routes to them. Coding assistants that read project skills from `.agents/skills/` find them here; `.claude/skills` is a symlink to this folder (on Windows, enable symlinks: Developer Mode, then `git config core.symlinks true`).

A top-level skills folder (`skills`, beside `.agents`) is kept for skills that agents *using* ResinSupport would load, as SolidCraft's MCP skills are; there are none yet.

`cargo xtask skills` fails when:
- a skill lacks its name or description, or its name is not its folder's;
- `AGENTS.md` or this table misses a skill, or names one that is gone;
- a skill cites a path or `cargo xtask` command that is gone;
- `AGENTS.md` grows past a router's size, or a new skill past 3600 characters (move detail to a reference file beside it);
- `.claude/skills` stops pointing here.

A path in SolidCraft's repository is written `solidcraft:<path>`.

The layout and much of the wording are adapted from SolidCraft's `.agents/skills/` (MIT OR Apache-2.0, at `cd1d2449b`; `4bdffef0` for the first set; see `ATTRIBUTION.md`). `resinsupport-file-pr` and `resinsupport-full-stop` come, through SolidCraft, from [tstack](https://github.com/t3dotgg/tstack) by Theo Browne, MIT (`licenses/LICENSE-MIT-tstack`).

| Skill | Loads when the agent |
|---|---|
| [`resinsupport-ground-rules`](resinsupport-ground-rules/SKILL.md) | adds a dependency, asset or wording, or ports code: independence, licences, SolidCraft ports, attribution, units |
| [`resinsupport-crash-safety`](resinsupport-crash-safety/SKILL.md) | writes non-test Rust, handles hostile input or investigates a crash |
| [`resinsupport-commands`](resinsupport-commands/SKILL.md) | adds a command or decides which crate code belongs in |
| [`resinsupport-ui`](resinsupport-ui/SKILL.md) | adds a dialog, ribbon button, panel, tooltip, camera input, gizmo, icon or colour, or gives a card presets |
| [`resinsupport-shared-controls`](resinsupport-shared-controls/SKILL.md) | changes selection, picking, keys, a tool's start and finish, or wants a new option: shared controls, and where a choice goes |
| [`resinsupport-ui-text`](resinsupport-ui-text/SKILL.md) | adds a label, message or language |
| [`resinsupport-preferences`](resinsupport-preferences/SKILL.md) | changes a default or wants a new option or setting |
| [`resinsupport-tests`](resinsupport-tests/SKILL.md) | places, writes or runs tests |
| [`resinsupport-performance`](resinsupport-performance/SKILL.md) | works on slow code, the window thread or the wasm build |
| [`resinsupport-running`](resinsupport-running/SKILL.md) | runs the app, drives it and takes screenshots |
| [`resinsupport-file-formats`](resinsupport-file-formats/SKILL.md) | touches import, export or a printer format |
| [`resinsupport-repro-first`](resinsupport-repro-first/SKILL.md) | fixes a bug seen only on some machines: an OS, graphics backend or driver, display scaling |
| [`resinsupport-full-stop`](resinsupport-full-stop/SKILL.md) | takes a change end to end with nobody watching: prove it, review it, leave it ready to commit or file |
| [`resinsupport-file-pr`](resinsupport-file-pr/SKILL.md) | writes a commit message or opens a pull request: the title, the body, the lines it needs, screenshots |
