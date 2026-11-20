//! The command registry. A command id is dotted and lower-case: `<area>.<thing>[.<variant>]`.

mod edit;
mod file;
mod label_preset;
mod model;
mod preset;
mod print_config;
mod support;
mod support_settings;
mod view;

use std::collections::BTreeSet;

use resin_geom::Vec3;
use serde_json::Value;

use crate::{EngineError, LabelPresetKind, Result, Session};

/// One command: what it is called, what it takes, and what it does.
pub struct CommandSpec {
    pub id: &'static str,
    pub label: &'static str,
    /// The parameters, for documentation and error messages.
    pub params: &'static str,
    pub run: fn(&mut Session, &Value) -> Result<Value>,
    /// A run that changes the scene is an undo step. True unless `noundo`: only undo, redo,
    /// opening a project (which starts a fresh history) and the [`CommandSpec::across_history`]
    /// ones opt out; a command that leaves the scene alone (the view, the support or print
    /// settings) makes no step anyway.
    pub undoable: bool,
    /// What it changes in the scene, it changes in every scene of the undo history too
    /// ([`Session::every_scene`]), so no undo or redo takes it back: the preset commands, the
    /// user's library never being an undo step, nor what follows from it. No step.
    pub across_history: bool,
}

impl CommandSpec {
    pub const fn new(id: &'static str, label: &'static str, run: fn(&mut Session, &Value) -> Result<Value>) -> Self {
        CommandSpec { id, label, params: "", run, undoable: true, across_history: false }
    }
    pub const fn params(mut self, p: &'static str) -> Self {
        self.params = p;
        self
    }
    pub const fn noundo(mut self) -> Self {
        self.undoable = false;
        self
    }
    pub const fn across_history(mut self) -> Self {
        self.undoable = false;
        self.across_history = true;
        self
    }
}

