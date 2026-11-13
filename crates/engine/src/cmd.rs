//! The command registry. A command id is dotted and lower-case: `<area>.<thing>[.<variant>]`.

mod edit;
mod file;
mod model;
mod print_config;
mod support;
mod support_settings;
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
    /// A run that changes the scene is an undo step. True unless `noundo`: only undo, redo and
    /// opening a project (which starts a fresh history) opt out; a command that leaves the scene alone (the view, the support or print settings) makes no
    /// step anyway.
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
        CommandSpec::new("edit.copy", "Copy", edit::copy)
            .params("ids?: [integer] (default the selected models); they and their supports replace what was copied before"),
        CommandSpec::new("edit.paste", "Paste", edit::paste).params(
            "none; adds copies of what was copied, with their supports, arranged on the plate around the other models as `model.arrange` does, and selects them; returns their `ids` and those that went `beside` the plate",
        ),
        CommandSpec::new("edit.redo", "Redo", edit::redo).noundo(),
        CommandSpec::new("edit.undo", "Undo", edit::undo).noundo(),
        CommandSpec::new("file.export_ctb", "Export CTB…", file::export_ctb).params(
            "path: a .ctb file, encryption_key?: integer 0 to 4294967295 (default 0x67; 0 writes the layers unciphered); slices first, as `print.slice` does, when the kept job no longer matches the scene and settings, then writes the job as a Chitubox CTB v2 file with plain previews (the job seen from above). Returns path, bytes, layers",
        ),
        CommandSpec::new("file.import_stl", "Import STL…", model::import_stl)
            .params("path: string, fit?: bool (default true), lifted?: bool (default false): its lowest point at the lift height, arrange?: bool (default false): arranged on the plate around the other models as `model.arrange` does, and the answer says `on_plate`"),
        CommandSpec::new("file.open", "Open Project", file::open).noundo().params(
            "path: a .rsup project; its models and supports replace the scene's, unlifted, where they were saved, the view is fitted, and the undo history starts afresh",
        ),
        CommandSpec::new("file.save", "Save Project", file::save).params(
            "path?: string ending in .rsup (default the scene's file); the models (mesh, name, placement) and supports (tip, normal, placed corners), not the lift or any setting; the file it replaces is kept as <path>.bak",
        ),
        CommandSpec::new("model.arrange", "Arrange Models", model::arrange).params(
            "ids?: [integer] (default every model); arranges them, the largest first, each at the free spot nearest the plate's centre, around the other models: moved across, turned a quarter about Z to lay their long side along the plate's or when only that fits (not a model with supports), put down on the plate or at the lift height; those with no room go in a column right of the plate, listed in `beside`",
        ),
        CommandSpec::new("model.delete", "Delete Model", model::delete).params("id: integer"),
        CommandSpec::new("model.lift", "Lift Models", model::lift).params(
            "lifted: bool (true keeps their lowest points at the lift height, false leaves them where they are), ids?: [integer] (default the selected models)",
        ),
        CommandSpec::new("model.list", "List Models", model::list),
        CommandSpec::new("model.move", "Move", model::translate).params("by: [x, y, z] mm, ids?: [integer] (default the selected models)"),
        CommandSpec::new("model.place", "Place Models", model::place).params(
            "models: [{ id, position?: [x, y, z] mm, rotation?: [x, y, z] degrees, scale?: [x, y, z], lifted?: bool }], each measured from the model as imported; without `lifted`, a change of Z alone stops the model following the lift",
        ),
        CommandSpec::new("model.rotate", "Rotate", model::rotate)
            .params("degrees: [x, y, z] (turned about X, then Y, then Z, around the models' centre), ids?: [integer] (default the selected models)"),
        CommandSpec::new("model.scale", "Scale", model::scale).params(
            "factors: [x, y, z] or one number, each 0.001 to 1000, along each model's own axes; ids?: [integer] (default the selected models)",
        ),
        CommandSpec::new("model.select", "Select Models", model::select)
            .params("ids: [integer] (empty clears), mode?: \"replace\" | \"add\" | \"toggle\" (default replace)"),
        CommandSpec::new("print.configure", "Print Settings", print_config::configure).params(
            "printer?: { name: string (at most 64 characters), resolution_x, resolution_y px, display_x, display_y mm, build_height mm, mirror_x, mirror_y }, layers?: { height mm, bottom_count, transition_count }, exposure?: { normal, bottom, light_off, bottom_light_off s, power, bottom_power 1-255 }, motion?: { lift_height, bottom_lift_height mm, lift_speed, bottom_lift_speed, retract_speed mm/min }, slicing?: { anti_aliasing: 1 | 2 | 4 | 8 }, estimates?: { resin_density g/ml, bottle_volume ml, bottle_cost }; each partial, the result validated as a whole",
        ),
        CommandSpec::new("print.reset", "Restore Print Settings", print_config::reset),
        CommandSpec::new("print.settings", "List Print Settings", print_config::settings),
        CommandSpec::new("print.slice", "Slice", print_config::slice).params(
            "none; slices the scene with its reachable supports and the print settings, on the calling thread, and keeps the job. Returns layers, height mm, volume_ml, print_time_s, clipped (solids crossing the plate's edge or build height), unreachable_supports (left out), open_rows (rows of open or inside-out meshes)",
        ),
        CommandSpec::new("scene.clear", "Clear Scene", model::clear),
        CommandSpec::new("scene.lift_height", "Lift Height", model::lift_height)
            .params("height: mm, 0 to 500: where every lifted model's lowest point sits"),
        CommandSpec::new("support.add", "Add Support", support::add)
            .params("model: integer, point: [x, y, z] on its surface, normal?: [x, y, z] outward (default down), parent?: integer (a branch: a straight tip to that support's head, sharing its body)"),
        CommandSpec::new("support.add_braces", "Add Braces", support::add_braces).params(
            "ids?: [integer] (default the selected supports and those with selected parts; a branch stands for the support whose body it shares), pattern?: zigzag | cross (default zigzag), pairing?: chain | tree | all (default tree), max_distance?: mm, 1 to 100 (default 10; farther pairs are not braced), angle?: degrees from horizontal, 15 to 80 (default 45; measured in the plane of the two bodies, section by section between their knees, and made only steeper so whole bays fill each section), diameter_ratio?: 0.1 to 1 of the body's diameter (default 0.5), head_gap?: mm below the lower head, foot_gap?: mm above the base cones, each 0 to 100 (default 2 and 1); struts that would come within the body clearance of a model are left out and a pair with none left is skipped; every brace already joining two of the supports is replaced, so running it again with another pairing or setting rebuilds them, and braces to other supports stay. Returns the new braces' `ids`, how many pairs were `skipped` and how many braces were `replaced`",
        ),
        CommandSpec::new("support.add_line", "Add Support Line", support::add_line).params(
            "from: integer, to: integer (another support) | { model: integer, point: [x, y, z] on its surface, normal?: [x, y, z] outward (default down) } (a new support there, made last), spacing: mm (the gaps are equal and at most this, at most 1000 tips), per_body?: integer (default 3); the points between the two tips are cast onto the surface along the normal blended from theirs, those landing nowhere skipped (and ending a run); ends sharing a body get every new tip as a branch of it, otherwise each run of `per_body` tips is a new support (its middle tip) and its branches. Returns the new `ids` and how many points were `skipped`",
        ),
        CommandSpec::new("support.configure", "Support Settings", support_settings::configure).params(
            "preset?: light | medium | heavy (default the active one), active?: light | medium | heavy, tip?: { contact_shape: cone | sphere, contact_diameter, penetration, length, upper_diameter mm, max_angle degrees }, body?: { shape: cylinder | prism, diameter mm, max_angle degrees, segments }, base?: { diameter, height } mm, raft?: { enabled, thickness mm, margin mm, chamfer_angle degrees }; each partial, the result validated as a whole",
        ),
        CommandSpec::new("support.delete", "Delete Supports", support::delete).params("ids?: [integer], braces?: [integer] (with neither, the selected supports and braces); a body goes with its last tip, a deleted support's first remaining branch takes its body, and a brace goes with either of its supports"),
        CommandSpec::new("support.list", "List Supports", support::list),
        CommandSpec::new("support.move_tip", "Move Support Tip", support::move_tip).params(
            "support: integer, model: integer, point: [x, y, z] on its surface, normal?: [x, y, z] outward (default down); placed corners and branches stay, the tip stretches to its head; the support's braces are removed (not a branch's: its body stays)",
        ),
        CommandSpec::new("support.place_joints", "Move Support Joints", support::place_joints).params(
            "joints: [{ support: integer, joint: integer, position: [x, y, z] mm, Z from 0 }] in the scene; a support whose route was worked out keeps that route as placed corners first; a moved support's braces are removed",
        ),
        CommandSpec::new("support.reset_preset", "Restore Support Preset", support_settings::reset_preset)
            .params("preset?: light | medium | heavy (default the active one)"),
        CommandSpec::new("support.reset_route", "Reset Support Path", support::reset_route)
            .params("ids?: [integer] (default the selected supports and those with selected parts); a reset support's braces are removed"),
        CommandSpec::new("support.select", "Select Supports", support::select).params(
            "ids?: [integer], parts?: [{ support: integer, joint: integer } | { support: integer, segment: integer }], braces?: [integer] (at least one; empty clears), mode?: \"replace\" | \"add\" | \"toggle\" (default replace), over all three",
        ),
        CommandSpec::new("support.settings", "List Support Settings", support_settings::settings),
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
pub(crate) fn select_ids<T: Ord>(selected: &mut BTreeSet<T>, ids: Vec<T>, p: &Value) -> Result<()> {
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
