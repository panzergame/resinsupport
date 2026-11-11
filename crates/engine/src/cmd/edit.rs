// Ported from SolidCraft (MIT OR Apache-2.0), crates/engine/src/cmd/edit.rs (`undo`, `redo`,
// `go_to`) at 4bdffef0; see ATTRIBUTION.md. Our steps hold the scene, not a document.
//! Undo and redo: swap the scene with the step on top of one stack, and push where it was onto
//! the other under the same label.

use serde_json::{Value, json};

use crate::session::{Clipboard, Snapshot};
use crate::{EngineError, Result, Session};

use super::model::{plate, targets};

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

/// `ids?` (default the selected models): keep them and their supports for `edit.paste`, in place
/// of what was copied before.
pub(crate) fn copy(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = targets(s, p)?;
    let project = s.scene.project_of(|id| ids.contains(&id));
    let lifted = s.scene.models().iter().filter(|m| ids.contains(&m.id)).map(|m| m.lifted).collect();
    let copied = project.models.len();
    s.clipboard = Some(Clipboard { project, lifted });
    Ok(json!({ "copied": copied }))
}

/// Add what was copied, arranged on the plate around the other models, and select it.
pub(crate) fn paste(s: &mut Session, _: &Value) -> Result<Value> {
    let clip = s.clipboard.clone().ok_or_else(|| EngineError::Failed("nothing to paste".into()))?;
    let ids = s.scene.append(clip.project)?;
    let lifted: Vec<_> = ids.iter().zip(&clip.lifted).filter_map(|(&id, &l)| s.scene.model(id).map(|m| (id, m.placement, Some(l)))).collect();
    s.scene.place(&lifted)?;
    let beside = s.scene.arrange(&ids, plate(s))?;
    s.scene.selected_models = ids.iter().copied().collect();
    Ok(json!({ "ids": ids, "beside": beside }))
}