/// Every command, sorted by id.
pub fn commands() -> &'static [CommandSpec] {
    static ALL: &[CommandSpec] = &[
        CommandSpec::new("edit.copy", "Copy", edit::copy)
            .params("ids?: [integer] (default the selected models); they and their supports replace what was copied before"),
        CommandSpec::new("edit.paste", "Paste", edit::paste).params(
            "none; adds copies of what was copied, with their supports, arranged on the plate around the other models as `model.arrange` does but at the height they were copied at, and selects them; returns their `ids` and those that went `beside` the plate",
        ),
        CommandSpec::new("edit.redo", "Redo", edit::redo).noundo(),
        CommandSpec::new("edit.undo", "Undo", edit::undo).noundo(),
        CommandSpec::new("file.export_ctb", "Export CTB…", file::export_ctb).params(
            "path: a .ctb file, format?: \"encrypted\" | \"v2\" (default the printer's `ctb_format`), encryption_key?: integer 0 to 4294967295 (default 0x67; 0 writes the layers unciphered); slices first, as `print.slice` does, when the kept job no longer matches the scene and settings, then writes the job as a Chitubox CTB file (encrypted v5 or plain v2) with plain previews (the job seen from above). Returns path, bytes, layers, format",
        ),
        CommandSpec::new("file.import_stl", "Import STL…", model::import_stl)
            .params("path: string, fit?: bool (default true), lifted?: bool (default false): its lowest point at the lift height, arrange?: bool (default false): arranged on the plate around the other models as `model.arrange` does, and the answer says `on_plate`"),
        CommandSpec::new("file.open", "Open Project", file::open).noundo().params(
            "path: a .rsup project; its models and supports replace the scene's, unlifted, where they were saved, the view is fitted, and the undo history starts afresh",
        ),
        CommandSpec::new("file.save", "Save Project", file::save).params(
            "path?: string ending in .rsup (default the scene's file); the models (mesh, name, placement) and supports (tip, normal, placed corners), not the lift or any setting; the file it replaces is kept as <path>.bak",
        ),
        CommandSpec::new("label.preset.delete", "Delete Label Preset", preset::delete::<LabelPresetKind>).params(
            "name: string (ignoring case); the preset goes, and labels following it keep their settings and its name, so saving a preset of that name again takes them back. Returns the `preset` deleted",
        ),
        CommandSpec::new("label.preset.list", "List Label Presets", preset::list::<LabelPresetKind>).params("none; returns `presets`, each `name`, `side`, `size`, `depth`, `margin`, sorted by name"),
        CommandSpec::new("label.preset.rename", "Rename Label Preset", preset::rename::<LabelPresetKind>).across_history().params(
            "name: string (ignoring case), to: string (as for `label.preset.save`; another preset's name refused, a change of case allowed); the preset takes the new name, and every label following it follows it under that name, in the scene and through the undo history (no step: undo never brings the old name back; labels in other projects still name the old one). Returns `preset` and the ids `relinked`",
        ),
        CommandSpec::new("label.preset.save", "Save Label Preset", preset::save::<LabelPresetKind>).across_history().params(
            "name: string (1-64 characters, no spaces round it, unique ignoring case; at most 64 presets), id?: integer (default the one selected model, which must have a label); a new preset of the label's side, size, depth and margin (not its text), the label then following it, as it does in every scene of the undo history where it has those settings. Presets are the user's own, kept with the preferences across projects, and never an undo step, nor is the link. Returns `target` (the model's id) and `preset`",
        ),
        CommandSpec::new("model.arrange", "Arrange Models", model::arrange).params(
            "ids?: [integer] (default every model); arranges them, the largest first, each at the free spot nearest the plate's centre, around the other models: moved across, turned a quarter about Z to lay their long side along the plate's or when only that fits (not a model with supports), put down on the plate or at the lift height; those with no room go in a column right of the plate, listed in `beside`",
        ),
        CommandSpec::new("model.delete", "Delete Model", model::delete).params("id: integer"),
        CommandSpec::new("model.label", "Label Model", model::label).params(
            "id?: integer (default the one selected model), preset?: string (a label preset's name: the label takes its settings and follows it) | null (it follows none), text?: string (lines break at \\n; at most 200 characters, 8 lines), side?: \"front\" (-Y) | \"back\" (+Y) | \"left\" (-X) | \"right\" (+X), size?: 1-50 mm (the capital letters' height), depth?: 0.1-10 mm (how far the letters stand out), margin?: 0-20 mm (from the support's base to the letters; the raft's edge and rim keep their own room round the other sides); what is left out is kept, or for a model without a label its number in the model list, front, 2 mm, 0.5 mm and 1 mm. The label is printed past the base of the model's support farthest on that side, inset into the raft, the raft widened out round it, the letters standing on it, read from that side. Side, size, depth or margin given for a label following a known preset change the preset to the label's settings, and every other model following it (one step). Returns `id`, `label` and the `followers` that changed with it",
        ),
        CommandSpec::new("model.label.delete", "Delete Model Label", model::delete_label).params("id?: integer (default the one selected model)"),
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
        CommandSpec::new("model.slicing", "Model Slicing Settings", model::slicing).params(
            "ids?: [integer] (default the selected models), clear?: [key] | \"all\" (back to the plate's), set?: { key: value } with keys and ranges as print.configure's slicing group (anti_aliasing, exact, anti_aliasing_z, blur_radius, blur_passes, dither_z, dither_xy, dither_floor, dither_max_angle, gamma); the clearing first, then the setting. The models and their supports (braces between two of one model's) slice with these in place of the plate's; each set of settings is sliced apart and the brightest grey of any kept. Returns each model's `slicing`",
        ),
        CommandSpec::new("print.configure", "Print Settings", print_config::configure).params(
            "printer?: { name: string (at most 64 characters), resolution_x, resolution_y px, display_x, display_y mm, build_height mm, mirror_x, mirror_y, ctb_format: \"encrypted\" (CTB v5, Saturn 2 and later) | \"v2\" (plain CTB v2) }, layers?: { height mm, bottom_count, transition_count }, exposure?: { normal, bottom, light_off, bottom_light_off s, power, bottom_power 1-255 }, motion?: { lift_distance_fast, lift_distance_slow, retract_distance_slow mm, lift_speed_fast, lift_speed_slow, retract_speed_fast, retract_speed_slow mm/min, and each again with a bottom_ prefix for the bottom layers; the lift goes slow then fast, the retract comes back the whole lift, fast then slow over retract_distance_slow (at most the lift, shortened to fit a shorter lift unless given) }, slicing?: { anti_aliasing: 1 | 2 | 4 | 8 sub-samples per pixel side, exact: true to grey each pixel by the share of it covered instead, anti_aliasing_z: 1 | 2 | 4 | 8 planes cut through each layer and averaged, blur_radius: 1-16 px of box blur after the anti-aliasing, blur_passes: 0-4 (0 off), dither_z: true to dither the greys printing under dither_floor across layers (each lit at the floor on its share of them), dither_xy: true to start each pixel's dither from its own point so neighbours switch on apart, dither_floor: 1-100 % the lowest printed grey the resin cures (default 50; 100: lit or dark only), dither_max_angle: 0-90° from vertical, the flattest surface dithered (default 20; 90: everywhere; needs anti_aliasing_z of 2 or more), gamma 0-1 (greys become 255 · (grey / 255)^gamma; 0 lights the pixels at least half covered) }, estimates?: { resin_density g/ml, bottle_volume ml, bottle_cost }; each partial, the result validated as a whole",
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
            "preset?: light | medium | heavy (default the active one), active?: light | medium | heavy, tip?: { contact_shape: cone | sphere, contact_diameter, penetration, length, upper_diameter mm, max_angle degrees }, body?: { shape: cylinder | prism, diameter mm, max_angle degrees, segments }, base?: { diameter, height } mm, raft?: { enabled, thickness, margin, merge_distance mm, edge: straight | undercut | chamfer, edge_angle degrees, rim_height, rim_width mm }; each partial, the result validated as a whole",
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
