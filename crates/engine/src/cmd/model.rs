use serde_json::{Value, json};

use resin_geom::{Affine3, Label, LabelSide, Placement, Vec3};

use super::{ids_param, num_param, select_ids, str_param, vec3_param};
use resin_slice::{SLICING_KEYS, field};

use crate::{EngineError, LabelPresetKind, LabelSettings, Result, SCALE_RANGE, Session};

fn bool_param(p: &Value, key: &str) -> Result<Option<bool>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v.as_bool().map(Some).ok_or_else(|| EngineError::BadParams(format!("`{key}` must be a boolean"))),
    }
}

pub(crate) fn import_stl(s: &mut Session, p: &Value) -> Result<Value> {
    let path = std::path::Path::new(str_param(p, "path")?);
    let fit = bool_param(p, "fit")?.unwrap_or(true);
    let lifted = bool_param(p, "lifted")?.unwrap_or(false);
    let arrange = bool_param(p, "arrange")?.unwrap_or(false);
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (s, path, fit, lifted, arrange);
        Err(EngineError::Failed("not supported yet: importing from a path in the browser".into()))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let read = resin_io::read_stl_file(path)?;
        if read.mesh.is_empty() {
            return Err(EngineError::Failed(format!("{} has no usable triangles ({} dropped)", path.display(), read.dropped)));
        }
        let name = read
            .name
            .filter(|n| !n.trim().is_empty())
            .unwrap_or_else(|| path.file_stem().map_or_else(|| "model".into(), |s| s.to_string_lossy().into_owned()));
        let triangles = read.mesh.triangle_count();
        let id = s.scene.add(name.clone(), read.mesh);
        if lifted && let Some(at) = s.scene.model(id).map(|m| m.placement) {
            s.scene.place(&[(id, at, Some(true))])?;
        }
        let mut answer = json!({ "id": id, "name": name, "triangles": triangles, "dropped": read.dropped });
        if arrange {
            let beside = s.scene.arrange(&[id], plate(s), true)?;
            answer["on_plate"] = json!(beside.is_empty());
        }
        if fit {
            s.camera.fit(&s.scene.bounds());
        }
        Ok(answer)
    }
}

/// The build plate's width and depth, mm.
pub(super) fn plate(s: &Session) -> (f64, f64) {
    let printer = &s.print_config().printer;
    (printer.display_x, printer.display_y)
}

/// `ids?` (default every model): arrange them on the plate around the others.
pub(crate) fn arrange(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = match ids_param(p, "model")? {
        Some(ids) => ids,
        None => s.scene.models().iter().map(|m| m.id).collect(),
    };
    check_known(s, &ids)?;
    let beside = s.scene.arrange(&ids, plate(s), true)?;
    let mut answer = placed(s, &ids);
    answer["beside"] = json!(beside);
    Ok(answer)
}

fn id_param(p: &Value) -> Result<u64> {
    p.get("id").and_then(Value::as_u64).ok_or_else(|| EngineError::BadParams("`id` must be a model id".into()))
}

pub(crate) fn delete(s: &mut Session, p: &Value) -> Result<Value> {
    let id = id_param(p)?;
    let preset = *s.support_settings.current();
    if !s.scene.remove_models(|m| m == id, &preset) {
        return Err(EngineError::BadParams(format!("no model {id}")));
    }
    Ok(json!({ "id": id }))
}

pub(crate) fn list(s: &mut Session, _: &Value) -> Result<Value> {
    let models: Vec<Value> = s
        .scene
        .models()
        .iter()
        .map(|m| {
            json!({
                "id": m.id,
                "name": m.name,
                "triangles": m.mesh.triangle_count(),
                "bounds": { "min": m.bounds.min, "max": m.bounds.max },
                "placement": m.placement,
                "lifted": m.lifted,
                "slicing": m.slicing,
                "label": m.label,
                "selected": s.scene.selected_models.contains(&m.id),
            })
        })
        .collect();
    Ok(json!({ "models": models, "lift_height": s.scene.lift_height() }))
}

pub(crate) fn clear(s: &mut Session, _: &Value) -> Result<Value> {
    let removed = s.scene.models().len();
    let preset = *s.support_settings.current();
    s.scene.remove_models(|_| true, &preset);
    Ok(json!({ "removed": removed }))
}

