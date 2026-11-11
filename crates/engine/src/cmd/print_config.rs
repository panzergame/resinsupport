//! `print.configure`, `print.reset`, `print.settings`: the print configuration on the session;
//! `print.slice`: the job sliced from the scene with it. None touches the scene, so these make no
//! undo step.

use resin_slice::{Cancel, Progress};
use serde_json::{Value, json};

use crate::{EngineError, Result, Session};

fn settings_value(s: &Session) -> Result<Value> {
    serde_json::to_value(&s.print_config).map_err(|e| EngineError::Internal(e.to_string()))
}

pub(crate) fn settings(s: &mut Session, _: &Value) -> Result<Value> {
    settings_value(s)
}

pub(crate) fn configure(s: &mut Session, p: &Value) -> Result<Value> {
    s.print_config.configure(p)?;
    settings_value(s)
}

pub(crate) fn reset(s: &mut Session, _: &Value) -> Result<Value> {
    s.print_config.reset();
    s.print_config.validate()?;
    settings_value(s)
}

/// Slice the scene now, on this thread, and keep the job on the session.
pub(crate) fn slice(s: &mut Session, _: &Value) -> Result<Value> {
    let (input, unreachable) = s.slice_input();
    let job = resin_slice::slice(&input, &Progress::default(), &Cancel::default()).map_err(|e| EngineError::Failed(e.to_string()))?;
    let st = job.stats;
    let out = json!({
        "layers": job.layers.len(),
        "height": job.height(),
        "volume_ml": st.volume_ml,
        "print_time_s": st.print_time_s,
        "clipped": st.clipped,
        "unreachable_supports": unreachable,
        "open_rows": st.open_rows,
    });
    if !s.adopt_job(job, unreachable) {
        return Err(EngineError::Internal("the scene changed while it was sliced".into()));
    }
    Ok(out)
}
