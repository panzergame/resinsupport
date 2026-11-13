use serde_json::{Value, json};

use std::collections::{BTreeMap, BTreeSet};

use super::{ids_param, num_param, select_ids, vec3_param};
use crate::session::MAX_EXTENT;
use crate::support_line::{DEFAULT_TIPS_PER_BODY, MAX_LINE_TIPS, line_points};
use crate::{
    BracePairing, BraceParams, BracePattern, DEFAULT_MAX_DISTANCE, EngineError, MAX_BRACES, MAX_DISTANCE_RANGE, Result, Route, Session, SupportPart,
    brace_pairs, brace_solids, route,
};
use resin_geom::Vec3;

/// How far off its model's bounds a tip may be: the surface can be anywhere inside them.
const BOUNDS_TOLERANCE: f64 = 0.01;

fn check_known(s: &Session, ids: &[u64]) -> Result<()> {
    match ids.iter().find(|id| !s.scene.supports().iter().any(|t| t.id == **id)) {
        Some(id) => Err(EngineError::BadParams(format!("no support {id}"))),
        None => Ok(()),
    }
}

/// `model`, `point` on it and outward `normal` (default down), as `support.add` and
/// `support.move_tip` take them.
fn surface_params(s: &Session, p: &Value) -> Result<(u64, Vec3, Vec3)> {
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
    Ok((model, point, normal))
}

pub(crate) fn add(s: &mut Session, p: &Value) -> Result<Value> {
    let (model, point, normal) = surface_params(s, p)?;
    let parent = match p.get("parent") {
        None | Some(Value::Null) => None,
        Some(v) => Some(v.as_u64().ok_or_else(|| EngineError::BadParams("`parent` must be a support id".into()))?),
    };
    let id = s.scene.add_support(model, point, normal, parent)?;
    Ok(json!({ "id": id }))
}

/// Tips between support `from` and `to` (a support, or a surface where a new support goes first).
/// Ends sharing one body give it every new tip; otherwise the new tips go in runs of `per_body`
/// along the line, each run's middle tip a new support and the others its branches, so no tip
/// reaches far sideways to its head. Points that land on no surface are skipped, and end a run.
pub(crate) fn add_line(s: &mut Session, p: &Value) -> Result<Value> {
    let from = p.get("from").and_then(Value::as_u64).ok_or_else(|| EngineError::BadParams("`from` must be a support id".into()))?;
    check_known(s, &[from])?;
    let spacing = num_param(p, "spacing")?.ok_or_else(|| EngineError::BadParams("`spacing` is required".into()))?;
    let per_body = match p.get("per_body") {
        None | Some(Value::Null) => DEFAULT_TIPS_PER_BODY,
        Some(v) => v
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .filter(|n| (1..=MAX_LINE_TIPS).contains(n))
            .ok_or_else(|| EngineError::BadParams(format!("`per_body` must be a whole number from 1 to {MAX_LINE_TIPS}")))?,
    };
    let tip_of = |s: &Session, id: u64| s.scene.supports().iter().find(|t| t.id == id).map(|t| (t.tip, t.normal));
    let a = tip_of(s, from).ok_or_else(|| EngineError::BadParams(format!("no support {from}")))?;
    let to = p.get("to").ok_or_else(|| EngineError::BadParams("`to` is required".into()))?;
    let (b, surface) = match to.as_u64() {
        Some(id) if id == from => return Err(EngineError::BadParams("`from` and `to` must be two supports".into())),
        Some(id) => (tip_of(s, id).ok_or_else(|| EngineError::BadParams(format!("no support {id}")))?, None),
        None if to.is_object() => {
            let (model, point, normal) = surface_params(s, to)?;
            ((point, normal), Some(model))
        }
        None => return Err(EngineError::BadParams("`to` must be a support id or { model, point, normal? }".into())),
    };
    let points = line_points(&s.scene, a, b, spacing)
        .ok_or_else(|| EngineError::BadParams(format!("`spacing` must be positive and make at most {MAX_LINE_TIPS} tips")))?;
    let mut ids = Vec::new();
    // A new destination is a body of its own, never `from`'s.
    let to_root = to.as_u64().and_then(|id| s.scene.root_of(id));
    let hits: Vec<(u64, Vec3, Vec3)> = points.iter().filter_map(|q| q.hit).collect();
    let skipped = points.len() - hits.len();
    match s.scene.root_of(from).filter(|r| Some(*r) == to_root) {
        Some(root) => {
            for &(model, point, normal) in &hits {
                ids.push(s.scene.add_support(model, point, normal, Some(root))?);
            }
        }
        None => {
            // A point landing nowhere ends a run: a body must not reach across the gap it marks.
            let stretches = points.split(|q| q.hit.is_none()).map(|stretch| stretch.iter().filter_map(|q| q.hit).collect::<Vec<_>>());
            for stretch in stretches {
                for run in stretch.chunks(per_body) {
                    let mid = run.len() / 2;
                    let Some(&(model, point, normal)) = run.get(mid) else { continue };
                    // Before its branches, as a parent must be.
                    let root = s.scene.add_support(model, point, normal, None)?;
                    ids.push(root);
                    for (i, &(model, point, normal)) in run.iter().enumerate() {
                        if i != mid {
                            ids.push(s.scene.add_support(model, point, normal, Some(root))?);
                        }
                    }
                }
            }
        }
    }
    // Last, so the newest support is where the line ended and the next line can start there.
    if let Some(model) = surface {
        ids.push(s.scene.add_support(model, b.0, b.1, None)?);
    }
    Ok(json!({ "ids": ids, "skipped": skipped }))
}