pub(crate) fn select(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = ids_param(p, "model")?.ok_or_else(|| EngineError::BadParams("`ids` is required".into()))?;
    check_known(s, &ids)?;
    select_ids(&mut s.scene.selected_models, ids, p)?;
    Ok(json!({ "selected": s.scene.selected_models }))
}

/// `ids?` (default the selected models), `set?: { key: value }`, `clear?: [key] | "all"`: the
/// clearing first, then the setting, for each model; all or nothing.
pub(crate) fn slicing(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = targets(s, p)?;
    if let Value::Object(o) = p
        && let Some(k) = o.keys().find(|k| !["ids", "set", "clear"].contains(&k.as_str()))
    {
        return Err(EngineError::BadParams(format!("unknown parameter `{k}`")));
    }
    let clear: Vec<&str> = match p.get("clear") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::String(all)) if all == "all" => SLICING_KEYS.to_vec(),
        Some(Value::Array(keys)) => keys
            .iter()
            .map(|k| {
                k.as_str().filter(|k| SLICING_KEYS.contains(k)).ok_or_else(|| EngineError::BadParams(format!("`clear`: {k} is no slicing setting")))
            })
            .collect::<Result<_>>()?,
        Some(_) => return Err(EngineError::BadParams("`clear` must be an array of setting keys or \"all\"".into())),
    };
    let set = match p.get("set") {
        None | Some(Value::Null) => serde_json::Map::new(),
        Some(Value::Object(o)) => o.clone(),
        Some(_) => return Err(EngineError::BadParams("`set` must be an object of settings".into())),
    };
    let mut edits = Vec::with_capacity(ids.len());
    for id in ids {
        let Some(mut o) = s.scene.model(id).map(|m| m.slicing) else { continue };
        for k in &clear {
            o.clear(k);
        }
        for (k, v) in &set {
            let ok = match v {
                Value::Bool(on) => o.set_switch(k, *on),
                Value::Number(n) => n.as_f64().is_some_and(|x| o.set_number(k, x)),
                _ => false,
            };
            if !ok {
                let why = field("slicing", k).filter(|f| SLICING_KEYS.contains(&f.key)).map_or_else(
                    || {
                        if SLICING_KEYS.contains(&k.as_str()) {
                            format!("`slicing.{k}` must be true or false")
                        } else {
                            format!("unknown setting `{k}`")
                        }
                    },
                    |f| f.describe(),
                );
                return Err(EngineError::BadParams(why));
            }
        }
        edits.push((id, o));
    }
    s.scene.set_slicing(&edits)?;
    let models: Vec<Value> = edits.iter().map(|(id, o)| json!({ "id": id, "slicing": o })).collect();
    Ok(json!({ "models": models }))
}

/// The `id` parameter, or the one selected model.
pub(super) fn one_target(s: &Session, p: &Value) -> Result<u64> {
    let id = match p.get("id") {
        None | Some(Value::Null) => match s.scene.selected_models.iter().copied().collect::<Vec<_>>()[..] {
            [id] => id,
            _ => return Err(EngineError::BadParams("select one model, or give its `id`".into())),
        },
        Some(_) => id_param(p)?,
    };
    check_known(s, &[id])?;
    Ok(id)
}

pub(super) fn refuse_unknown(p: &Value, known: &[&str]) -> Result<()> {
    if let Value::Object(o) = p
        && let Some(k) = o.keys().find(|k| !known.contains(&k.as_str()))
    {
        return Err(EngineError::BadParams(format!("unknown parameter `{k}`")));
    }
    Ok(())
}

