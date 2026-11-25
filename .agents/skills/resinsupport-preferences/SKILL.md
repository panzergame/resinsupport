---
name: resinsupport-preferences
description: "Use when a ResinSupport change alters a default behaviour, an existing shortcut or the look users already have, or when it wants a new option or setting. Covers where a choice goes (the tool's card, rarely Preferences), how old projects keep their result, the preference table and the commit's 'Changes a default' line."
---

# Defaults, options and Preferences

People rely on how ResinSupport behaves today. A change that makes users see or do something different out of the box is a change of a default.

## Where a choice goes
- **Old projects keep their old result.** When a change would alter what an existing `.rsup` project gives (its supports, its slices), the project format records the old behaviour as a legacy value, so a project saved before the change opens and rebuilds as it did; new projects get the new behaviour. Do it through a format bump (`.agents/skills/resinsupport-file-formats/SKILL.md`), not a setting.
- **A choice people switch while they work** (how a tool behaves, a support shape, a snap step) goes on that tool's or edition's card, as the support, brace and line settings do (`.agents/skills/resinsupport-shared-controls/SKILL.md`). It never goes in Preferences.
- **Preferences are for rare, app-wide choices** people set once: language, navigation, what happens on import, the build plate's look.
- Don't add a preference to keep a disputed or broken behaviour around, or to paper over a problem: fix the problem.

## The commit's line
Say it in the commit message (and a pull request's body), on its own line:
- `Changes a default: no`
- `Changes a default: yes - <what changes>, old behaviour kept behind setting <name>`, where `<name>` is the tool option, the preference or the format's legacy value.

## What counts as a default
Something people already rely on changes for everyone: an existing control, interaction, shortcut, camera or navigation behaviour, unit, theme, a setting's value, a tool that appears unasked. Not defaults: bug fixes and more correct results (a fix that makes the app do what it already claimed), new behaviour on an input that did nothing before, a new option that is off by default, tooling. Write `no` for those.

## Adding a preference (rarely)
Settings live in the `preference_settings!` table in `crates/ui-egui/src/prefs.rs`. One row gives the field, its doc comment, its default, its page in `PAGES` and its control (`check`, `slider`, `choice` or `custom`). The table generates the `Prefs` struct, its defaults, the page's Restore defaults and the page's rows in the Preferences window (`crates/ui-egui/src/preferences.rs`). A tool's remembered values are rows on no page: stored, but on no page and reset by no page.
- Its label and help are catalog messages under `prefs.` (`.agents/skills/resinsupport-ui-text/SKILL.md`).
- Preferences are saved with `#[serde(default)]`: a settings file from an older version keeps its values and gains the new field's default (`old_settings_files_keep_their_values_and_gain_the_new_ones`).
- `every_setting_has_a_page_and_restore_defaults_restores_it` checks the table; add a test next to it for the behaviour the setting switches.
