use serde_json::{Value, json};

use std::collections::BTreeMap;

use super::{ids_param, select_ids, vec3_param};
use crate::session::MAX_EXTENT;
use crate::{EngineError, Result, Session, SupportPart, route};
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

/// Each support with its route under the preset in use: `path` runs from the contact through the
/// tip's upper end to the foot on the plate, or stops at the tip when `reachable` is false. The
/// selected joints and segments are those the route still has.
pub(crate) fn list(s: &mut Session, _: &Value) -> Result<Value> {
    let preset = *s.support_settings.current();
    let models = s.scene.models();
    let supports: Vec<Value> = s
        .scene
        .supports()
        .iter()
        .map(|t| {
            let r = route(models, t, &preset);
            let path: Vec<Vec3> = [r.contact, r.head].into_iter().chain(r.body.iter().skip(1).copied()).collect();
            let picked = |pick: fn(SupportPart) -> Option<u32>| -> Vec<u32> {
                s.scene
                    .selected_parts
                    .range((t.id, SupportPart::Joint(0))..=(t.id, SupportPart::Segment(u32::MAX)))
                    .filter_map(|&(_, part)| part.corners(&r.body).and(pick(part)))
                    .collect()
            };
            json!({
                "id": t.id,
                "model": t.model,
                "tip": t.tip,
                "normal": t.normal,
                "selected": s.scene.selected.contains(&t.id),
                "path": path,
                "reachable": r.reachable(),
                "edited": t.corners.is_some(),
                "fault": r.fault,
                "selected_joints": picked(|p| if let SupportPart::Joint(i) = p { Some(i) } else { None }),
                "selected_segments": picked(|p| if let SupportPart::Segment(i) = p { Some(i) } else { None }),
            })
        })
        .collect();
    Ok(json!({ "supports": supports }))
}

/// Put joints where they are given. A support whose route was worked out first keeps that route
/// as its placed corners, so the joints keep their numbers and the rest of the path stays put.
pub(crate) fn place_joints(s: &mut Session, p: &Value) -> Result<Value> {
    let bad = || EngineError::BadParams("`joints` must be [{ support, joint, position: [x, y, z] }]".into());
    let list = p.get("joints").and_then(Value::as_array).ok_or_else(bad)?;
    let preset = *s.support_settings.current();
    let mut corners: BTreeMap<u64, Vec<Vec3>> = BTreeMap::new();
    for j in list {
        let id = j.get("support").and_then(Value::as_u64).ok_or_else(bad)?;
        let joint = j.get("joint").and_then(Value::as_u64).and_then(|n| usize::try_from(n).ok()).ok_or_else(bad)?;
        let at = vec3_param(j, "position")?.ok_or_else(bad)?;
        if at.z < 0.0 || ![at.x, at.y, at.z].iter().all(|v| v.abs() <= MAX_EXTENT) {
            return Err(EngineError::BadParams(format!("support {id}: a joint must be above the plate and within the {MAX_EXTENT} mm workspace")));
        }
        let list = match corners.entry(id) {
            std::collections::btree_map::Entry::Occupied(e) => e.into_mut(),
            std::collections::btree_map::Entry::Vacant(e) => {
                let t = s.scene.supports().iter().find(|t| t.id == id).ok_or_else(|| EngineError::BadParams(format!("no support {id}")))?;
                e.insert(match &t.corners {
                    Some(c) => c.clone(),
                    // The route as worked out; with no way down, the head alone, which the user
                    // can then move to where there is one.
                    None => {
                        let r = route(s.scene.models(), t, &preset);
                        match r.body.split_last() {
                            Some((_, c)) => c.to_vec(),
                            None => vec![r.head],
                        }
                    }
                })
            }
        };
        let slot = list.get_mut(joint).ok_or_else(|| EngineError::BadParams(format!("support {id} has no joint {joint}")))?;
        *slot = at;
    }
    s.scene.set_corners(corners.into_iter().map(|(id, c)| (id, Some(c))).collect())?;
    Ok(json!({}))
}

/// Give supports their worked-out route back.
pub(crate) fn reset_route(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = match ids_param(p, "support")? {
        Some(ids) => ids,
        None => {
            let scene = &s.scene;
            scene
                .selected
                .iter()
                .copied()
                .chain(scene.selected_parts.iter().map(|(id, _)| *id))
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect()
        }
    };
    s.scene.set_corners(ids.iter().map(|id| (*id, None)).collect())?;
    Ok(json!({ "reset": ids }))
}

pub(crate) fn select(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = ids_param(p, "support")?;
    let parts = parts_param(p)?;
    if ids.is_none() && parts.is_none() {
        return Err(EngineError::BadParams("`ids` or `parts` is required".into()));
    }
    let (ids, parts) = (ids.unwrap_or_default(), parts.unwrap_or_default());
    check_known(s, &ids)?;
    check_known(s, &parts.iter().map(|(id, _)| *id).collect::<Vec<_>>())?;
    // One mode over both sets: a plain click on a part replaces the supports selected too.
    select_ids(&mut s.scene.selected, ids, p)?;
    select_ids(&mut s.scene.selected_parts, parts, p)?;
    Ok(json!({ "selected": s.scene.selected, "parts": s.scene.selected_parts.iter().map(|&(id, part)| part_json(id, part)).collect::<Vec<_>>() }))
}

/// The optional `parts`: `[{ support, joint: i } | { support, segment: i }]`. The numbers are not
/// checked against the route: the one drawn may be a frame behind the one worked out here.
fn parts_param(p: &Value) -> Result<Option<Vec<(u64, SupportPart)>>> {
    let bad = || EngineError::BadParams("`parts` must be [{ support, joint | segment }] with whole numbers".into());
    let one = |v: &Value| -> Result<(u64, SupportPart)> {
        let id = v.get("support").and_then(Value::as_u64).ok_or_else(bad)?;
        let index = |k: &str| v.get(k).map(|n| n.as_u64().and_then(|n| u32::try_from(n).ok()).ok_or_else(bad)).transpose();
        match (index("joint")?, index("segment")?) {
            (Some(i), None) => Ok((id, SupportPart::Joint(i))),
            (None, Some(i)) => Ok((id, SupportPart::Segment(i))),
            _ => Err(bad()),
        }
    };
    match p.get("parts") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Array(a)) => a.iter().map(one).collect::<Result<_>>().map(Some),
        Some(_) => Err(bad()),
    }
}

fn part_json(id: u64, part: SupportPart) -> Value {
    match part {
        SupportPart::Joint(i) => json!({ "support": id, "joint": i }),
        SupportPart::Segment(i) => json!({ "support": id, "segment": i }),
    }
}