pub(crate) fn move_tip(s: &mut Session, p: &Value) -> Result<Value> {
    let id = p.get("support").and_then(Value::as_u64).ok_or_else(|| EngineError::BadParams("`support` must be a support id".into()))?;
    check_known(s, &[id])?;
    let (model, point, normal) = surface_params(s, p)?;
    s.scene.set_tip(id, model, point, normal)?;
    Ok(json!({}))
}

/// Supports `ids` and braces `braces`; with neither, the selected ones of both. A brace goes with
/// either of its supports.
pub(crate) fn delete(s: &mut Session, p: &Value) -> Result<Value> {
    let (ids, braces) = (ids_param(p, "support")?, braces_param(p)?);
    if let Some(ids) = &ids {
        check_known(s, ids)?;
    }
    if let Some(bad) = braces.iter().flatten().find(|id| !s.scene.braces().iter().any(|b| b.id == **id)) {
        return Err(EngineError::BadParams(format!("no brace {bad}")));
    }
    let (ids, braces) = match (ids, braces) {
        (None, None) => (s.scene.selected.iter().copied().collect(), s.scene.selected_braces.iter().copied().collect()),
        (ids, braces) => (ids.unwrap_or_default(), braces.unwrap_or_default()),
    };
    let before: Vec<u64> = s.scene.supports().iter().map(|t| t.id).collect();
    let braces_before: Vec<u64> = s.scene.braces().iter().map(|b| b.id).collect();
    s.scene.remove_braces(&braces);
    let preset = *s.support_settings.current();
    s.scene.remove_supports(&ids, &preset);
    let removed: Vec<u64> = before.into_iter().filter(|id| !s.scene.supports().iter().any(|t| t.id == *id)).collect();
    let removed_braces: Vec<u64> = braces_before.into_iter().filter(|id| !s.scene.braces().iter().any(|b| b.id == *id)).collect();
    Ok(json!({ "removed": removed, "removed_braces": removed_braces }))
}

