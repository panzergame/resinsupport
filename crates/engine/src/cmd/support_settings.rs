//! `support.configure`, `support.reset_preset`, `support.settings`: the support settings on the
//! session. They never touch the scene, so they make no undo step.

use serde_json::Value;

use crate::support_settings::preset_param;
use crate::{EngineError, Result, Session};

fn settings_value(s: &Session) -> Result<Value> {
    serde_json::to_value(s.support_settings).map_err(|e| EngineError::Internal(e.to_string()))
}

pub(crate) fn settings(s: &mut Session, _: &Value) -> Result<Value> {
    settings_value(s)
}

pub(crate) fn configure(s: &mut Session, p: &Value) -> Result<Value> {
    s.support_settings.configure(p)?;
    settings_value(s)
}

pub(crate) fn reset_preset(s: &mut Session, p: &Value) -> Result<Value> {
    let id = preset_param(p, "preset")?.unwrap_or(s.support_settings.active);
    s.support_settings.reset(id);
    settings_value(s)
}
