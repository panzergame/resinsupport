use serde_json::{Value, json};

use super::{ids_param, select_ids, vec3_param};
use crate::{EngineError, Result, Session};
use resin_geom::Vec3;

/// How far off its model's bounds a tip may be: the surface can be anywhere inside them.
const BOUNDS_TOLERANCE: f64 = 0.01;

fn check_known(s: &Session, ids: &[u64]) -> Result<()> {
    match ids.iter().find(|id| !s.scene.supports().iter().any(|t| t.id == **id)) {
        Some(id) => Err(EngineError::BadParams(format!("no support {id}"))),
        None => Ok(()),
    }
}

pub(crate) fn add(s: &mut Session, p: &Value) -> Result<Value> {
    let model = p.get("model").and_then(Value::as_u64).ok_or_else(|| EngineError::BadParams("`model` must be a model id".into()))?;
    let point = vec3_param(p, "point")?.ok_or_else(|| EngineError::BadParams("`point` is required".into()))?;
    let m = s.scene.model(model).ok_or_else(|| EngineError::BadParams(format!("no model {model}")))?;
    let mut reach = m.bounds;
    let pad = Vec3::new(BOUNDS_TOLERANCE, BOUNDS_TOLERANCE, BOUNDS_TOLERANCE);
    reach.min -= pad;
    reach.max += pad;
    if !reach.contains(point) {
        return Err(EngineError::BadParams(format!("the point is off model {model}")));
    }
    let normal = match vec3_param(p, "normal")? {
        Some(n) => n.normalized().ok_or_else(|| EngineError::BadParams("`normal` must not be zero".into()))?,
        None => -Vec3::Z,
    };
    let id = s.scene.add_support(model, point, normal)?;
    Ok(json!({ "id": id }))
}

pub(crate) fn delete(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = match ids_param(p, "support")? {
        Some(ids) => {
            check_known(s, &ids)?;
            ids
        }
        None => s.scene.selected.iter().copied().collect(),
    };
    s.scene.remove_supports(&ids);
    Ok(json!({ "removed": ids }))
}

pub(crate) fn list(s: &mut Session, _: &Value) -> Result<Value> {
    let supports: Vec<Value> = s
        .scene
        .supports()
        .iter()
        .map(|t| json!({ "id": t.id, "model": t.model, "tip": t.tip, "normal": t.normal, "selected": s.scene.selected.contains(&t.id) }))
        .collect();
    Ok(json!({ "supports": supports }))
}

pub(crate) fn select(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = ids_param(p, "support")?.ok_or_else(|| EngineError::BadParams("`ids` is required".into()))?;
    check_known(s, &ids)?;
    select_ids(&mut s.scene.selected, ids, p)?;
    Ok(json!({ "selected": s.scene.selected }))
}
