//! `cargo xtask design-pull`: what the design file owns, back into the code. Its variables are
//! exported to `design/tokens.json` and written into the code's token regions
//! (`design_tokens`); each `icon/<name>` component is exported in the Icon collection's Source
//! mode, rewritten as a master (`svg_norm`) and, when it changed since the last pull
//! (`design/icons.lock`), written to `crates/ui-egui/icons/<name>.svg`.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::json;

use crate::design_export::{DESIGN_FILE, ICONS_DIR, work_dir};
use crate::design_tokens::{self, SOURCE_MODE, TOKENS, Value as Tok};
use crate::figcraft::{Cli, step};
use crate::svg_norm;

pub const ICONS_LOCK: &str = "design/icons.lock";

pub fn design_pull(root: &Path) -> Vec<String> {
    let result = Cli::find().and_then(|cli| pull(root, &cli, true));
    match result {
        Ok(()) => design_tokens::design_tokens(root, false),
        Err(e) => vec![e],
    }
}

fn read_lock(root: &Path) -> BTreeMap<String, String> {
    std::fs::read_to_string(root.join(ICONS_LOCK))
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| l.split_once(' ').map(|(n, f)| (n.to_string(), f.trim().to_string())))
        .collect()
}

fn write_lock(root: &Path, lock: &BTreeMap<String, String>) -> Result<(), String> {
    let mut text = String::from(
        "# Each icon component's fingerprint as `cargo xtask design-pull` last wrote it: only a changed icon\n# is written back to crates/ui-egui/icons.\n",
    );
    for (name, fp) in lock {
        text += &format!("{name} {fp}\n");
    }
    std::fs::write(root.join(ICONS_LOCK), text).map_err(|e| format!("{ICONS_LOCK}: {e}"))
}

/// The title a master carries, kept when it is written again.
fn title_of(svg: &str) -> Option<String> {
    let start = svg.find("<title>")? + "<title>".len();
    let end = svg.get(start..)?.find("</title>")?;
    svg.get(start..start + end).map(str::to_string)
}

/// Export the tokens and the icons. `write_icons`: write the icons that changed since the lock
/// (else only record their fingerprints, as the export does for the masters it started from).
pub fn pull(root: &Path, cli: &Cli, write_icons: bool) -> Result<(), String> {
    let file = root.join(DESIGN_FILE);
    if !file.is_file() {
        return Err(format!("{DESIGN_FILE}: no design file (cargo xtask design-export makes it)"));
    }
    let work = work_dir(root);
    let (scripts, out) = (work.join("scripts"), work.join("icons-out"));
    for d in [&scripts, &out] {
        let _ = std::fs::remove_dir_all(d);
        std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
    }
    let design = root.join("design");
    let first = [step("variable.export", json!({ "path": root.join(TOKENS) })), step("component.list", json!({}))];
    let results = cli.run(&scripts, "pull-a", &first, Some(&file), None, &[&design, &work], &[&design])?;
    eprintln!("  wrote {TOKENS}");
    let icons: Vec<(String, u64)> = results
        .get(1)
        .and_then(|r| r.as_array())
        .into_iter()
        .flatten()
        .filter(|c| c["type"].as_str().is_some_and(|t| t.eq_ignore_ascii_case("component")))
        .filter_map(|c| Some((c["name"].as_str()?.strip_prefix("icon/")?.to_string(), c["id"].as_u64()?)))
        .collect();
    let mut second = Vec::new();
    for (name, id) in &icons {
        if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
            return Err(format!("icon component `icon/{name}`: a name is letters, digits, `_` and `-`"));
        }
        second.push(step("variable.setMode", json!({ "ids": [id], "collection": "Icon", "mode": SOURCE_MODE })));
        second.push(step("file.exportSvg", json!({ "ids": [id], "path": out.join(format!("{name}.svg")), "svgOutlineText": true })));
    }
    cli.run(&scripts, "pull-b", &second, Some(&file), None, &[&design, &work], &[&work])?;

    // An exported colour → what a master writes: its role's colour, `currentColor` for `line`.
    let vars = design_tokens::read_variables(&std::fs::read_to_string(root.join(TOKENS)).map_err(|e| format!("{TOKENS}: {e}"))?)?;
    let code = design_tokens::code_tokens(root).map_err(|p| p.join("\n  "))?;
    let mut paints: BTreeMap<String, String> = BTreeMap::new();
    for t in code.iter().filter(|t| t.collection == "Icon") {
        let Some(Tok::Colour(c)) = vars.get("Icon", &t.name, SOURCE_MODE) else {
            return Err(format!("{TOKENS}: icon role `{}` has no {SOURCE_MODE} colour", t.name));
        };
        let master = if t.name == "line" { "currentColor".to_string() } else { t.source.clone().unwrap_or_default() };
        if let Some(other) = paints.insert(design_tokens::hex_of(c), master) {
            return Err(format!("{TOKENS}: icon roles share the {SOURCE_MODE} colour {} (`{}` and another)", design_tokens::hex_of(c), other));
        }
    }
    let map = |c: &str| -> Result<String, String> {
        let rgba = design_tokens::hex(c).ok_or_else(|| format!("colour `{c}`"))?;
        let opaque = design_tokens::hex_of([rgba[0], rgba[1], rgba[2], 255]);
        paints.get(&opaque).cloned().ok_or_else(|| format!("{opaque} is no icon role's {SOURCE_MODE} colour: bind it to an Icon variable"))
    };

    let mut lock = read_lock(root);
    let mut problems = Vec::new();
    for (name, _) in &icons {
        let exported = match std::fs::read_to_string(out.join(format!("{name}.svg"))) {
            Ok(t) => t,
            Err(e) => {
                problems.push(format!("icon {name}: {e}"));
                continue;
            }
        };
        let master_path = root.join(ICONS_DIR).join(format!("{name}.svg"));
        let old = std::fs::read_to_string(&master_path).ok();
        let title = old.as_deref().and_then(title_of).unwrap_or_else(|| name.replace(['_', '-'], " "));
        let master = match svg_norm::normalise(&exported, &title, &map) {
            Ok(m) => m,
            Err(e) => {
                problems.push(format!("icon {name}: {e}"));
                continue;
            }
        };
        let fp = svg_norm::fingerprint(&master);
        if write_icons && lock.get(name) != Some(&fp) {
            if let Err(e) = std::fs::write(&master_path, &master) {
                problems.push(format!("{}: {e}", master_path.display()));
                continue;
            }
            eprintln!("  wrote {ICONS_DIR}/{name}.svg");
            if old.is_none() {
                eprintln!("  note: icon {name} is new: add it to MASTERS in crates/ui-egui/src/icons.rs and its row to ATTRIBUTION.md");
            }
        }
        lock.insert(name.clone(), fp);
    }
    for name in lock.keys().filter(|n| !icons.iter().any(|(i, _)| i == *n)).cloned().collect::<Vec<_>>() {
        eprintln!("  note: the design file has no icon/{name} any more; {ICONS_DIR}/{name}.svg is left as it is");
        lock.remove(&name);
    }
    if !problems.is_empty() {
        return Err(problems.join("\n  "));
    }
    write_lock(root, &lock)
}