/// `id?` (default the one selected model), `preset?`, `text?`, `side?`, `size?`, `depth?`,
/// `margin?`: change what is given of the model's label, giving it one first if it has none (its
/// text the model's place in the model list, from 1). `preset` (a name, or null for none) comes
/// first; settings given for a label following a known preset change the preset too, and every
/// other model of the scene following it.
pub(crate) fn label(s: &mut Session, p: &Value) -> Result<Value> {
    refuse_unknown(p, &["id", "preset", "text", "side", "size", "depth", "margin"])?;
    let id = one_target(s, p)?;
    let index = s.scene.models().iter().position(|m| m.id == id).map_or(0, |i| i + 1);
    let mut label = s.scene.model(id).and_then(|m| m.label.clone()).unwrap_or_else(|| Label { text: index.to_string(), ..Label::default() });
    match p.get("preset") {
        None => {}
        Some(Value::Null) => label.preset = None,
        Some(Value::String(name)) => {
            let preset = s.presets.label.find(name).ok_or_else(|| EngineError::BadParams(format!("there is no label preset named {name}")))?;
            label = preset.applied(&label);
        }
        Some(_) => return Err(EngineError::BadParams("`preset` must be a string or null".into())),
    }
    match p.get("text") {
        None | Some(Value::Null) => {}
        Some(Value::String(t)) => label.text = t.clone(),
        Some(_) => return Err(EngineError::BadParams("`text` must be a string".into())),
    }
    match p.get("side") {
        None | Some(Value::Null) => {}
        Some(v) => {
            label.side =
                v.as_str().and_then(LabelSide::parse).ok_or_else(|| EngineError::BadParams("`side` must be front, back, left or right".into()))?
        }
    }
    if let Some(size) = num_param(p, "size")? {
        label.size = size;
    }
    if let Some(depth) = num_param(p, "depth")? {
        label.depth = depth;
    }
    if let Some(margin) = num_param(p, "margin")? {
        label.margin = margin;
    }
    s.scene.set_label(id, Some(label.clone()))?;
    let settings_given = ["side", "size", "depth", "margin"].iter().any(|k| p.get(*k).is_some_and(|v| !v.is_null()));
    let followers =
        if settings_given { crate::presets::edited::<LabelPresetKind>(s, label.preset.as_deref(), &LabelSettings::of(&label))? } else { Vec::new() };
    Ok(json!({ "id": id, "label": label, "followers": followers }))
}

/// `id?` (default the one selected model): take the model's label away.
pub(crate) fn delete_label(s: &mut Session, p: &Value) -> Result<Value> {
    refuse_unknown(p, &["id"])?;
    let id = one_target(s, p)?;
    s.scene.set_label(id, None)?;
    Ok(json!({ "id": id }))
}

fn check_known(s: &Session, ids: &[u64]) -> Result<()> {
    match ids.iter().find(|id| s.scene.model(**id).is_none()) {
        Some(id) => Err(EngineError::BadParams(format!("no model {id}"))),
        None => Ok(()),
    }
}

/// The `ids` parameter, or the selected models; never empty.
pub(super) fn targets(s: &Session, p: &Value) -> Result<Vec<u64>> {
    let ids = match ids_param(p, "model")? {
        Some(ids) => ids,
        None => s.scene.selected_models.iter().copied().collect(),
    };
    if ids.is_empty() {
        return Err(EngineError::BadParams("no model selected".into()));
    }
    check_known(s, &ids)?;
    Ok(ids)
}

/// Re-place the target models: `make` gets each one's placement and the centre of their joint
/// bounds. `lifted` is what becomes of lifted models (`None`: they stay so).
fn replace(s: &mut Session, p: &Value, lifted: Option<bool>, make: impl Fn(Placement, Vec3) -> Placement) -> Result<Value> {
    let ids = targets(s, p)?;
    let centre = s.scene.bounds_of(&ids).center();
    let placements: Vec<(u64, Placement, Option<bool>)> =
        s.scene.models().iter().filter(|m| ids.contains(&m.id)).map(|m| (m.id, make(m.placement, centre), lifted)).collect();
    s.scene.place(&placements)?;
    Ok(placed(s, &ids))
}

fn placed(s: &Session, ids: &[u64]) -> Value {
    let models: Vec<Value> = s
        .scene
        .models()
        .iter()
        .filter(|m| ids.contains(&m.id))
        .map(|m| json!({ "id": m.id, "placement": m.placement, "lifted": m.lifted, "bounds": { "min": m.bounds.min, "max": m.bounds.max } }))
        .collect();
    json!({ "models": models })
}

pub(crate) fn translate(s: &mut Session, p: &Value) -> Result<Value> {
    let by = vec3_param(p, "by")?.ok_or_else(|| EngineError::BadParams("`by` is required".into()))?;
    // Moving up or down is the user choosing the height: the models stop following the lift.
    let lifted = (by.z != 0.0).then_some(false);
    replace(s, p, lifted, |pl, _| Placement { position: pl.position + by, ..pl })
}

