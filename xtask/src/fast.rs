//! `cargo xtask fast`: the check before a commit. It runs what `cargo xtask ci` runs, but tests
//! only the packages the change touches; `ci` stays the full gate.
//!
//! 1. The packages to test are the ones owning a file that differs from the base (the merge base
//!    with `origin/master`, else `master`), committed, uncommitted or untracked. Packages that
//!    depend on them are left out: clippy over the workspace still fails a change that breaks
//!    them. A workspace manifest, the lockfile or a lint config selects every package.
//! 2. Format, the repository checks and clippy, as `ci` runs them.
//! 3. The selected packages' tests.
//!
//! Every step runs even when one fails: all failures are listed at the end.
//!
//! The idea and its shape come from SolidCraft's `xtask/src/fast.rs` (MIT OR Apache-2.0) at
//! `cd1d2449b`; see ATTRIBUTION.md.

use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

const USAGE: &str = "usage: cargo xtask fast [--base REF] [-p PACKAGE]... [--no-tests] [--list] [-- TEST ARGS]";
/// Files whose change can break any package.
const EVERY_PACKAGE: &[&str] = &["Cargo.toml", "Cargo.lock", "clippy.toml", "rustfmt.toml", ".cargo/config.toml"];
/// Folders outside a crate that a crate embeds, and the packages that embed them.
const EMBEDDED: &[(&str, &[&str])] = &[("assets/fonts/", &["resin-geom"]), ("assets/", &["resin-ui-egui"])];
/// `resin-ui-egui` tested alone builds winit without the window-system features the app enables,
/// and fails to compile: it is always tested with the app.
const TESTED_WITH: &[(&str, &str)] = &[("resin-ui-egui", "resinsupport")];

#[derive(Debug, Default, PartialEq)]
struct Options {
    base: Option<String>,
    packages: Vec<String>,
    no_tests: bool,
    /// Print what would be tested, and stop.
    list: bool,
    test_args: Vec<String>,
}

fn parse(args: &[String]) -> Result<Options, String> {
    let mut o = Options::default();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--base" => o.base = Some(it.next().ok_or(USAGE)?.clone()),
            "-p" | "--package" => o.packages.push(it.next().ok_or(USAGE)?.clone()),
            "--no-tests" => o.no_tests = true,
            "--list" => o.list = true,
            "--" => o.test_args.extend(it.by_ref().cloned()),
            _ => return Err(format!("unknown argument `{a}`\n{USAGE}")),
        }
    }
    Ok(o)
}

pub fn fast(root: &Path, args: &[String]) -> bool {
    let options = match parse(args) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("{e}");
            return false;
        }
    };
    let packages = super::crate_dirs(root)
        .into_iter()
        .chain([root.join("xtask")])
        .filter_map(|dir| {
            let (name, _) = super::manifest(&dir.join("Cargo.toml"))?;
            Some((dir.strip_prefix(root).ok()?.to_string_lossy().replace('\\', "/") + "/", name))
        })
        .collect::<Vec<_>>();
    let selected = if options.packages.is_empty() {
        let paths = match changed_paths(root, options.base.as_deref()) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("xtask fast: {e}");
                return false;
            }
        };
        owners(&paths, &packages)
    } else {
        let known: BTreeSet<&str> = packages.iter().map(|(_, n)| n.as_str()).collect();
        if let Some(unknown) = options.packages.iter().find(|p| !known.contains(p.as_str())) {
            eprintln!("xtask fast: no package `{unknown}`");
            return false;
        }
        with_companions(options.packages.iter().cloned().collect())
    };
    eprintln!(
        "== fast: testing {}",
        if selected.is_empty() { "nothing (no package changed)".to_string() } else { selected.iter().cloned().collect::<Vec<_>>().join(", ") }
    );
    if options.list {
        return true;
    }

    let mut failed: Vec<&str> = Vec::new();
    let mut step = |name: &'static str, ok: bool| {
        if !ok {
            failed.push(name);
        }
    };
    step("fmt", cargo(root, &["fmt", "--all", "--", "--check"]));
    step("layers", super::report("layers", super::layers(root)));
    step("crash-guards", super::report("crash-guards", super::crash_guards(root)));
    step("lists", super::report("lists", crate::i18n::lists(root, false)));
    step("i18n", super::report("i18n", crate::i18n::i18n(root, true, false)));
    step("skills", super::report("skills", crate::skills::skills(root)));
    step("design-tokens", super::report("design-tokens", crate::design_tokens::design_tokens(root, true)));
    step("clippy", cargo(root, &["clippy", "--workspace", "--all-targets", "--", "-D", "warnings"]));
    if !options.no_tests && !selected.is_empty() {
        let mut args: Vec<String> = vec!["test".into()];
        for p in &selected {
            args.extend(["-p".into(), p.clone()]);
        }
        if !options.test_args.is_empty() {
            args.push("--".into());
            args.extend(options.test_args.iter().cloned());
        }
        step("test", cargo(root, &args.iter().map(String::as_str).collect::<Vec<_>>()));
    }

    if failed.is_empty() {
        eprintln!("== fast: passed (`cargo xtask ci` tests every package)");
        true
    } else {
        eprintln!("== fast: failed: {}", failed.join(", "));
        false
    }
}

