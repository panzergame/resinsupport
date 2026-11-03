---
name: resinsupport-ui-text
description: "Use when adding or changing any user-visible text in ResinSupport: a label, tooltip, status message, command label or a new language. Covers the message catalogs in assets/locales, stable ids, placeholders, counted messages and sorted catalogs."
---

# UI text and translations

ResinSupport embeds its translations and works offline (`crates/ui-egui/src/i18n.rs`). English (`assets/locales/en.json`) is the source and, for now, the only language.

## Adding a message
1. Give it a stable id under one of the namespaces in `MESSAGE_NAMESPACES` (`ribbon.move`, `tool.scale.uniform`, `status.imported`). Ids are stable: keep them when the wording changes. Never key a message by its English text.
2. Add the id and its English text to `assets/locales/en.json`, then run `cargo xtask lists --fix` to put it in place (sorted by id, one message per line).
3. In the code: `i18n::tr("id")` for a fixed message; `i18n::format("id", &[("name", &value)])` for a sentence with named placeholders; `i18n::count("id", n, &[…])` for a counted one, which reads `id.one` or `id.other` and fills `{n}`. Translate the whole sentence, punctuation included, so a translator can reorder its parts; never build a sentence by concatenating messages.
4. A table of ids (a ribbon button's label, a reason, a preset's name) holds the id; the code calls `i18n::tr` where it draws.
5. Command labels are `cmd.<command id>`: `cargo xtask i18n --add-missing` writes the missing ones.

## What the tests check
- `every_id_the_source_names_is_in_the_english_catalog`: every literal in `crates/ui-egui/src` shaped like a message id is in English. Keep command ids (`view.fit`) and message ids apart: messages never use a command's first part.
- `every_complete_catalog_carries_every_english_message`: each language in `COMPLETE` has every English message.
- `translations_keep_englishs_placeholders`, `every_command_label_is_in_the_english_catalog`.
- `cargo xtask lists` (catalogs canonical) and `cargo xtask i18n --check` run in `cargo xtask ci`; `cargo xtask i18n` alone reports every language's gaps.

## Not messages
The product name, key combinations (`Ctrl+I`), unit symbols (`mm`, `°`, `%`), axis letters and file-format names are the same in every language. Named keys (Delete) are messages (`key.delete`). Engine error texts are English for now.

## Adding a language
Add `assets/locales/<id>.json` with the same ids, register it in `CATALOGS` with its own name for itself, and add its id to `COMPLETE` once it carries every message. A language left out of `COMPLETE` falls back to English message by message.
