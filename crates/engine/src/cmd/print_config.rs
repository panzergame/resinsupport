//! `print.configure`, `print.reset`, `print.settings`: the print configuration on the session.
//! It never touches the scene, so these make no undo step.

use serde_json::Value;

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
