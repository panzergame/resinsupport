// Ported from SolidCraft (MIT OR Apache-2.0), crates/engine/src/cmd/edit.rs (`undo`, `redo`,
// `go_to`) at 4bdffef0; see ATTRIBUTION.md. Our steps hold the scene, not a document.
//! Undo and redo: swap the scene with the step on top of one stack, and push where it was onto
//! the other under the same label.

use serde_json::{Value, json};

use crate::session::Snapshot;
use crate::{EngineError, Result, Session};

pub(crate) fn undo(s: &mut Session, _: &Value) -> Result<Value> {
    s.end_step();
    let step = s.history.undo.pop().ok_or_else(|| EngineError::Failed("nothing to undo".into()))?;
    let was = go_to(s, step);
    let reply = json!({ "undone": was.label, "command": was.command });
    s.history.redo.push(was);
    Ok(reply)
}

pub(crate) fn redo(s: &mut Session, _: &Value) -> Result<Value> {
    s.end_step();
    let step = s.history.redo.pop().ok_or_else(|| EngineError::Failed("nothing to redo".into()))?;
    let was = go_to(s, step);
    let reply = json!({ "redone": was.label, "command": was.command });
    s.history.undo.push(was);
    Ok(reply)
}

/// Put the session at `step`; returns where it was, as the same step.
fn go_to(s: &mut Session, step: Snapshot) -> Snapshot {
    let Snapshot { label, command, serial, scene } = step;
    let was = s.scene.restore(scene);
    Snapshot { label, command, serial, scene: was }
}
