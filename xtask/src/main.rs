//! Repository checks. `cargo xtask ci` runs everything a change must pass.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod i18n;
mod skills;

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

/// Each library crate's layer. A crate may depend only on crates in lower layers.
const LAYERS: &[(&str, u8)] =
    &[("resin-geom", 0), ("resin-render", 1), ("resin-slice", 2), ("resin-io", 3), ("resin-engine", 4), ("resin-ui-egui", 5)];
/// Front-end crates: only layer 5 and the apps may use them, so the UI stays swappable.
const UI_ONLY: &[&str] = &["eframe", "egui", "egui-wgpu", "rfd", "wgpu", "winit"];
const CRASH_GUARD: &str =
    "#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let task = args.first().cloned().unwrap_or_default();
    let flag = |f: &str| args.iter().any(|a| a == f);
    let root = match workspace_root() {
        Some(r) => r,
        None => {
            eprintln!("xtask: cannot find the workspace root");
            return ExitCode::FAILURE;
        }
    };
    let ok = match task.as_str() {
        "ci" => ci(&root),
        "layers" => report("layers", layers(&root)),
        "crash-guards" => report("crash-guards", crash_guards(&root)),
        "lists" => report("lists", i18n::lists(&root, flag("--fix"))),
        "i18n" => report("i18n", i18n::i18n(&root, flag("--check"), flag("--add-missing"))),
        "skills" => report("skills", skills::skills(&root)),
        _ => {
            eprintln!("usage: cargo xtask <{}> (lists --fix, i18n --check | --add-missing)", skills::TASKS.join(" | "));
            false
        }
    };
    if ok { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}

fn workspace_root() -> Option<PathBuf> {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().map(Path::to_path_buf)
}

fn ci(root: &Path) -> bool {
    let steps: &[(&str, &[&str])] = &[
        ("fmt", &["fmt", "--all", "--", "--check"]),
        ("clippy", &["clippy", "--workspace", "--all-targets", "--", "-D", "warnings"]),
        ("test", &["test", "--workspace"]),
    ];
    report("layers", layers(root))
        && report("crash-guards", crash_guards(root))
        && report("lists", i18n::lists(root, false))
        && report("i18n", i18n::i18n(root, true, false))
        && report("skills", skills::skills(root))
        && steps.iter().all(|(name, args)| {
            eprintln!("== {name}");
            let ok = Command::new(env!("CARGO")).args(*args).current_dir(root).status().is_ok_and(|s| s.success());
            if !ok {
                eprintln!("xtask: {name} failed");
            }
            ok
        })
}

fn report(name: &str, problems: Vec<String>) -> bool {
    eprintln!("== {name}");
    for p in &problems {
        eprintln!("  {p}");
    }
    problems.is_empty()
}

fn crate_dirs(root: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = ["crates", "apps"]
        .iter()
        .filter_map(|d| std::fs::read_dir(root.join(d)).ok())
        .flatten()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.join("Cargo.toml").is_file())
        .collect();
    dirs.sort();
    dirs
}

/// The package name and the names under `[dependencies]` (dev-dependencies don't count).
fn manifest(path: &Path) -> Option<(String, Vec<String>)> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut name = None;
    let mut deps = Vec::new();
    let mut section = String::new();
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            section = line.trim_matches(['[', ']']).to_string();
            continue;
        }
        let Some((key, value)) = line.split_once('=') else { continue };
        let key = key.trim();
        if section == "package" && key == "name" {
            name = Some(value.trim().trim_matches('"').to_string());
        } else if section == "dependencies" || (section.starts_with("target.") && section.ends_with(".dependencies")) {
            deps.push(key.to_string());
        }
    }
    Some((name?, deps))
}

fn layers(root: &Path) -> Vec<String> {
    let layer = |n: &str| LAYERS.iter().find(|(c, _)| *c == n).map(|(_, l)| *l);
    let mut problems = Vec::new();
    for dir in crate_dirs(root) {
        let Some((name, deps)) = manifest(&dir.join("Cargo.toml")) else {
            problems.push(format!("{}: unreadable Cargo.toml", dir.display()));
            continue;
        };
        // Apps sit above every layer.
        let Some(own) = layer(&name) else { continue };
        for d in &deps {
            if let Some(l) = layer(d)
                && l >= own
            {
                problems.push(format!("{name} (layer {own}) depends on {d} (layer {l})"));
            }
            if own < 5 && UI_ONLY.contains(&d.as_str()) {
                problems.push(format!("{name} (layer {own}) depends on the front-end crate {d}"));
            }
        }
    }
    problems
}

/// Every non-test crate root denies the panicking shortcuts.
fn crash_guards(root: &Path) -> Vec<String> {
    let mut roots: Vec<PathBuf> =
        crate_dirs(root).into_iter().flat_map(|d| [d.join("src/lib.rs"), d.join("src/main.rs")]).filter(|p| p.is_file()).collect();
    roots.push(root.join("xtask/src/main.rs"));
    roots
        .into_iter()
        .filter(|p| {
            let text = std::fs::read_to_string(p).unwrap_or_default();
            !text.contains(CRASH_GUARD)
        })
        .map(|p| format!("{} lacks `{CRASH_GUARD}`", p.display()))
        .collect()
}
