//! The Move, Rotate and Scale tools (T, R, S in Object edition): the gizmo on the selection and
//! a small panel of its placement, both live. Values are measured from the model as imported,
//! and every change is a `model.place` command built from the placements a drag began with, so
//! nothing piles up while dragging. A gesture (a drag, a field typed in) is one undo step, and
//! Cancel leaves the history as it was.

use egui::{DragValue, vec2};
use resin_engine::MAX_EXTENT;
use resin_engine::geom::{Affine3, EPS, Placement, Vec3};
use serde_json::{Value, json};

use crate::gizmo::{Change, Drag, Triad};
use crate::theme::Tokens;
use crate::theme::metrics::{CONTROL_HEIGHT, FORM_CARD_WIDTH, SPACE_12};
use crate::ui_kit::{self, Action, Buttons, Card};
use crate::{EditMode, ResinApp, i18n};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    Move,
    Rotate,
    Scale,
}

impl Op {
    /// Catalog id of its name.
    pub fn label(self) -> &'static str {
        match self {
            Op::Move => "ribbon.move",
            Op::Rotate => "ribbon.rotate",
            Op::Scale => "ribbon.scale",
        }
    }
    /// Its icon in [`crate::icons`].
    pub fn icon(self) -> &'static str {
        match self {
            Op::Move => "move",
            Op::Rotate => "revolve",
            Op::Scale => "scale",
        }
    }
    pub fn shortcut(self) -> &'static str {
        match self {
            Op::Move => "T",
            Op::Rotate => "R",
            Op::Scale => "S",
        }
    }
}

const AXES: [&str; 3] = ["X", "Y", "Z"];
const DIRS: [Vec3; 3] = [Vec3::X, Vec3::Y, Vec3::Z];

pub struct Tool {
    pub op: Op,
    uniform: bool,
    /// While a gizmo drag runs: the centre it turns about and the placements it began with.
    drag: Option<(Vec3, Vec<(u64, Placement)>)>,
    /// The selection's placements when the tool opened: what Cancel puts back.
    before: Vec<(u64, Placement)>,
    /// The history then: Cancel forgets the steps made since.
    mark: u64,
}

fn placements(app: &ResinApp) -> Vec<(u64, Placement)> {
    let scene = &app.session.scene;
    scene.models().iter().filter(|m| scene.selected_models.contains(&m.id)).map(|m| (m.id, m.placement)).collect()
}

/// Choose the tool for `op` on the selected models.
pub fn open(app: &mut ResinApp, op: Op) {
    if app.mode != EditMode::Object {
        app.status = Some((i18n::format("tool.needs_object_edition", &[("tool", &i18n::tr(op.label()))]), true));
        return;
    }
    if app.session.scene.selected_models.is_empty() {
        app.status = Some((i18n::tr("reason.select_model").into(), true));
        return;
    }
    let uniform = app.tool.as_ref().is_none_or(|t| t.uniform);
    app.tool = Some(Tool { op, uniform, drag: None, before: placements(app), mark: app.session.history_mark() });
}

/// Whether the tool can act now: Object edition with something selected.
fn active(app: &ResinApp) -> Option<Op> {
    let op = app.tool.as_ref()?.op;
    (app.mode == EditMode::Object && !app.session.scene.selected_models.is_empty()).then_some(op)
}

fn selection(app: &ResinApp) -> Vec<u64> {
    app.session.scene.selected_models.iter().copied().collect()
}

/// The gizmo for the current tool: at a lone model's pivot, or at the centre of several. Scale's
/// handles follow a lone model's own axes.
pub fn triad(app: &ResinApp) -> Option<Triad> {
    let op = active(app)?;
    let ids = selection(app);
    let scene = &app.session.scene;
    let lone = match ids.as_slice() {
        [id] => scene.model(*id),
        _ => None,
    };
    let center = match app.tool.as_ref().and_then(|t| t.drag.as_ref()) {
        // Rotating about where the drag began; moving and scaling follow the model.
        Some((c, _)) if op == Op::Rotate => *c,
        _ => lone.map_or_else(|| scene.bounds_of(&ids).center(), |m| m.placement.position),
    };
    let axes = lone.map_or(DIRS, |m| {
        let r = Affine3::rotation_xyz(m.placement.rotation);
        DIRS.map(|d| r.vector(d))
    });
    Some(Triad { center, translate: op == Op::Move, rotate: op == Op::Rotate, scale: (op == Op::Scale).then_some(axes) })
}

