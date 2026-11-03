// Reduced from SolidCraft (MIT OR Apache-2.0), crates/ui-egui/src/i18n.rs at 4bdffef0
// (`Catalog`, `match_system_tags`, `effective_language`, named placeholders); see ATTRIBUTION.md.

//! UI text. Every label, tooltip and message is a stable id in the catalogs under
//! `assets/locales` (one JSON object per language, sorted by id, one message per line), embedded
//! at build time. [`tr`] looks an id up in the current language, then in English; [`format`]
//! fills named placeholders (`{name}`) into a whole sentence; [`count`] picks the `.one` or
//! `.other` form of a counted message. Adding a language is a catalog file and one line in
//! [`CATALOGS`].

use std::collections::BTreeMap;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};

pub struct Catalog {
    /// A BCP 47 tag: `en`, `fr`, `pt-BR`.
    pub id: &'static str,
    /// The language's own name for itself.
    pub name: &'static str,
    source: &'static str,
    messages: OnceLock<BTreeMap<String, String>>,
}

impl Catalog {
    const fn new(id: &'static str, name: &'static str, source: &'static str) -> Catalog {
        Catalog { id, name, source, messages: OnceLock::new() }
    }

    /// The messages, parsed once. A catalog that fails to parse (a bug the tests catch) is logged
    /// and empty, so its language falls back to English.
    pub fn messages(&self) -> &BTreeMap<String, String> {
        self.messages.get_or_init(|| match serde_json::from_str(self.source) {
            Ok(m) => m,
            Err(e) => {
                log::error!("catalog {}: {e}", self.id);
                BTreeMap::new()
            }
        })
    }

    pub fn source(&self) -> &'static str {
        self.source
    }
}

static ENGLISH: Catalog = Catalog::new("en", "English", include_str!("../../../assets/locales/en.json"));

/// Every language, English first.
pub static CATALOGS: &[&Catalog] = &[&ENGLISH];

/// Languages a new message must be given in: the tests fail when one of them lacks a message
/// English has.
pub const COMPLETE: &[&str] = &["en"];

/// The first part of every message id. Command ids (`file.import_stl`, `view.fit`) live apart, under
/// `cmd.`, so a literal shaped like a message id is always one: the tests check them all.
pub const MESSAGE_NAMESPACES: &[&str] = &[
    "appbar",
    "cmd",
    "group",
    "key",
    "menu",
    "mode",
    "models",
    "nav",
    "prefs",
    "reason",
    "ribbon",
    "status",
    "stdview",
    "tab",
    "tool",
    "ui",
    "viewport",
    "workspace",
];

/// The `language` preference's value that follows the system.
pub const SYSTEM: &str = "system";

static CURRENT: AtomicUsize = AtomicUsize::new(0);

pub fn is_supported(id: &str) -> bool {
    CATALOGS.iter().any(|c| c.id == id)
}

/// Show the UI in language `id` (English for an unknown one).
pub fn set_language(id: &str) {
    CURRENT.store(CATALOGS.iter().position(|c| c.id == id).unwrap_or(0), Ordering::Relaxed);
}

pub fn current() -> &'static str {
    CATALOGS.get(CURRENT.load(Ordering::Relaxed)).map_or("en", |c| c.id)
}

/// The registered language for the system's preferred tags: the exact tag, then its base
/// language, then English.
pub fn match_system_tags(tags: &[String]) -> &'static str {
    for tag in tags {
        let tag = tag.replace('_', "-");
        if let Some(c) = CATALOGS.iter().find(|c| c.id.eq_ignore_ascii_case(&tag)) {
            return c.id;
        }
        let base = tag.split('-').next().unwrap_or_default();
        if let Some(c) = CATALOGS.iter().find(|c| c.id.eq_ignore_ascii_case(base)) {
            return c.id;
        }
    }
    "en"
}

/// The system's language, read once.
pub fn system_language() -> &'static str {
    static SYSTEM_LANGUAGE: OnceLock<&'static str> = OnceLock::new();
    SYSTEM_LANGUAGE.get_or_init(|| match_system_tags(&sys_locale::get_locales().collect::<Vec<_>>()))
}

/// The language the `language` preference asks for: `system`, or a registered id.
pub fn effective_language(preference: &str) -> &'static str {
    if preference == SYSTEM { system_language() } else { CATALOGS.iter().find(|c| c.id == preference).map_or_else(system_language, |c| c.id) }
}

/// Whether English has message `id`.
pub fn has(id: &str) -> bool {
    ENGLISH.messages().contains_key(id)
}

/// Message `id` in the current language, else English.
pub fn lookup(id: &str) -> Option<&'static str> {
    let get = |c: &'static Catalog| c.messages().get(id).map(String::as_str);
    CATALOGS.get(CURRENT.load(Ordering::Relaxed)).and_then(|c| get(c)).or_else(|| get(&ENGLISH))
}

