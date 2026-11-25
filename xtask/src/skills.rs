//! `cargo xtask skills`: keeps the router (`AGENTS.md`) and the skill folders honest, so an agent
//! is never sent to a file, command or skill that is gone. [`SKILL_DIRS`]: `.agents/skills/` holds
//! the skills for working on the code; `skills/` is kept for skills that agents using the app
//! would load (none yet), and may be missing.
//!
//! - Every path an `AGENTS.md`, README or SKILL.md names in backticks exists, from the repository
//!   root or from the file's own folder.
//! - Every `cargo xtask <name>` names a task of this tool.
//! - The router table in `AGENTS.md` lists exactly the skills that have a SKILL.md, each folder's
//!   README table lists exactly its own skills, a skill name is in one folder only, and each
//!   SKILL.md has a front matter `name` equal to its folder and a `description`.
//! - `AGENTS.md` stays a router (at most [`ROUTER_MAX_CHARS`]) and a skill stays short (at most
//!   [`SKILL_MAX_CHARS`]); the skills in [`LONG_SKILLS`] predate the cap.
//! - `.claude/skills` is the relative symlink [`DISCOVERY_LINK`], so a coding assistant that looks
//!   in either place finds the same skills. A checkout without symlinks has a text file holding
//!   the target instead: that passes.
//!
//! Adapted from SolidCraft's `xtask/src/skills.rs` (MIT OR Apache-2.0) at `cd1d2449b`; see
//! ATTRIBUTION.md.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// The tasks `main` knows.
pub const TASKS: &[&str] = &["ci", "crash-guards", "design-export", "design-pull", "design-tokens", "fast", "i18n", "layers", "lists", "skills"];
/// The skill folders: the dev skills, then the in-app ones.
const SKILL_DIRS: &[&str] = &[".agents/skills", "skills"];
/// The discovery link and its target, relative to the link's folder.
const DISCOVERY_LINK: (&str, &str) = (".claude/skills", "../.agents/skills");
/// The router lists every skill on its own line with when to load it, so it gets more room.
const ROUTER_MAX_CHARS: usize = 5600;
const SKILL_MAX_CHARS: usize = 3600;
/// Skills written before the size cap.
const LONG_SKILLS: &[&str] = &["resinsupport-commands", "resinsupport-ui"];

pub fn skills(root: &Path) -> Vec<String> {
    let mut problems = Vec::new();
    problems.extend(discovery_link(root));

    // Skill name -> its folder.
    let mut found: BTreeMap<String, &str> = BTreeMap::new();
    let mut cited = vec![root.join("AGENTS.md")];
    for dir in SKILL_DIRS {
        let base = root.join(dir);
        if !base.is_dir() {
            continue;
        }
        let mut names: Vec<String> = std::fs::read_dir(&base)
            .map(|rd| rd.filter_map(|e| e.ok()).filter(|e| e.path().is_dir()).map(|e| e.file_name().to_string_lossy().into_owned()).collect())
            .unwrap_or_default();
        names.sort();
        let readme = base.join("README.md");
        let readme_text = std::fs::read_to_string(&readme).unwrap_or_default();
        if readme_text.is_empty() {
            problems.push(format!("{dir}/README.md: missing"));
        }
        let listed = readme_skills(&readme_text);
        cited.push(readme);
        for name in &names {
            let path = base.join(name).join("SKILL.md");
            let Ok(text) = std::fs::read_to_string(&path) else {
                problems.push(format!("{dir}/{name}: no SKILL.md"));
                continue;
            };
            match front_matter(&text) {
                Some((n, _)) if n != *name => problems.push(format!("{dir}/{name}/SKILL.md: its front matter's name must be `{name}`")),
                Some((_, d)) if d.len() < 40 => problems.push(format!("{dir}/{name}/SKILL.md: no description saying when to load it")),
                None => problems.push(format!("{dir}/{name}/SKILL.md: no front matter with a name and a description")),
                Some(_) => {}
            }
            if text.len() > SKILL_MAX_CHARS && !LONG_SKILLS.contains(&name.as_str()) {
                problems.push(format!(
                    "{dir}/{name}/SKILL.md: {} characters, over {SKILL_MAX_CHARS}; move detail to a reference file beside it",
                    text.len()
                ));
            }
            if let Some(other) = found.insert(name.clone(), dir) {
                problems.push(format!("skill `{name}` is in both {other}/ and {dir}/"));
            }
            if !listed.contains(name) {
                problems.push(format!("{dir}/README.md does not list {name}"));
            }
            cited.push(path);
        }
        for name in listed.difference(&names.iter().cloned().collect()) {
            problems.push(format!("{dir}/README.md lists {name}, which has no folder"));
        }
    }
    if found.is_empty() {
        problems.push(format!("{}: no skills", SKILL_DIRS.join(", ")));
    }

    let router = std::fs::read_to_string(root.join("AGENTS.md")).unwrap_or_default();
    if router.len() > ROUTER_MAX_CHARS {
        problems.push(format!("AGENTS.md: {} characters, over {ROUTER_MAX_CHARS}; it routes, the skills explain", router.len()));
    }
    let routed = router_skills(&router);
    for (name, dir) in &found {
        if !routed.contains(&format!("{dir}/{name}")) {
            problems.push(format!("AGENTS.md's table does not route to {dir}/{name}"));
        }
    }
    for skill in &routed {
        let known = skill.rsplit_once('/').is_some_and(|(dir, name)| found.get(name).is_some_and(|d| *d == dir));
        if !known {
            problems.push(format!("AGENTS.md's table routes to {skill}, which is not a skill"));
        }
    }

    for path in cited {
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        let shown = path.strip_prefix(root).unwrap_or(&path).display().to_string();
        for span in code_spans(&text) {
            if let Some(task) = span.strip_prefix("cargo xtask ") {
                let task = task.split_whitespace().next().unwrap_or_default();
                if !TASKS.contains(&task) {
                    problems.push(format!("{shown}: `cargo xtask {task}` does not exist"));
                }
            } else if looks_like_path(span)
                && !root.join(span.trim_end_matches('/')).exists()
                && !path.parent().is_some_and(|p| p.join(span).exists())
            {
                problems.push(format!("{shown}: `{span}` does not exist"));
            }
        }
    }
    problems
}

