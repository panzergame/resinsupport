//! The message catalogs (`assets/locales/*.json`): sorted and canonical (`lists`), complete where
//! they must be, and carrying every command label (`i18n`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

type Catalog = BTreeMap<String, String>;

fn locales(root: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(root.join("assets/locales"))
        .map(|rd| rd.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|x| x == "json")).collect())
        .unwrap_or_default();
    files.sort();
    files
}

fn read(path: &Path) -> Result<Catalog, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}

/// One message per line, sorted by id, two-space indent, a final newline.
fn canonical(c: &Catalog) -> Result<String, String> {
    serde_json::to_string_pretty(c).map(|s| s + "\n").map_err(|e| e.to_string())
}

fn write(path: &Path, c: &Catalog) -> Result<(), String> {
    std::fs::write(path, canonical(c)?).map_err(|e| format!("{}: {e}", path.display()))
}

/// Catalogs not in their canonical form (unsorted, repeated ids, other spacing); `fix` rewrites
/// them.
pub fn lists(root: &Path, fix: bool) -> Vec<String> {
    let mut problems = Vec::new();
    for path in locales(root) {
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        let catalog = match read(&path) {
            Ok(c) => c,
            Err(e) => {
                problems.push(e);
                continue;
            }
        };
        // A repeated id parses (the last one wins) but would lose a message.
        let ids = text.lines().filter(|l| l.trim_start().starts_with('"')).count();
        let Ok(want) = canonical(&catalog) else { continue };
        if text != want || ids != catalog.len() {
            if fix && ids == catalog.len() {
                if let Err(e) = write(&path, &catalog) {
                    problems.push(e);
                }
            } else {
                problems.push(format!("{}: not sorted one message per line, or an id repeats (cargo xtask lists --fix)", path.display()));
            }
        }
    }
    problems
}

/// The engine's commands and their labels, read from the registry's source.
fn commands(root: &Path) -> Vec<(String, String)> {
    let text = std::fs::read_to_string(root.join("crates/engine/src/cmd.rs")).unwrap_or_default();
    let mut out = Vec::new();
    for chunk in text.split("CommandSpec {").skip(1) {
        let field = |name: &str| chunk.split(&format!("{name}: \"")).nth(1).and_then(|s| s.split('"').next()).map(str::to_string);
        if let (Some(id), Some(label)) = (field("id"), field("label")) {
            out.push((id, label));
        }
    }
    out
}

/// The languages a new message must be given in, from `COMPLETE` in the UI's i18n module.
fn complete(root: &Path) -> Vec<String> {
    let text = std::fs::read_to_string(root.join("crates/ui-egui/src/i18n.rs")).unwrap_or_default();
    text.split("pub const COMPLETE: &[&str] = &[")
        .nth(1)
        .and_then(|s| s.split(']').next())
        .map(|list| list.split(',').map(|s| s.trim().trim_matches('"').to_string()).filter(|s| !s.is_empty()).collect())
        .unwrap_or_default()
}

/// Each language's gaps, printed; with `check`, a complete language's gap or a command label
/// English lacks is a problem. `add_missing` writes the missing command labels into `en.json`.
pub fn i18n(root: &Path, check: bool, add_missing: bool) -> Vec<String> {
    let mut problems = Vec::new();
    let en_path = root.join("assets/locales/en.json");
    let mut en = match read(&en_path) {
        Ok(c) => c,
        Err(e) => return vec![e],
    };
    let mut added = 0;
    for (id, label) in commands(root) {
        let key = format!("cmd.{id}");
        if en.get(&key) != Some(&label) {
            if add_missing {
                en.insert(key, label);
                added += 1;
            } else {
                problems.push(format!("en.json: `{key}` is not \"{label}\" (cargo xtask i18n --add-missing)"));
            }
        }
    }
    if added > 0 {
        eprintln!("  added {added} command labels to en.json");
        if let Err(e) = write(&en_path, &en) {
            problems.push(e);
        }
    }
    let complete = complete(root);
    for path in locales(root) {
        let id = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        if id == "en" {
            continue;
        }
        let Ok(c) = read(&path) else { continue };
        let missing: Vec<&String> = en.keys().filter(|k| !c.contains_key(*k)).collect();
        if missing.is_empty() {
            continue;
        }
        let line = format!("{id}: {} of {} messages missing", missing.len(), en.len());
        if complete.contains(&id) {
            problems.push(format!("{line} (a complete language): {missing:?}"));
        } else {
            eprintln!("  {line}");
        }
    }
    if !check {
        // A report: the problems are shown but fail nothing.
        for p in &problems {
            eprintln!("  {p}");
        }
        return Vec::new();
    }
    problems
}