/// Apply a gizmo drag: from the placements it began with, every frame.
pub fn drag(app: &mut ResinApp, g: Drag) {
    let Some(tool) = app.tool.as_mut() else { return };
    if g.started || tool.drag.is_none() {
        let scene = &app.session.scene;
        let ids: Vec<u64> = scene.selected_models.iter().copied().collect();
        let centre = match ids.as_slice() {
            [id] => scene.model(*id).map_or(Vec3::ZERO, |m| m.placement.position),
            _ => scene.bounds_of(&ids).center(),
        };
        let start = scene.models().iter().filter(|m| ids.contains(&m.id)).map(|m| (m.id, m.placement)).collect();
        tool.drag = Some((centre, start));
    }
    let uniform = tool.uniform;
    let Some((centre, start)) = tool.drag.clone() else { return };
    if g.done {
        tool.drag = None;
    }
    let models: Vec<Value> = start
        .iter()
        .map(|(id, p0)| {
            let p = match g.change {
                Change::Translate(v) => Placement { position: p0.position + v, ..*p0 },
                Change::Rotate { axis, angle } => {
                    let turn = Affine3::rotation_about(DIRS[axis.min(2)], angle);
                    Placement {
                        position: centre + turn.vector(p0.position - centre),
                        rotation: Affine3::rotation_xyz(p0.rotation).then(&turn).euler_xyz_degrees(),
                        ..*p0
                    }
                }
                Change::Scale { axis, factor } => {
                    let mut k: [f64; 3] = p0.scale.into();
                    match axis {
                        Some(i) if !uniform => {
                            if let Some(v) = k.get_mut(i) {
                                *v *= factor;
                            }
                        }
                        _ => k = k.map(|v| v * factor),
                    }
                    Placement { scale: k.into(), ..*p0 }
                }
            };
            json!({ "id": id, "position": p.position, "rotation": p.rotation, "scale": p.scale })
        })
        .collect();
    // The app closes the step once the button is up (`ResinApp::ui`).
    app.session.begin_step();
    app.run("model.place", json!({ "models": models }));
}

/// Set fields of model `id`'s placement now.
fn place(app: &mut ResinApp, id: u64, fields: Value) {
    let mut entry = json!({ "id": id });
    if let (Some(e), Some(f)) = (entry.as_object_mut(), fields.as_object()) {
        e.extend(f.clone());
    }
    app.run("model.place", json!({ "models": [entry] }));
}

