//! The command registry. A command id is dotted and lower-case: `<area>.<thing>[.<variant>]`.

mod model;
mod support;
mod view;

use std::collections::BTreeSet;

use resin_geom::Vec3;
use serde_json::Value;

use crate::{EngineError, Result, Session};

/// One command: what it is called, what it takes, and what it does.
pub struct CommandSpec {
    pub id: &'static str,
    pub label: &'static str,
    /// The parameters, for documentation and error messages.
    pub params: &'static str,
    pub run: fn(&mut Session, &Value) -> Result<Value>,
}

/// Every command, sorted by id.
pub fn commands() -> &'static [CommandSpec] {
    static ALL: &[CommandSpec] = &[
        CommandSpec { id: "file.import_stl", label: "Import STL…", params: "path: string, fit?: bool (default true)", run: model::import_stl },
        CommandSpec { id: "model.delete", label: "Delete Model", params: "id: integer", run: model::delete },
        CommandSpec { id: "model.list", label: "List Models", params: "", run: model::list },
        CommandSpec {
            id: "model.move",
            label: "Move",
            params: "by: [x, y, z] mm, ids?: [integer] (default the selected models)",
            run: model::translate,
        },
        CommandSpec {
            id: "model.place",
            label: "Place Models",
            params: "models: [{ id, position?: [x, y, z] mm, rotation?: [x, y, z] degrees, scale?: [x, y, z] }], each measured from the model as imported",
            run: model::place,
        },
        CommandSpec {
            id: "model.rotate",
            label: "Rotate",
            params: "degrees: [x, y, z] (turned about X, then Y, then Z, around the models' centre), ids?: [integer] (default the selected models)",
            run: model::rotate,
        },
        CommandSpec {
            id: "model.scale",
            label: "Scale",
            params: "factors: [x, y, z] or one number, each 0.001 to 1000, along each model's own axes; ids?: [integer] (default the selected models)",
            run: model::scale,
        },
        CommandSpec {
            id: "model.select",
            label: "Select Models",
            params: "ids: [integer] (empty clears), mode?: \"replace\" | \"add\" | \"toggle\" (default replace)",
            run: model::select,
        },
        CommandSpec { id: "scene.clear", label: "Clear Scene", params: "", run: model::clear },
        CommandSpec {
            id: "support.add",
            label: "Add Support",
            params: "model: integer, point: [x, y, z] on its surface, normal?: [x, y, z] outward (default down)",
            run: support::add,
        },
        CommandSpec { id: "support.delete", label: "Delete Supports", params: "ids?: [integer] (default the selection)", run: support::delete },
        CommandSpec { id: "support.list", label: "List Supports", params: "", run: support::list },
        CommandSpec {
            id: "support.select",
            label: "Select Supports",
            params: "ids: [integer] (empty clears), mode?: \"replace\" | \"add\" | \"toggle\" (default replace)",
            run: support::select,
        },
        CommandSpec { id: "view.fit", label: "Fit All", params: "aspect?: number (width / height)", run: view::fit },
        CommandSpec {
            id: "view.projection",
            label: "Projection",
            params: "mode: \"perspective\" | \"orthographic\" | \"toggle\"",
            run: view::projection,
        },
        CommandSpec {
            id: "view.standard",
            label: "Standard View",
            params: "view: front | back | left | right | top | bottom | iso",
            run: view::standard,
        },
        CommandSpec {
            id: "view.turn",
            label: "Turn View",
            params: "direction: left | right | up | down, degrees?: number (default 15)",
            run: view::turn,
        },
    ];
    ALL
}

pub(crate) fn find(id: &str) -> Option<&'static CommandSpec> {
    commands().iter().find(|c| c.id == id)
}

pub(crate) fn str_param<'a>(params: &'a Value, key: &str) -> Result<&'a str> {
    params.get(key).and_then(Value::as_str).ok_or_else(|| EngineError::BadParams(format!("`{key}` must be a string")))
}

/// An optional finite number.
pub(crate) fn num_param(params: &Value, key: &str) -> Result<Option<f64>> {
    match params.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v.as_f64().filter(|x| x.is_finite()).map(Some).ok_or_else(|| EngineError::BadParams(format!("`{key}` must be a finite number"))),
    }
}

pub(crate) fn vec3_param(p: &Value, key: &str) -> Result<Option<Vec3>> {
    let bad = || EngineError::BadParams(format!("`{key}` must be [x, y, z] finite numbers"));
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Array(a)) if a.len() == 3 => {
            let c: Vec<f64> = a.iter().map(|v| v.as_f64().filter(|x| x.is_finite()).ok_or_else(bad)).collect::<Result<_>>()?;
            match c.as_slice() {
                &[x, y, z] => Ok(Some(Vec3::new(x, y, z))),
                _ => Err(bad()),
            }
        }
        Some(_) => Err(bad()),
    }
}

/// An optional array of ids of `what` ("model", "support").
pub(crate) fn ids_param(p: &Value, what: &str) -> Result<Option<Vec<u64>>> {
    let bad = || EngineError::BadParams(format!("`ids` must be an array of {what} ids"));
    match p.get("ids") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Array(a)) => a.iter().map(|v| v.as_u64().ok_or_else(bad)).collect::<Result<_>>().map(Some),
        Some(_) => Err(bad()),
    }
}

/// Change `selected` by `ids` as the optional `mode` parameter says.
pub(crate) fn select_ids(selected: &mut BTreeSet<u64>, ids: Vec<u64>, p: &Value) -> Result<()> {
    let mode = match p.get("mode") {
        None | Some(Value::Null) => "replace",
        Some(v) => v.as_str().ok_or_else(|| EngineError::BadParams("`mode` must be a string".into()))?,
    };
    match mode {
        "replace" => {
            selected.clear();
            selected.extend(ids);
        }
        "add" => selected.extend(ids),
        "toggle" => {
            for id in ids {
                if !selected.remove(&id) {
                    selected.insert(id);
                }
            }
        }
        other => return Err(EngineError::BadParams(format!("unknown mode `{other}`"))),
    }
    Ok(())
}
