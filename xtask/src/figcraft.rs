//! Running FigCraft without a window: `figcraft-cli run` takes a script of commands and prints
//! one JSON result per step. The CLI is `FIGCRAFT_CLI` (`figcraft-cli`, or the FigCraft AppImage,
//! which runs it as `<AppImage> cli`), else `figcraft-cli` on the `PATH`.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

pub struct Cli {
    program: PathBuf,
    /// `cli` for the AppImage.
    prefix: Vec<String>,
}

impl Cli {
    pub fn find() -> Result<Cli, String> {
        let program = match std::env::var_os("FIGCRAFT_CLI") {
            Some(p) => PathBuf::from(p),
            None => std::env::var_os("PATH")
                .map(|paths| std::env::split_paths(&paths).map(|d| d.join("figcraft-cli")).find(|p| p.is_file()))
                .unwrap_or_default()
                .ok_or("figcraft-cli not found: set FIGCRAFT_CLI to it or to the FigCraft AppImage")?,
        };
        let appimage = program.extension().is_some_and(|e| e.eq_ignore_ascii_case("appimage"));
        Ok(Cli { program, prefix: if appimage { vec!["cli".into()] } else { Vec::new() } })
    }

    /// Run `script` (`{command, params, as?}` steps, one undo step), on `input` if given, saving
    /// to `output` if given. File access is confined to `read` and `write` (folders that exist).
    /// Each step's result, in order; the first failing step is an error naming it.
    pub fn run(
        &self,
        work: &Path,
        label: &str,
        script: &[Value],
        input: Option<&Path>,
        output: Option<&Path>,
        read: &[&Path],
        write: &[&Path],
    ) -> Result<Vec<Value>, String> {
        let path = work.join(format!("{label}.json"));
        let text = serde_json::to_string(script).map_err(|e| e.to_string())?;
        std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut cmd = Command::new(&self.program);
        cmd.args(&self.prefix).arg("run").arg(&path);
        if let Some(i) = input {
            cmd.arg("--in").arg(i);
        }
        if let Some(o) = output {
            cmd.arg("--out").arg(o);
        }
        for r in read {
            cmd.arg("--automation-read-root").arg(r);
        }
        for w in write {
            cmd.arg("--automation-write-root").arg(w);
        }
        let out = cmd.output().map_err(|e| format!("{}: {e}", self.program.display()))?;
        let stdout = String::from_utf8_lossy(&out.stdout);
        let mut results = Vec::with_capacity(script.len());
        for line in stdout.lines().filter(|l| l.starts_with('{')) {
            let Ok(step) = serde_json::from_str::<Value>(line) else { continue };
            if step["ok"] != true {
                let index = step["step"].as_u64().unwrap_or_default() as usize;
                let shown = script.get(index).map(|s| s.to_string()).unwrap_or_default();
                let shown: String = shown.chars().take(400).collect();
                return Err(format!("{label}: step {index} ({}) failed: {}\n  {shown}", step["command"], step["error"]));
            }
            results.push(step["result"].clone());
        }
        if !out.status.success() || results.len() != script.len() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            return Err(format!("{label}: figcraft-cli ran {} of {} steps: {}", results.len(), script.len(), stderr.trim()));
        }
        Ok(results)
    }
}

/// One script step.
pub fn step(command: &str, params: Value) -> Value {
    serde_json::json!({ "command": command, "params": params })
}

/// One script step whose result later steps name `{"$ref": name}`.
pub fn named(name: &str, command: &str, params: Value) -> Value {
    serde_json::json!({ "command": command, "params": params, "as": name })
}

pub fn reference(name: &str) -> Value {
    serde_json::json!({ "$ref": name })
}