/// The settings a new brace takes, each defaulting to [`BraceParams::default`].
fn brace_params(p: &Value) -> Result<BraceParams> {
    let d = BraceParams::default();
    let pattern = match p.get("pattern") {
        None | Some(Value::Null) => d.pattern,
        Some(v) => v.as_str().and_then(BracePattern::parse).ok_or_else(|| EngineError::BadParams("`pattern` must be zigzag or cross".into()))?,
    };
    let params = BraceParams {
        pattern,
        angle: num_param(p, "angle")?.unwrap_or(d.angle),
        diameter_ratio: num_param(p, "diameter_ratio")?.unwrap_or(d.diameter_ratio),
        head_gap: num_param(p, "head_gap")?.unwrap_or(d.head_gap),
        foot_gap: num_param(p, "foot_gap")?.unwrap_or(d.foot_gap),
    };
    params.validate().map_err(EngineError::BadParams)?;
    Ok(params)
}

/// The supports a brace command acts on: those given, or the selected ones and those with
/// selected parts; each as the support whose body it shares.
fn brace_roots(s: &Session, p: &Value) -> Result<BTreeSet<u64>> {
    let ids: Vec<u64> = match ids_param(p, "support")? {
        Some(ids) => {
            check_known(s, &ids)?;
            ids
        }
        None => s.scene.selected.iter().copied().chain(s.scene.selected_parts.iter().map(|(id, _)| *id)).collect(),
    };
    Ok(ids.into_iter().filter_map(|id| s.scene.root_of(id)).collect())
}

/// Brace the supports' bodies, paired as `pairing` says, those standing at most `max_distance`
/// apart, in place of every brace already joining two of them: changing the pairing or a setting
/// and running it again rebuilds them. A pair with no strut to make (too close, too short, every
/// strut near a model) is skipped.
pub(crate) fn add_braces(s: &mut Session, p: &Value) -> Result<Value> {
    let roots = brace_roots(s, p)?;
    if roots.len() < 2 {
        return Err(EngineError::BadParams("select at least two supports to brace".into()));
    }
    let params = brace_params(p)?;
    let pairing = match p.get("pairing") {
        None | Some(Value::Null) => BracePairing::Tree,
        Some(v) => v.as_str().and_then(BracePairing::parse).ok_or_else(|| EngineError::BadParams("`pairing` must be chain, tree or all".into()))?,
    };
    let max_distance = num_param(p, "max_distance")?.unwrap_or(DEFAULT_MAX_DISTANCE);
    if !MAX_DISTANCE_RANGE.contains(&max_distance) {
        let r = MAX_DISTANCE_RANGE;
        return Err(EngineError::BadParams(format!("`max_distance` must be a number from {} to {}", r.start(), r.end())));
    }
    let preset = *s.support_settings.current();
    let scene = &s.scene;
    let routes: BTreeMap<u64, Route> = scene
        .supports()
        .iter()
        .filter(|t| roots.contains(&t.id))
        .map(|t| (t.id, route(scene, t, &preset)))
        .filter(|(_, r)| r.reachable() && r.foot)
        .collect();
    let feet: Vec<(u64, Vec3)> = routes.iter().filter_map(|(id, r)| r.body.last().map(|f| (*id, *f))).collect();
    let pairs = brace_pairs(&feet, pairing, max_distance);
    if pairs.len() > MAX_BRACES {
        return Err(EngineError::BadParams(format!("{} braces asked for, at most {MAX_BRACES} at once", pairs.len())));
    }
    let mut braced = Vec::with_capacity(pairs.len());
    let mut skipped = 0;
    for (a, b) in pairs {
        let (Some(ra), Some(rb)) = (routes.get(&a), routes.get(&b)) else { continue };
        if brace_solids(scene, ra, rb, &params, &preset).0.is_empty() {
            skipped += 1;
        } else {
            braced.push((a, b, params));
        }
    }
    let (ids, replaced) = s.scene.rebuild_braces(&roots, &braced)?;
    Ok(json!({ "ids": ids, "skipped": skipped, "replaced": replaced }))
}

