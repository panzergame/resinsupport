//! Project files: open and save; printer files: export.

use serde_json::{Value, json};

use super::str_param;
use crate::{EngineError, Result, Session};

fn is_project_path(path: &std::path::Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case(resin_io::project::EXTENSION))
}

pub(crate) fn open(s: &mut Session, p: &Value) -> Result<Value> {
    let path = str_param(p, "path")?;
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (s, path);
        Err(EngineError::Failed("not supported yet: opening a path in the browser".into()))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let project = resin_io::read_project_file(std::path::Path::new(path)).map_err(|e| EngineError::Failed(format!("{path}: {e}")))?;
        let (models, supports) = (project.models.len(), project.supports.len());
        s.scene.adopt(project)?;
        // A project starts its own history: undo never brings back the scene it replaced.
        s.clear_history();
        if !s.scene.models().is_empty() {
            s.camera.fit(&s.scene.bounds());
        }
        s.mark_saved(path.to_owned());
        Ok(json!({ "path": path, "models": models, "supports": supports }))
    }
}

/// Write the job as a CTB v2 file, slicing first when the job no longer matches the scene.
pub(crate) fn export_ctb(s: &mut Session, p: &Value) -> Result<Value> {
    let path = str_param(p, "path")?.to_owned();
    if !resin_io::ctb::is_ctb_path(std::path::Path::new(&path)) {
        return Err(EngineError::BadParams(format!("a CTB file name ends in .{}: {path}", resin_io::ctb::EXTENSION)));
    }
    let key = match p.get("encryption_key") {
        None | Some(Value::Null) => resin_io::ctb::DEFAULT_KEY,
        Some(v) => v
            .as_u64()
            .and_then(|k| u32::try_from(k).ok())
            .ok_or_else(|| EngineError::BadParams("`encryption_key` must be an integer from 0 to 4294967295".into()))?,
    };
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (s, key);
        Err(EngineError::Failed("not supported yet: exporting to a path in the browser".into()))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        if !s.job_is_current() {
            super::print_config::slice(s, &Value::Null)?;
        }
        let job = s.job().map(|j| std::sync::Arc::clone(&j.sliced)).ok_or_else(|| EngineError::Internal("no job after slicing".into()))?;
        let opts = resin_io::CtbOptions { encryption_key: key, modified_minutes: resin_io::ctb::minutes_now() };
        let bytes = resin_io::write_ctb(&job, &resin_io::ctb::Previews::plain(&job), &opts)?;
        resin_io::write_atomic(std::path::Path::new(&path), &bytes, false).map_err(|e| EngineError::Failed(format!("{path}: {e}")))?;
        Ok(json!({ "path": path, "bytes": bytes.len(), "layers": job.layers.len() }))
    }
}

pub(crate) fn save(s: &mut Session, p: &Value) -> Result<Value> {
    let path = match p.get("path") {
        None | Some(Value::Null) => {
            s.file().map(str::to_owned).ok_or_else(|| EngineError::BadParams("the scene has no file yet: give a `path`".into()))?
        }
        Some(_) => str_param(p, "path")?.to_owned(),
    };
    // Saving over a model file someone imports from would lose it.
    if !is_project_path(std::path::Path::new(&path)) {
        return Err(EngineError::BadParams(format!("a project file name ends in .{}: {path}", resin_io::project::EXTENSION)));
    }
    let bytes = resin_io::write_project(&s.scene.project())?;
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (s, bytes);
        Err(EngineError::Failed("not supported yet: saving to a path in the browser".into()))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        resin_io::write_atomic(std::path::Path::new(&path), &bytes, true).map_err(|e| EngineError::Failed(format!("{path}: {e}")))?;
        s.mark_saved(path.clone());
        Ok(json!({ "path": path, "bytes": bytes.len() }))
    }
}
