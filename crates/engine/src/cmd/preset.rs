//! `<area>.preset.{delete,list,rename,save}` for every kind of preset ([`crate::presets`]): the
//! user's library of the kind on the session. None is an undo step: what save and rename do to
//! followers (linking the target, the new name) reaches the whole history
//! ([`crate::CommandSpec::across_history`]), so no undo takes it back.

use serde_json::{Value, json};

use super::model::refuse_unknown;
use super::str_param;
use crate::presets::{Follow, Preset};
use crate::{Result, Session};

pub(super) fn list<K: Follow>(s: &mut Session, p: &Value) -> Result<Value> {
    refuse_unknown(p, &[])?;
    Ok(json!({ "presets": K::library(s) }))
}

/// `name` and the kind's target: a new preset of the target's settings, the target then
/// following it.
pub(super) fn save<K: Follow>(s: &mut Session, p: &Value) -> Result<Value> {
    let known: Vec<&str> = ["name"].iter().chain(K::TARGET_PARAMS).copied().collect();
    refuse_unknown(p, &known)?;
    let name = str_param(p, "name")?;
    let target = K::target(s, p)?;
    let preset = Preset { name: name.into(), settings: K::settings(s, &target)? };
    K::library_mut(s).add(preset.clone())?;
    K::link(s, &target, &preset)?;
    Ok(json!({ "target": target, "preset": preset }))
}

/// `name`, `to`: the preset called `to`, and what followed it following it under that name.
pub(super) fn rename<K: Follow>(s: &mut Session, p: &Value) -> Result<Value> {
    refuse_unknown(p, &["name", "to"])?;
    let (old, to) = (str_param(p, "name")?, str_param(p, "to")?);
    let preset = K::library_mut(s).rename(old, to)?;
    let relinked = K::rename(s, old, &preset.name)?;
    Ok(json!({ "preset": preset, "relinked": relinked }))
}

/// `name`: the preset goes; what followed it keeps its settings and its name.
pub(super) fn delete<K: Follow>(s: &mut Session, p: &Value) -> Result<Value> {
    refuse_unknown(p, &["name"])?;
    let gone = K::library_mut(s).remove(str_param(p, "name")?)?;
    Ok(json!({ "preset": gone }))
}