/// Each support with its route under the preset in use: `path` runs from the contact through the
/// tip's upper end to the foot on the plate (a branch's stops at the head it shares), or stops at
/// the tip when the route has no body. `reachable`: the route reaches the plate, a branch's through its
/// parents, as the slicer prints it. The selected joints and segments are those the route still has.
pub(crate) fn list(s: &mut Session, _: &Value) -> Result<Value> {
    let preset = *s.support_settings.current();
    // A parent comes before its branches.
    let mut grounded = std::collections::BTreeSet::new();
    let supports: Vec<Value> = s
        .scene
        .supports()
        .iter()
        .map(|t| {
            let r = route(&s.scene, t, &preset);
            let reachable = r.reachable() && t.parent.is_none_or(|p| grounded.contains(&p));
            if reachable {
                grounded.insert(t.id);
            }
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
                "parent": t.parent,
                "tip": t.tip,
                "normal": t.normal,
                "selected": s.scene.selected.contains(&t.id),
                "path": path,
                "reachable": reachable,
                "edited": t.corners.is_some(),
                "fault": r.fault,
                "selected_joints": picked(|p| if let SupportPart::Joint(i) = p { Some(i) } else { None }),
                "selected_segments": picked(|p| if let SupportPart::Segment(i) = p { Some(i) } else { None }),
            })
        })
        .collect();
    let mut routes: BTreeMap<u64, Route> = BTreeMap::new();
    for b in s.scene.braces() {
        for id in [b.a, b.b] {
            if let (None, Some(t)) = (routes.get(&id), s.scene.supports().iter().find(|t| t.id == id)) {
                routes.insert(id, route(&s.scene, t, &preset));
            }
        }
    }
    let braces: Vec<Value> = s
        .scene
        .braces()
        .iter()
        .map(|b| {
            let struts = match (routes.get(&b.a), routes.get(&b.b)) {
                (Some(ra), Some(rb)) => brace_solids(&s.scene, ra, rb, &b.params, &preset).0,
                _ => Vec::new(),
            };
            let pr = b.params;
            json!({
                "id": b.id,
                "a": b.a,
                "b": b.b,
                "pattern": pr.pattern.id(),
                "angle": pr.angle,
                "diameter_ratio": pr.diameter_ratio,
                "head_gap": pr.head_gap,
                "foot_gap": pr.foot_gap,
                "selected": s.scene.selected_braces.contains(&b.id),
                "struts": struts.iter().map(|(a, b)| [a, b]).collect::<Vec<_>>(),
            })
        })
        .collect();
    Ok(json!({ "supports": supports, "braces": braces }))
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
                        let r = route(&s.scene, t, &preset);
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
    let braces = braces_param(p)?;
    if ids.is_none() && parts.is_none() && braces.is_none() {
        return Err(EngineError::BadParams("`ids`, `parts` or `braces` is required".into()));
    }
    let (ids, parts, braces) = (ids.unwrap_or_default(), parts.unwrap_or_default(), braces.unwrap_or_default());
    check_known(s, &ids)?;
    check_known(s, &parts.iter().map(|(id, _)| *id).collect::<Vec<_>>())?;
    if let Some(bad) = braces.iter().find(|id| !s.scene.braces().iter().any(|b| b.id == **id)) {
        return Err(EngineError::BadParams(format!("no brace {bad}")));
    }
    // One mode over every set: a plain click on a part replaces the supports selected too.
    select_ids(&mut s.scene.selected, ids, p)?;
    select_ids(&mut s.scene.selected_parts, parts, p)?;
    select_ids(&mut s.scene.selected_braces, braces, p)?;
    Ok(json!({
        "selected": s.scene.selected,
        "parts": s.scene.selected_parts.iter().map(|&(id, part)| part_json(id, part)).collect::<Vec<_>>(),
        "braces": s.scene.selected_braces,
    }))
}

/// The optional `braces`: brace ids.
fn braces_param(p: &Value) -> Result<Option<Vec<u64>>> {
    let bad = || EngineError::BadParams("`braces` must be an array of brace ids".into());
    match p.get("braces") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Array(a)) => a.iter().map(|v| v.as_u64().ok_or_else(bad)).collect::<Result<_>>().map(Some),
        Some(_) => Err(bad()),
    }
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
