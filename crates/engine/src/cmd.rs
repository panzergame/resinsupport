//! The command registry. A command id is dotted and lower-case: `<area>.<thing>[.<variant>]`.

mod edit;
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
    /// A run that changes the scene is an undo step. True unless `noundo`: only undo and redo
    /// opt out; a command that leaves the scene alone makes no step anyway.
    pub undoable: bool,
}

impl CommandSpec {
    pub const fn new(id: &'static str, label: &'static str, run: fn(&mut Session, &Value) -> Result<Value>) -> Self {
        CommandSpec { id, label, params: "", run, undoable: true }
    }
    pub const fn params(mut self, p: &'static str) -> Self {
        self.params = p;
        self
    }
    pub const fn noundo(mut self) -> Self {
        self.undoable = false;
        self
    }
}

/// Every command, sorted by id.
pub fn commands() -> &'static [CommandSpec] {
    static ALL: &[CommandSpec] = &[
        CommandSpec::new("edit.redo", "Redo", edit::redo).noundo(),
        CommandSpec::new("edit.undo", "Undo", edit::undo).noundo(),
        CommandSpec::new("file.import_stl", "Import STL…", model::import_stl).params("path: string, fit?: bool (default true)"),
        CommandSpec::new("model.delete", "Delete Model", model::delete).params("id: integer"),
        CommandSpec::new("model.list", "List Models", model::list),
        CommandSpec::new("model.move", "Move", model::translate).params("by: [x, y, z] mm, ids?: [integer] (default the selected models)"),
        CommandSpec::new("model.place", "Place Models", model::place).params(
            "models: [{ id, position?: [x, y, z] mm, rotation?: [x, y, z] degrees, scale?: [x, y, z] }], each measured from the model as imported",
        ),
        CommandSpec::new("model.rotate", "Rotate", model::rotate)
            .params("degrees: [x, y, z] (turned about X, then Y, then Z, around the models' centre), ids?: [integer] (default the selected models)"),
        CommandSpec::new("model.scale", "Scale", model::scale).params(
            "factors: [x, y, z] or one number, each 0.001 to 1000, along each model's own axes; ids?: [integer] (default the selected models)",
        ),
        CommandSpec::new("model.select", "Select Models", model::select)
            .params("ids: [integer] (empty clears), mode?: \"replace\" | \"add\" | \"toggle\" (default replace)"),
        CommandSpec::new("scene.clear", "Clear Scene", model::clear),
        CommandSpec::new("support.add", "Add Support", support::add)
            .params("model: integer, point: [x, y, z] on its surface, normal?: [x, y, z] outward (default down)"),
        CommandSpec::new("support.delete", "Delete Supports", support::delete).params("ids?: [integer] (default the selection)"),
        CommandSpec::new("support.list", "List Supports", support::list),
        CommandSpec::new("support.select", "Select Supports", support::select)
            .params("ids: [integer] (empty clears), mode?: \"replace\" | \"add\" | \"toggle\" (default replace)"),
        CommandSpec::new("view.fit", "Fit All", view::fit).params("aspect?: number (width / height)"),
        CommandSpec::new("view.projection", "Projection", view::projection).params("mode: \"perspective\" | \"orthographic\" | \"toggle\""),
        CommandSpec::new("view.standard", "Standard View", view::standard).params("view: front | back | left | right | top | bottom | iso"),
        CommandSpec::new("view.turn", "Turn View", view::turn).params("direction: left | right | up | down, degrees?: number (default 15)"),
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