pub(crate) fn rotate(s: &mut Session, p: &Value) -> Result<Value> {
    let degrees = vec3_param(p, "degrees")?.ok_or_else(|| EngineError::BadParams("`degrees` is required".into()))?;
    let turn = Affine3::rotation_xyz(degrees);
    replace(s, p, None, |pl, centre| Placement {
        position: centre + turn.vector(pl.position - centre),
        rotation: Affine3::rotation_xyz(pl.rotation).then(&turn).euler_xyz_degrees(),
        ..pl
    })
}

pub(crate) fn scale(s: &mut Session, p: &Value) -> Result<Value> {
    let k = match p.get("factors") {
        Some(v) if v.is_number() => v.as_f64().map(|k| Vec3::new(k, k, k)),
        _ => vec3_param(p, "factors")?,
    }
    .ok_or_else(|| EngineError::BadParams("`factors` must be [x, y, z] or one number".into()))?;
    if ![k.x, k.y, k.z].iter().all(|k| SCALE_RANGE.contains(k)) {
        return Err(EngineError::BadParams(format!("scale factors must be from {} to {}", SCALE_RANGE.start(), SCALE_RANGE.end())));
    }
    // Along each model's own axes, about its own pivot: a turned model keeps its shape.
    replace(s, p, None, |pl, _| Placement { scale: Vec3::new(pl.scale.x * k.x, pl.scale.y * k.y, pl.scale.z * k.z), ..pl })
}

/// `models: [{ id, position?, rotation?, scale?, lifted? }]`: set each listed model's placement,
/// the fields left out kept. Without `lifted`, an entry that changes only Z (rotation and scale
/// as they are) is a move up or down, and the model stops following the lift.
pub(crate) fn place(s: &mut Session, p: &Value) -> Result<Value> {
    let bad = || EngineError::BadParams("`models` must be an array of { id, position?, rotation?, scale?, lifted? }".into());
    let list = p.get("models").and_then(Value::as_array).ok_or_else(bad)?;
    let mut placements = Vec::with_capacity(list.len());
    for entry in list {
        let id = entry.get("id").and_then(Value::as_u64).ok_or_else(bad)?;
        let m = s.scene.model(id).ok_or_else(|| EngineError::BadParams(format!("no model {id}")))?;
        let mut pl = m.placement;
        if let Some(v) = vec3_param(entry, "position")? {
            pl.position = v;
        }
        if let Some(v) = vec3_param(entry, "rotation")? {
            pl.rotation = v;
        }
        if let Some(v) = vec3_param(entry, "scale")? {
            pl.scale = v;
        }
        let was = m.placement;
        // Exact comparisons: the gizmo and the panel copy an unchanged Z as it is.
        let z_moved = pl.position.z != was.position.z && pl.rotation == was.rotation && pl.scale == was.scale;
        let lifted = bool_param(entry, "lifted")?.or(z_moved.then_some(false));
        placements.push((id, pl, lifted));
    }
    s.scene.place(&placements)?;
    let ids: Vec<u64> = placements.iter().map(|(id, ..)| *id).collect();
    Ok(placed(s, &ids))
}

/// `lifted: bool`, `ids?`: keep the models' lowest points at the lift height, or (false) leave
/// them where they are from now on.
pub(crate) fn lift(s: &mut Session, p: &Value) -> Result<Value> {
    let lifted = bool_param(p, "lifted")?.ok_or_else(|| EngineError::BadParams("`lifted` must be a boolean".into()))?;
    replace(s, p, Some(lifted), |pl, _| pl)
}

/// `height` mm: where every lifted model's lowest point sits.
pub(crate) fn lift_height(s: &mut Session, p: &Value) -> Result<Value> {
    let h = num_param(p, "height")?.ok_or_else(|| EngineError::BadParams("`height` must be a finite number".into()))?;
    s.scene.set_lift_height(h)?;
    let lifted: Vec<u64> = s.scene.models().iter().filter(|m| m.lifted).map(|m| m.id).collect();
    Ok(json!({ "lift_height": s.scene.lift_height(), "lifted": lifted }))
}
