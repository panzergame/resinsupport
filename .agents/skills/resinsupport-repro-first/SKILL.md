---
name: resinsupport-repro-first
description: "Use before fixing a ResinSupport bug that happens only on some machines: an operating system, desktop or window system, graphics backend or GPU driver, display scaling, or the browser build. Covers the evidence a fix needs and how to keep everyone else's path unchanged."
---

# Platform, graphics and driver fixes: reproduce first

These bugs show on one kind of machine and not on yours. A fix written from a guess often changes the path that works for everyone else, so the evidence comes first.

## Before you write code
- Get the log or a recording from the machine where it fails, with the build it ran (a commit or a release) and how it was started. The log is what `RUST_LOG=info cargo run -p resinsupport` prints.
- Record the configuration: the operating system and version, X11 or Wayland, the wgpu backend (Vulkan, Metal, DX12, GL, WebGPU) and adapter, the GPU and driver version, the display scaling.
- Say what the user does, what they expect and what happens. Strip user names, home folders and private models from logs.
- No evidence yet: ask for it, and don't write a speculative fix.

## The fix
- Scope it to the condition you saw: detect that system, backend, driver or setting and change behaviour only there. Name the condition in the code and in the commit.
- The default path stays as it is for every other machine. A fix that would change what every user gets is a changed default (`.agents/skills/resinsupport-preferences/SKILL.md`).
- Don't add a preference so users can work around the bug: fix the bug.
- Don't swallow the error to hide it: when the app can't do something on that machine, it says so and goes on with what it can.
- Never crash (`.agents/skills/resinsupport-crash-safety/SKILL.md`), and keep work off the window thread (`.agents/skills/resinsupport-performance/SKILL.md`).

## Proving it
- Show the same check passing on the affected machine with the fixed build, and keep its log or screenshot.
- If you can't reach that machine, say so and ask the reporter to try the build. Don't call it fixed before they have.
- Show that the usual path still works: run the app without the condition (Xvfb and lavapipe count, `.agents/skills/resinsupport-running/SKILL.md`), and say which tests cover it.

Adapted from `solidcraft:.agents/skills/solidcraft-repro-first/SKILL.md` at `cd1d2449b` (MIT OR Apache-2.0).
