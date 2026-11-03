---
name: resinsupport-preferences
description: "Use when a ResinSupport change alters a default behaviour, an existing shortcut or the look users already have, or adds a setting to Preferences. Covers keeping the old behaviour behind a preference, the preference table and the commit message's 'Changes a default' line."
---

# Preferences and defaults

People rely on how ResinSupport behaves today. A change that makes users see or do something different out of the box is a change of a default.

## The rule
- Keep the old behaviour reachable: add a preference whose old value gives the old behaviour, and test that it still does.
- Say so in the commit message, on its own line:
  - `Changes a default: no`
  - `Changes a default: yes - <what changes>, old behaviour kept behind preference <name>`

## Adding a preference
Settings live in the `preference_settings!` table in `crates/ui-egui/src/prefs.rs`. One row gives the field, its doc comment, its default, its page in `PAGES` and its control (`check`, `slider`, `choice` or `custom`). The table generates the `Prefs` struct, its defaults, the page's Restore defaults and the page's rows in the Preferences window (`crates/ui-egui/src/preferences.rs`).
- Its label and help are catalog messages under `prefs.` (`skills/resinsupport-ui-text/SKILL.md`).
- Preferences are saved with `#[serde(default)]`: a settings file from an older version keeps its values and gains the new field's default (`old_settings_files_keep_their_values_and_gain_the_new_ones`).
- `every_setting_has_a_page_and_restore_defaults_restores_it` checks the table; add a test next to it for the behaviour the setting switches.

## What counts as a default
Anything a user sees or does differently on first launch after the update: a changed shortcut, camera or navigation behaviour, unit, theme, a tool that appears unasked. A fix that makes the app do what it already claimed is not a default change; say so on the line.