/// What is wrong with the `.claude/skills` link, if anything.
fn discovery_link(root: &Path) -> Option<String> {
    let (link, target) = DISCOVERY_LINK;
    let path = root.join(link);
    match std::fs::read_link(&path) {
        Ok(t) if t == Path::new(target) => None,
        Ok(t) => Some(format!("{link} points to {}, not {target}", t.display())),
        // Git writes a symlink as a text file holding its target when symlinks are off.
        Err(_) if std::fs::read_to_string(&path).is_ok_and(|t| t.trim() == target) => None,
        Err(_) => Some(format!("{link}: must be a symlink to {target} (`ln -s {target} {link}`)")),
    }
}

/// The backticked spans of `text`.
fn code_spans(text: &str) -> impl Iterator<Item = &str> {
    text.split('`').skip(1).step_by(2)
}

/// The skills a router table row names, as `<folder>/<name>`: `| … | `<folder>/<name>/SKILL.md` |`.
fn router_skills(text: &str) -> BTreeSet<String> {
    text.lines()
        .filter(|l| l.trim_start().starts_with('|'))
        .flat_map(code_spans)
        .filter_map(|s| s.strip_suffix("/SKILL.md"))
        .filter(|s| SKILL_DIRS.iter().any(|d| s.strip_prefix(d).is_some_and(|rest| rest.starts_with('/'))))
        .map(str::to_string)
        .collect()
}

/// The skills a README's table links to: `| [`name`](name/SKILL.md) | … |`.
fn readme_skills(text: &str) -> BTreeSet<String> {
    text.lines()
        .filter(|l| l.trim_start().starts_with('|'))
        .flat_map(|l| l.split("](").skip(1))
        .filter_map(|s| s.split_once("/SKILL.md)").map(|(name, _)| name.to_string()))
        .collect()
}

/// The `name` and `description` of a `---` front matter block.
fn front_matter(text: &str) -> Option<(String, String)> {
    let front = text.strip_prefix("---\n")?.split_once("\n---\n")?.0;
    let field = |key: &str| front.lines().find_map(|l| l.strip_prefix(&format!("{key}: "))).map(|v| v.trim().trim_matches('"').to_string());
    Some((field("name")?, field("description")?))
}

/// A repository path: no spaces, placeholders or wildcards, and a slash or a file extension.
fn looks_like_path(s: &str) -> bool {
    let plain = !s.is_empty() && !s.contains([' ', '<', '>', '*', '{', '(', ':', '=', '|', '~', '$', '#', '@']) && !s.starts_with("http");
    // A bare folder name is as likely a task or a word (`skills`, `cargo xtask lists`).
    let rooted = s.contains('/')
        && s.split('/').next().is_some_and(|first| {
            ["apps", "assets", "crates", "design", "docs", "licenses", "skills", "xtask", ".agents", ".claude"].contains(&first)
        });
    let file = ["AGENTS.md", "ATTRIBUTION.md", "CLAUDE.md", "CONTRIBUTING.md", "README.md", "ROADMAP.md", "Cargo.toml"].contains(&s);
    plain && (rooted || file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_router_table_names_skills_in_either_folder_but_not_other_files() {
        let text = "| A | `.agents/skills/resinsupport-ui/SKILL.md`, Presets |\n\
                    | B | `skills/resinsupport-print/SKILL.md` |\n\
                    | C | `crates/render/src/camera.rs` |\n\
                    Prose `.agents/skills/resinsupport-tests/SKILL.md` outside a table.";
        let got: Vec<String> = router_skills(text).into_iter().collect();
        assert_eq!(got, [".agents/skills/resinsupport-ui", "skills/resinsupport-print"]);
    }

    #[test]
    fn a_readme_table_names_its_linked_skills() {
        let text = "| Skill | When |\n|---|---|\n| [`resinsupport-ui`](resinsupport-ui/SKILL.md) | a dialog |\nSee [the router](../../AGENTS.md).";
        assert_eq!(readme_skills(text).into_iter().collect::<Vec<_>>(), ["resinsupport-ui"]);
    }

    #[test]
    fn front_matter_needs_both_fields() {
        assert_eq!(front_matter("---\nname: a\ndescription: \"b\"\n---\n# A"), Some(("a".into(), "b".into())));
        assert_eq!(front_matter("---\nname: a\n---\n# A"), None);
        assert_eq!(front_matter("# A"), None);
    }

    #[test]
    fn paths_are_told_from_identifiers_and_placeholders() {
        assert!(looks_like_path(".agents/skills/resinsupport-ui/SKILL.md"));
        assert!(looks_like_path("crates/io/src/stl.rs"));
        assert!(looks_like_path("CONTRIBUTING.md"));
        assert!(!looks_like_path("crates/io/src/<format>.rs"));
        assert!(!looks_like_path("solidcraft:crates/ui-kit/src/"));
        assert!(!looks_like_path("ui_kit::Card"));
        assert!(!looks_like_path("skills"));
    }
}
