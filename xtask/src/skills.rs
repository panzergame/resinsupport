//! The agent skills (`skills/<name>/SKILL.md`) and the files that route to them stay honest:
//! every skill has its name and description, `AGENTS.md` and `skills/README.md` list every skill
//! and only real ones, and the paths and `cargo xtask` commands they cite exist.

use std::path::Path;

/// The tasks `main` knows.
pub const TASKS: &[&str] = &["ci", "crash-guards", "i18n", "layers", "lists", "skills"];

pub fn skills(root: &Path) -> Vec<String> {
    let mut problems = Vec::new();
    let dir = root.join("skills");
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .map(|rd| rd.filter_map(|e| e.ok()).filter(|e| e.path().is_dir()).map(|e| e.file_name().to_string_lossy().into_owned()).collect())
        .unwrap_or_default();
    names.sort();
    if names.is_empty() {
        problems.push("skills/: no skills".into());
    }
    let mut cited = vec![root.join("AGENTS.md"), dir.join("README.md")];
    for name in &names {
        let path = dir.join(name).join("SKILL.md");
        let Ok(text) = std::fs::read_to_string(&path) else {
            problems.push(format!("skills/{name}: no SKILL.md"));
            continue;
        };
        let front = text.strip_prefix("---\n").and_then(|t| t.split_once("\n---\n")).map(|(f, _)| f).unwrap_or_default();
        let field = |key: &str| front.lines().find_map(|l| l.strip_prefix(&format!("{key}: "))).map(|v| v.trim().trim_matches('"').to_string());
        if field("name").as_deref() != Some(name.as_str()) {
            problems.push(format!("skills/{name}/SKILL.md: its front matter's name must be `{name}`"));
        }
        if field("description").is_none_or(|d| d.len() < 40) {
            problems.push(format!("skills/{name}/SKILL.md: no description saying when to load it"));
        }
        cited.push(path);
    }
    for router in ["AGENTS.md", "skills/README.md"] {
        let text = std::fs::read_to_string(root.join(router)).unwrap_or_default();
        for name in &names {
            if !text.contains(&format!("{name}/SKILL.md")) {
                problems.push(format!("{router} does not route to skills/{name}"));
            }
        }
    }
    for path in cited {
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        let shown = path.strip_prefix(root).unwrap_or(&path).display().to_string();
        for (i, span) in text.split('`').enumerate() {
            if i % 2 == 0 {
                continue;
            }
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

/// A repository path: no spaces, placeholders or wildcards, and a slash or a file extension.
fn looks_like_path(s: &str) -> bool {
    let plain = !s.is_empty() && !s.contains([' ', '<', '>', '*', '{', '(', ':', '=', '|', '~', '$', '#', '@']) && !s.starts_with("http");
    let rooted = s.split('/').next().is_some_and(|first| ["apps", "assets", "crates", "docs", "skills", "xtask", ".claude"].contains(&first));
    let file = ["AGENTS.md", "ATTRIBUTION.md", "CLAUDE.md", "README.md", "ROADMAP.md", "Cargo.toml"].contains(&s);
    plain && (rooted || file)
}
