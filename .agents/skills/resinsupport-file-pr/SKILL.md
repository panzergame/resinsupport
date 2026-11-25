---
name: resinsupport-file-pr
description: "Use when writing a ResinSupport commit message or opening or updating a pull request: the title, the body, the lines it must carry, and the screenshots a UI change needs."
---

# Commit messages and pull requests

Adapted, through `solidcraft:.agents/skills/solidcraft-file-pr/SKILL.md` (`cd1d2449b`), from tstack's file-pr (https://github.com/t3dotgg/tstack, MIT; see `licenses/LICENSE-MIT-tstack`).

## Before
- Read your diff against `master`: one topic; no `target/`, scratch or generated files.
- Look for an open pull request or issue on the same files or problem (`gh pr list`, `gh issue list`).

## The title
It is the commit's first line on `master`. Form: `Area: what the user or caller now gets, in plain words`.
- Area: what people recognise (UI, Supports, Slicing, Labels, Import, CTB export, Docs, Lint), not a crate or file.
- Behaviour, not mechanism, in the present tense. For a fix, say what now works, not "fix bug".
- No `feat:`/`fix:`, no emoji, no issue number. Under about 100 characters, important words first.

From the history:
- Good: `UI: a model's raft is part of its object, lit and picked with it`
- Good: `Supports: hiding a model rebuilds nothing; a raft per model`
- Bad: `fix(ui-egui): handle raft in pick`. Better: `UI: clicking a raft selects its model`

## The body
Open with the problem as a user meets it, then the result. Don't list the functions you changed. In your own words: no chat transcripts or tool footers. Then, each on its own line:
- `Changes a default: no`, or `Changes a default: yes - <what>, old behaviour kept behind setting <name>` (`.agents/skills/resinsupport-preferences/SKILL.md`).
- `Test change: <reason>` when an existing test's expected value changed or a test was removed: why the old value was wrong, derived. New tests need no line (`.agents/skills/resinsupport-tests/SKILL.md`).
- Validation: what ran (`cargo xtask ci`, the app driven) and what came out; expected and measured values for geometry.

## Pictures (pull requests)
- Screenshots of the whole flow, uncropped and captured yourself: the ribbon button, the card, the picks, the result; before and after is best.
- Check pictures and logs for private models, home folders and user names.