fn cargo(root: &Path, args: &[&str]) -> bool {
    eprintln!("== cargo {}", args.join(" "));
    Command::new(env!("CARGO")).args(args).current_dir(root).status().is_ok_and(|s| s.success())
}

/// Every path that differs from the base: committed since it, uncommitted, or untracked.
fn changed_paths(root: &Path, base: Option<&str>) -> Result<Vec<String>, String> {
    let git = |args: &[&str]| -> Result<String, String> {
        let out = Command::new("git").args(args).current_dir(root).output().map_err(|e| format!("git: {e}"))?;
        if !out.status.success() {
            return Err(format!("git {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim()));
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    };
    let base = match base {
        Some(b) => b.to_string(),
        None => ["origin/master", "master"]
            .iter()
            .find(|b| git(&["rev-parse", "--verify", "--quiet", b]).is_ok())
            .ok_or("no `origin/master` or `master` to compare with; pass --base")?
            .to_string(),
    };
    let merge_base = git(&["merge-base", "HEAD", &base])?.trim().to_string();
    let mut paths = split_nul(&git(&["diff", "--name-only", "-z", &merge_base])?);
    paths.extend(split_nul(&git(&["ls-files", "--others", "--exclude-standard", "-z"])?));
    paths.sort();
    paths.dedup();
    Ok(paths)
}

fn split_nul(text: &str) -> Vec<String> {
    text.split('\0').filter(|s| !s.is_empty()).map(str::to_string).collect()
}

/// The packages owning `paths`, given each package's folder (ending in `/`) and name.
fn owners(paths: &[String], packages: &[(String, String)]) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for path in paths {
        if EVERY_PACKAGE.contains(&path.as_str()) {
            return with_companions(packages.iter().map(|(_, n)| n.clone()).collect());
        }
        if let Some((_, name)) = packages.iter().find(|(dir, _)| path.starts_with(dir.as_str())) {
            out.insert(name.clone());
        }
        if let Some((_, embedders)) = EMBEDDED.iter().find(|(dir, _)| path.starts_with(dir)) {
            out.extend(embedders.iter().map(|n| n.to_string()));
        }
    }
    with_companions(out)
}

fn with_companions(mut packages: BTreeSet<String>) -> BTreeSet<String> {
    for (alone, with) in TESTED_WITH {
        if packages.contains(*alone) {
            packages.insert(with.to_string());
        }
    }
    packages
}

#[cfg(test)]
mod tests {
    use super::*;

    fn packages() -> Vec<(String, String)> {
        [("crates/geom/", "resin-geom"), ("crates/ui-egui/", "resin-ui-egui"), ("apps/resinsupport/", "resinsupport"), ("xtask/", "xtask")]
            .iter()
            .map(|(d, n)| (d.to_string(), n.to_string()))
            .collect()
    }

    fn owned(paths: &[&str]) -> Vec<String> {
        owners(&paths.iter().map(|p| p.to_string()).collect::<Vec<_>>(), &packages()).into_iter().collect()
    }

    #[test]
    fn a_path_selects_its_package_and_docs_select_nothing() {
        assert_eq!(owned(&["crates/geom/src/bvh.rs", "docs/architecture.md", ".agents/skills/README.md"]), ["resin-geom"]);
        assert_eq!(owned(&["xtask/src/fast.rs"]), ["xtask"]);
    }

    #[test]
    fn the_ui_crate_is_tested_with_the_app() {
        assert_eq!(owned(&["crates/ui-egui/src/toolbar.rs"]), ["resin-ui-egui", "resinsupport"]);
    }

    #[test]
    fn embedded_assets_select_the_crates_that_embed_them() {
        assert_eq!(owned(&["assets/fonts/liberation-sans/OFL.txt"]), ["resin-geom"]);
        assert_eq!(owned(&["assets/locales/en.json"]), ["resin-ui-egui", "resinsupport"]);
    }

    #[test]
    fn the_lockfile_selects_every_package() {
        assert_eq!(owned(&["Cargo.lock"]).len(), packages().len());
    }

    #[test]
    fn arguments_parse_and_unknown_ones_are_refused() {
        let args: Vec<String> = ["--base", "HEAD~2", "-p", "resin-geom", "--no-tests", "--", "bvh"].iter().map(|s| s.to_string()).collect();
        let o = parse(&args).unwrap();
        assert_eq!(o.base.as_deref(), Some("HEAD~2"));
        assert_eq!(o.packages, ["resin-geom"]);
        assert!(o.no_tests && !o.list);
        assert_eq!(o.test_args, ["bvh"]);
        assert!(parse(&["--nope".to_string()]).is_err());
    }
}
