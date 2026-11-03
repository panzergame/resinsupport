# Agent skills

Skills for working on ResinSupport, in the [Agent Skills](https://agentskills.io) format: one folder per skill, a `SKILL.md` whose `name` and `description` tell the agent when to load it. `AGENTS.md` routes to them; Claude Code also finds them through `.claude/skills`, which points here. `cargo xtask skills` fails when a skill lacks its name or description, when `AGENTS.md` or this table misses a skill, or when a skill cites a path or `cargo xtask` command that is gone. A path in SolidCraft's repository is written `solidcraft:<path>`.

The layout and much of the wording are adapted from SolidCraft's `skills/` (MIT OR Apache-2.0, at `4bdffef0`; see `ATTRIBUTION.md`).

| Skill | Loads when the agent |
|---|---|
| [`resinsupport-ground-rules`](resinsupport-ground-rules/SKILL.md) | adds a dependency, asset or wording, or ports code: independence, licences, SolidCraft ports, attribution, units |
| [`resinsupport-crash-safety`](resinsupport-crash-safety/SKILL.md) | writes non-test Rust, handles hostile input or investigates a crash |
| [`resinsupport-commands`](resinsupport-commands/SKILL.md) | adds a command or decides which crate code belongs in |
| [`resinsupport-ui`](resinsupport-ui/SKILL.md) | adds a dialog, ribbon button, panel, camera input, gizmo, icon or colour |
| [`resinsupport-ui-text`](resinsupport-ui-text/SKILL.md) | adds a label, message or language |
| [`resinsupport-preferences`](resinsupport-preferences/SKILL.md) | adds a setting or changes a default |
| [`resinsupport-tests`](resinsupport-tests/SKILL.md) | places or runs tests |
| [`resinsupport-performance`](resinsupport-performance/SKILL.md) | works on slow code, the window thread or the wasm build |
| [`resinsupport-running`](resinsupport-running/SKILL.md) | runs the app, drives it and takes screenshots |
| [`resinsupport-file-formats`](resinsupport-file-formats/SKILL.md) | touches import, export or a printer format |