/// The tool's card: the lone selected model's placement, applied as it is edited. OK keeps the
/// changes and puts the tool away; Cancel puts back the placements the tool opened with.
pub fn panel(app: &mut ResinApp, ctx: &egui::Context) {
    let Some(op) = active(app) else {
        if app.tool.as_ref().is_some_and(|t| t.drag.is_none()) && app.mode != EditMode::Object {
            app.tool = None;
        }
        return;
    };
    let ids = selection(app);
    // Selecting other models keeps what was done to the last ones: Cancel now concerns these.
    if let Some(tool) = app.tool.as_mut()
        && !tool.before.iter().map(|(id, _)| *id).eq(ids.iter().copied())
    {
        let scene = &app.session.scene;
        tool.before = scene.models().iter().filter(|m| ids.contains(&m.id)).map(|m| (m.id, m.placement)).collect();
        tool.mark = app.session.history_mark();
    }
    let lone = match ids.as_slice() {
        [id] => app.session.scene.model(*id).cloned(),
        _ => None,
    };
    let t = Tokens::for_dark(ctx.global_style().visuals.dark_mode);
    let at = app.viewport_rect.map_or(egui::pos2(220.0, 140.0), |r| r.left_top() + vec2(SPACE_12, SPACE_12));
    let mut edit: Option<Value> = None;
    let mut uniform = app.tool.as_ref().is_some_and(|t| t.uniform);
    let card = Card {
        id: egui::Id::new("transform_tool"),
        title: i18n::tr(op.label()),
        icon: op.icon(),
        at,
        bounds: app.viewport_rect,
        width: FORM_CARD_WIDTH,
        buttons: Buttons::OkCancel { ok_enabled: true },
    };
    let action = card.show(ctx, &t, |ui| {
        let Some(m) = &lone else {
            ui_kit::hint(ui, &t, &i18n::format("tool.several", &[("n", &ids.len())]));
            if op == Op::Scale {
                ui.checkbox(&mut uniform, i18n::tr("tool.scale.uniform"));
            }
            return;
        };
        let pl = m.placement;
        match op {
            Op::Move => {
                if let Some(v) = xyz_rows(ui, &t, pl.position.into(), " mm", 0.1, -MAX_EXTENT..=MAX_EXTENT) {
                    edit = Some(json!({ "position": v }));
                }
                ui_kit::hint(ui, &t, i18n::tr("tool.move.hint"));
                if ui_kit::button(ui, i18n::tr("ui.reset")).on_hover_text(i18n::tr("tool.move.reset_tip")).clicked() {
                    edit = Some(json!({ "position": m.pivot }));
                }
            }
            Op::Rotate => {
                if let Some(v) = xyz_rows(ui, &t, pl.rotation.into(), "°", 1.0, -360.0..=360.0) {
                    edit = Some(json!({ "rotation": v }));
                }
                ui_kit::hint(ui, &t, i18n::tr("tool.rotate.hint"));
                if ui_kit::button(ui, i18n::tr("ui.reset")).clicked() {
                    edit = Some(json!({ "rotation": [0, 0, 0] }));
                }
            }
            Op::Scale => {
                ui_kit::row(ui, &t, i18n::tr("tool.scale.uniform"), |ui| ui.checkbox(&mut uniform, ""));
                let size: [f64; 3] = m.mesh.bounds().size().into();
                let scale: [f64; 3] = pl.scale.into();
                for axis in 0..3 {
                    let new = ui_kit::row(ui, &t, AXES[axis], |ui| {
                        let half = (ui.available_width() - ui.spacing().item_spacing.x) / 2.0;
                        let mut pct = scale[axis] * 100.0;
                        let pct_changed = ui
                            .add_sized(
                                [half, CONTROL_HEIGHT],
                                DragValue::new(&mut pct).speed(0.5).range(0.1..=100_000.0).suffix(" %").max_decimals(2),
                            )
                            .changed();
                        let base = size[axis];
                        let mut mm = base * scale[axis];
                        // A flat model has no size to aim for along its thin axis.
                        let mm_changed = ui
                            .add_enabled_ui(base > EPS, |ui| {
                                ui.add_sized(
                                    [half, CONTROL_HEIGHT],
                                    DragValue::new(&mut mm).speed(0.1).range(0.001..=MAX_EXTENT).suffix(" mm").max_decimals(3),
                                )
                            })
                            .inner
                            .changed();
                        if pct_changed {
                            Some(pct / 100.0)
                        } else if mm_changed && base > EPS {
                            Some(mm / base)
                        } else {
                            None
                        }
                    });
                    if let Some(k) = new.filter(|k| *k > 0.0) {
                        let mut s = scale;
                        if uniform {
                            let f = k / scale[axis].max(EPS);
                            s = s.map(|v| v * f);
                        } else {
                            s[axis] = k;
                        }
                        edit = Some(json!({ "scale": s }));
                    }
                }
                if ui_kit::button(ui, i18n::tr("ui.reset")).clicked() {
                    edit = Some(json!({ "scale": [1, 1, 1] }));
                }
            }
        }
        ui_kit::hint(ui, &t, i18n::tr("tool.gizmo_hint"));
    });
    if let Some(tool) = app.tool.as_mut() {
        tool.uniform = uniform;
    }
    if let (Some(fields), Some(m)) = (edit, &lone) {
        // A field dragged or typed in is one step: the app closes it once the gesture is over.
        app.session.begin_step();
        place(app, m.id, fields);
    }
    match action {
        Action::Ok => app.tool = None,
        Action::Cancel => cancel(app),
        // A tool card has no Restore defaults.
        Action::Reset | Action::None => {}
    }
}

/// Put the tool away as its Cancel does: the placements it opened with come back and its undo
/// steps go. Nothing when no tool is open.
pub fn cancel(app: &mut ResinApp) {
    if let Some(tool) = app.tool.take() {
        let models: Vec<Value> =
            tool.before.iter().map(|(id, p)| json!({ "id": id, "position": p.position, "rotation": p.rotation, "scale": p.scale })).collect();
        if !models.is_empty() && app.run("model.place", json!({ "models": models })).is_some() {
            // Steps of anything else (a model imported meanwhile) stay: they still describe the scene.
            app.session.forget_since(tool.mark, "model.place");
        }
    }
}

/// Rows X, Y, Z of number fields; the values when one changed.
fn xyz_rows(ui: &mut egui::Ui, t: &Tokens, mut v: [f64; 3], suffix: &str, speed: f64, range: std::ops::RangeInclusive<f64>) -> Option<[f64; 3]> {
    let mut changed = false;
    for (axis, x) in AXES.iter().zip(v.iter_mut()) {
        changed |= ui_kit::row(ui, t, axis, |ui| {
            ui.add_sized([ui.available_width(), CONTROL_HEIGHT], DragValue::new(x).speed(speed).range(range.clone()).suffix(suffix).max_decimals(3))
                .changed()
        });
    }
    changed.then_some(v)
}