/// Message `id` in the current language, else English, else the id itself (so a missing
/// message shows, and the tests name it).
pub fn tr(id: &'static str) -> &'static str {
    lookup(id).unwrap_or(id)
}

/// [`tr`] for an id built at run time (`"ribbon.move.tip"`).
pub fn tr_built(id: &str) -> String {
    lookup(id).map_or_else(|| id.to_string(), str::to_string)
}

/// Message `id` with its `{name}` placeholders filled. Values go in literally: braces in a value
/// (a file name) are never read as placeholders.
pub fn format(id: &'static str, args: &[(&str, &dyn std::fmt::Display)]) -> String {
    fill(tr(id), args)
}

/// The `.one` or `.other` form of a counted message, with `{n}` filled.
pub fn count(id: &'static str, n: usize, args: &[(&str, &dyn std::fmt::Display)]) -> String {
    let form = if n == 1 { "one" } else { "other" };
    let key = format!("{id}.{form}");
    let template = lookup(&key).unwrap_or(key.as_str());
    let mut all: Vec<(&str, &dyn std::fmt::Display)> = vec![("n", &n)];
    all.extend_from_slice(args);
    fill(template, &all)
}

fn fill(template: &str, args: &[(&str, &dyn std::fmt::Display)]) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(rest.get(..open).unwrap_or_default());
        let after = rest.get(open + 1..).unwrap_or_default();
        match after.find('}').and_then(|close| Some((close, after.get(..close)?))) {
            Some((close, name)) if args.iter().any(|(n, _)| *n == name) => {
                if let Some((_, v)) = args.iter().find(|(n, _)| *n == name) {
                    out.push_str(&v.to_string());
                }
                rest = after.get(close + 1..).unwrap_or_default();
            }
            _ => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// The `{name}` placeholders in a message, sorted.
pub fn placeholders(text: &str) -> Vec<&str> {
    let mut out: Vec<&str> = text.split('{').skip(1).filter_map(|s| s.split_once('}').map(|(name, _)| name)).collect();
    out.sort_unstable();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalogs_parse_and_are_registered_once() {
        let mut ids: Vec<&str> = CATALOGS.iter().map(|c| c.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), CATALOGS.len());
        for c in CATALOGS {
            assert!(serde_json::from_str::<BTreeMap<String, String>>(c.source()).is_ok(), "{}", c.id);
        }
        assert!(COMPLETE.iter().all(|id| is_supported(id)));
        assert!(ENGLISH.messages().len() > 50);
    }

    #[test]
    fn every_complete_catalog_carries_every_english_message() {
        for c in CATALOGS.iter().filter(|c| COMPLETE.contains(&c.id)) {
            let missing: Vec<&String> = ENGLISH.messages().keys().filter(|k| !c.messages().contains_key(*k)).collect();
            assert!(missing.is_empty(), "{} lacks {missing:?}", c.id);
        }
    }

    /// A message whose text is its own id was filled in by a tool, not written.
    #[test]
    fn no_message_is_its_own_id() {
        for c in CATALOGS {
            let echoes: Vec<&String> = c.messages().iter().filter(|(id, text)| id == text).map(|(id, _)| id).collect();
            assert!(echoes.is_empty(), "{}: {echoes:?}", c.id);
        }
    }

    #[test]
    fn translations_keep_englishs_placeholders() {
        for c in CATALOGS {
            for (id, text) in c.messages() {
                if let Some(en) = ENGLISH.messages().get(id) {
                    assert_eq!(placeholders(text), placeholders(en), "{} {id}", c.id);
                } else {
                    panic!("{} has {id}, which English lacks", c.id);
                }
            }
        }
    }

    #[test]
    fn every_command_label_is_in_the_english_catalog() {
        for spec in resin_engine::commands() {
            let id = format!("cmd.{}", spec.id);
            assert_eq!(ENGLISH.messages().get(&id).map(String::as_str), Some(spec.label), "{id}: add it to en.json (cargo xtask i18n --add-missing)");
        }
    }

    /// Every string literal in the UI's source shaped like a message id (`ribbon.move`,
    /// `status.deleted_models` for a counted one) is in English, so no message shows as its id.
    #[test]
    fn every_id_the_source_names_is_in_the_english_catalog() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut missing = Vec::new();
        let mut seen = 0;
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            let text = std::fs::read_to_string(&path).unwrap();
            for (i, literal) in text.split('"').enumerate() {
                let id_like = i % 2 == 1
                    && literal.split_once('.').is_some_and(|(ns, rest)| MESSAGE_NAMESPACES.contains(&ns) && !rest.is_empty())
                    && literal.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '_')
                    // A file name (`viewport.wgsl`) is not a message.
                    && !literal.rsplit('.').next().is_some_and(|ext| ["json", "md", "rs", "svg", "toml", "wgsl"].contains(&ext));
                if !id_like {
                    continue;
                }
                seen += 1;
                let counted = has(&format!("{literal}.one")) && has(&format!("{literal}.other"));
                if !(has(literal) || counted) {
                    missing.push(format!("{}: {literal}", path.file_name().unwrap().to_string_lossy()));
                }
            }
        }
        assert!(seen > 100, "the sweep found only {seen} ids");
        assert!(missing.is_empty(), "not in en.json: {missing:#?}");
    }

    #[test]
    fn placeholders_fill_literally_and_counts_pick_their_form() {
        let s = fill("Imported {name} ({n} triangles)", &[("name", &"a{n}b"), ("n", &3)]);
        assert_eq!(s, "Imported a{n}b (3 triangles)");
        assert_eq!(fill("{unknown} {", &[]), "{unknown} {");
        assert_eq!(placeholders("{a} and {b} and {a}"), vec!["a", "b"]);
        assert!(count("status.deleted_models", 1, &[]).contains('1'));
        assert_ne!(count("status.deleted_models", 1, &[]), count("status.deleted_models", 2, &[]).replace('2', "1"));
    }

    #[test]
    fn system_tags_match_exactly_then_by_base_then_english() {
        assert_eq!(match_system_tags(&["en-US".into()]), "en");
        assert_eq!(match_system_tags(&["xx_YY".into(), "en_GB".into()]), "en");
        assert_eq!(match_system_tags(&["xx".into()]), "en");
        assert_eq!(match_system_tags(&[]), "en");
        assert_eq!(effective_language("nope"), system_language());
    }
}
