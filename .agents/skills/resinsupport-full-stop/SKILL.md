---
name: resinsupport-full-stop
description: "Use when asked to take a ResinSupport change all the way (\"full stop\") with nobody watching: make it, prove it, review it yourself, run the gate, and stop with the change ready to commit or file. Never commits, pushes or merges unless the owner said so."
---

# Full stop: from a task to a change ready to land

Adapted, through `solidcraft:.agents/skills/solidcraft-full-stop/SKILL.md` (`cd1d2449b`), from tstack's full-stop (https://github.com/t3dotgg/tstack, MIT; see `licenses/LICENSE-MIT-tstack`).

Nobody is watching. You own the change until it is proven and passes the gate. Then you stop.

## Never, unless the owner asked for it
- Commit, push, force-push or merge; ResinSupport's owner commits and pushes.
- Delete a branch, rewrite history, or publish anything (a release, a comment, an issue).

## Steps
1. **Make the change.** Read `AGENTS.md` and the skill for the area. Make the smallest change that fixes the case that happened.
2. **Prove it** with evidence you saw yourself: the failing case, then the same case passing; for UI or support behaviour, the app driven and a screenshot you looked at (`.agents/skills/resinsupport-running/SKILL.md`). Green tests on mocked input don't prove what a user sees.
3. **Review it yourself.** Read the whole diff (`git diff`, and `git status` for new files) once more, against:
   - the hard rules in `AGENTS.md`: never crash, layering, commands and undo, licences and attribution, catalogs;
   - the area skill: `.agents/skills/resinsupport-shared-controls/SKILL.md` for controls, `.agents/skills/resinsupport-tests/SKILL.md` for tests, `.agents/skills/resinsupport-repro-first/SKILL.md` for platform fixes;
   - one topic, no stray or scratch files, nothing the goal doesn't need. Cut what grew.
4. **Run the gate:** `cargo xtask fast` while you work, `cargo xtask ci` once at the end. A failure that also happens on `master` without your change is not yours: show it, don't fix it here.
5. **Write the message** the commit or pull request will carry (`.agents/skills/resinsupport-file-pr/SKILL.md`), and stop.

## The gate
All true on the final tree:
- your proof covers every change in the diff;
- `cargo xtask ci` passed;
- the message has the `Changes a default:` line, and `Test change:` if an existing test's expectation changed.

## Report
Lead with the verdict: **ready** (the gate passes), or **blocked** (what blocks it, what you tried, what is needed). Then the proposed commit message, your proof, the choices you made, what is still open, and plainly that nothing was committed.
