//! The Move, Rotate and Scale tools (T, R, S in Object edition): the gizmo on the selection and
//! a small panel of its placement, both live. Values are measured from the model as imported,
//! and every change is a `model.place` command built from the placements a drag began with, so
//! nothing piles up while dragging. A gesture (a drag, a field typed in) is one undo step, and
//! Cancel leaves the history as it was.
//!
//! With a tool open, the keyboard does what a drag does: X, Y or Z picks the axis and a number
//! the amount (`T X 50` moves 50 mm along X, `R 90` turns 90° about Z, `S X 2` doubles X), shown
//! live; Enter keeps it and puts the tool away, Esc puts the models back.

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
    /// While a gizmo drag runs: the centre it turns about, and the placements it began with and
    /// whether each model was lifted.
    drag: Option<(Vec3, Vec<(u64, Placement, bool)>)>,
    /// The selection's placements and whether each was lifted when the tool opened: what Cancel
    /// puts back.
    before: Vec<(u64, Placement, bool)>,
    /// The history then: Cancel forgets the steps made since.
    mark: u64,
    typed: Option<Typed>,
}

/// An amount being typed: from the placements it began with, about the centre it began with.
struct Typed {
    axis: Option<usize>,
    text: String,
    centre: Vec3,
    start: Vec<(u64, Placement, bool)>,
    /// The history when it began: Esc forgets the steps made since.
    mark: u64,
}

/// Enough digits for any amount the engine takes; more is a key held down.
const MAX_TYPED: usize = 12;

fn placements(app: &ResinApp) -> Vec<(u64, Placement, bool)> {
    let scene = &app.session.scene;
    scene.models().iter().filter(|m| scene.selected_models.contains(&m.id)).map(|m| (m.id, m.placement, m.lifted)).collect()
}

/// Choose the tool for `op` on the selected models.
pub fn open(app: &mut ResinApp, op: Op) {
    abort_typed(app);
    if app.mode != EditMode::Object {
        app.status = Some((i18n::format("tool.needs_object_edition", &[("tool", &i18n::tr(op.label()))]), true));
        return;
    }
    if app.session.scene.selected_models.is_empty() {
        app.status = Some((i18n::tr("reason.select_model").into(), true));
        return;
    }
    let uniform = app.tool.as_ref().is_none_or(|t| t.uniform);
    app.tool = Some(Tool { op, uniform, drag: None, before: placements(app), mark: app.session.history_mark(), typed: None });
}

/// Where the selection turns about: a lone model's pivot, or the centre of several.
fn centre(app: &ResinApp) -> Vec3 {
    let scene = &app.session.scene;
    let ids = selection(app);
    match ids.as_slice() {
        [id] => scene.model(*id).map_or(Vec3::ZERO, |m| m.placement.position),
        _ => scene.bounds_of(&ids).center(),
    }
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
    if g.started {
        abort_typed(app);
    }
    let (c, start) = (centre(app), placements(app));
    let Some(tool) = app.tool.as_mut() else { return };
    if g.started || tool.drag.is_none() {
        tool.drag = Some((c, start));
    }
    let uniform = tool.uniform;
    let Some((centre, start)) = tool.drag.clone() else { return };
    if g.done {
        tool.drag = None;
    }
    let models = placed(&start, centre, g.change, uniform);
    // The app closes the step once the button is up (`ResinApp::ui`).
    app.session.begin_step();
    app.run("model.place", json!({ "models": models }));
}

/// `model.place` entries for `change` applied to the placements `start`, turning about `centre`.
fn placed(start: &[(u64, Placement, bool)], centre: Vec3, change: Change, uniform: bool) -> Vec<Value> {
    start
        .iter()
        .map(|(id, p0, lifted0)| {
            let p = match change {
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
            // Said outright: from the second frame on, the start's Z is no longer the lifted
            // model's, and the engine would read a turn that stopped on a snap as a move in Z.
            let lifted = match change {
                Change::Translate(v) => *lifted0 && v.z == 0.0,
                _ => *lifted0,
            };
            json!({ "id": id, "position": p.position, "rotation": p.rotation, "scale": p.scale, "lifted": lifted })
        })
        .collect()
}

/// Whether an amount is being typed: its undo step stays open meanwhile.
pub fn typing(app: &ResinApp) -> bool {
    app.tool.as_ref().is_some_and(|t| t.typed.is_some())
}

/// The tool's op, when it can take typed keys: open, active and not dragged.
fn typable(app: &ResinApp) -> Option<Op> {
    let op = active(app)?;
    app.tool.as_ref().is_some_and(|t| t.drag.is_none()).then_some(op)
}

/// Start typing, from the selection as it is now.
fn begin_typed(app: &mut ResinApp) {
    if typing(app) {
        return;
    }
    // The typed amount is a step of its own, apart from whatever came before.
    app.session.end_step();
    let typed = Typed { axis: None, text: String::new(), centre: centre(app), start: placements(app), mark: app.session.history_mark() };
    if let Some(tool) = app.tool.as_mut() {
        tool.typed = Some(typed);
    }
}

/// A key for the typed amount: X, Y or Z picks the axis (again: none), Backspace takes a
/// character back, Enter keeps the amount and puts the tool away, Esc puts the models back.
/// Returns whether the key was used.
pub fn key(app: &mut ResinApp, key: egui::Key, mods: egui::Modifiers) -> bool {
    use egui::Key;
    if !mods.is_none() || typable(app).is_none() {
        return false;
    }
    match key {
        Key::X | Key::Y | Key::Z => {
            let axis = match key {
                Key::X => 0,
                Key::Y => 1,
                _ => 2,
            };
            begin_typed(app);
            if let Some(t) = app.tool.as_mut().and_then(|t| t.typed.as_mut()) {
                t.axis = (t.axis != Some(axis)).then_some(axis);
            }
            show_typed(app);
        }
        Key::Backspace if typing(app) => {
            if let Some(t) = app.tool.as_mut().and_then(|t| t.typed.as_mut()) {
                t.text.pop();
            }
            show_typed(app);
        }
        Key::Enter if typing(app) => commit_typed(app),
        Key::Escape if typing(app) => abort_typed(app),
        _ => return false,
    }
    true
}

/// Text typed while a tool is open: digits, a decimal point (`.` or `,`) and `-`, which turns
/// the sign over. Anything else is not the amount's.
pub fn text(app: &mut ResinApp, text: &str) {
    if typable(app).is_none() {
        return;
    }
    for c in text.chars() {
        if !(c.is_ascii_digit() || matches!(c, '.' | ',' | '-')) {
            continue;
        }
        begin_typed(app);
        let Some(t) = app.tool.as_mut().and_then(|t| t.typed.as_mut()) else { return };
        match c {
            '-' => {
                if t.text.starts_with('-') {
                    t.text.remove(0);
                } else {
                    t.text.insert(0, '-');
                }
            }
            '.' | ',' if t.text.contains('.') => {}
            _ if t.text.len() >= MAX_TYPED => {}
            '.' | ',' => t.text.push('.'),
            _ => t.text.push(c),
        }
        show_typed(app);
    }
}

/// The typed amount, when it reads as one.
fn amount(text: &str) -> Option<f64> {
    text.parse::<f64>().ok().filter(|v| v.is_finite())
}

/// What the typed amount does: nothing yet while it is empty, or a move has no axis.
fn typed_change(op: Op, axis: Option<usize>, text: &str) -> Change {
    let v = amount(text);
    match op {
        Op::Move => {
            let d = v.unwrap_or(0.0).clamp(-MAX_EXTENT, MAX_EXTENT);
            Change::Translate(axis.and_then(|a| DIRS.get(a)).map_or(Vec3::ZERO, |&dir| dir * d))
        }
        // About Z by default: the plate's own axis.
        Op::Rotate => Change::Rotate { axis: axis.unwrap_or(2), angle: v.unwrap_or(0.0).clamp(-360.0, 360.0).to_radians() },
        Op::Scale => Change::Scale { axis, factor: v.filter(|k| *k > 0.0).unwrap_or(1.0) },
    }
}

/// Place the selection as the typed amount says, from where the typing began.
fn show_typed(app: &mut ResinApp) {
    let Some(tool) = app.tool.as_ref() else { return };
    let Some(t) = &tool.typed else { return };
    // An axis typed for Scale is that axis alone, whatever the Uniform box says.
    let models = placed(&t.start, t.centre, typed_change(tool.op, t.axis, &t.text), false);
    app.session.begin_step();
    app.run("model.place", json!({ "models": models }));
}

/// Keep the typed amount and put the tool away, as Enter does.
fn commit_typed(app: &mut ResinApp) {
    let Some(t) = app.tool.take().and_then(|t| t.typed) else { return };
    // Nothing typed that changed anything: no empty step left in the history.
    if t.start == placements_of(app, &t.start) {
        app.session.forget_since(t.mark, "model.place");
    }
    app.session.end_step();
}

/// The placements now of the models in `start`.
fn placements_of(app: &ResinApp, start: &[(u64, Placement, bool)]) -> Vec<(u64, Placement, bool)> {
    let scene = &app.session.scene;
    start.iter().filter_map(|(id, ..)| scene.model(*id).map(|m| (m.id, m.placement, m.lifted))).collect()
}

/// Put the models back where the typing began and forget its step; the tool stays open. Nothing
/// when nothing is typed.
pub fn abort_typed(app: &mut ResinApp) {
    let Some(t) = app.tool.as_mut().and_then(|t| t.typed.take()) else { return };
    let models: Vec<Value> = t
        .start
        .iter()
        .map(|(id, p, lifted)| json!({ "id": id, "position": p.position, "rotation": p.rotation, "scale": p.scale, "lifted": lifted }))
        .collect();
    if !models.is_empty() && app.run("model.place", json!({ "models": models })).is_some() {
        app.session.forget_since(t.mark, "model.place");
    }
}

/// The line the typed axis runs along, through where the typing began: world X, Y or Z, or the
/// lone model's own axis while scaling.
pub fn typed_axis(app: &ResinApp) -> Option<(Vec3, Vec3, usize)> {
    let tool = app.tool.as_ref()?;
    let t = tool.typed.as_ref()?;
    let axis = match (tool.op, t.axis) {
        (_, Some(a)) => a,
        (Op::Rotate, None) => 2,
        _ => return None,
    };
    let dir = *DIRS.get(axis)?;
    let dir = match (tool.op, t.start.as_slice()) {
        (Op::Scale, [(_, p, _)]) => Affine3::rotation_xyz(p.rotation).vector(dir),
        _ => dir,
    };
    Some((t.centre, dir, axis))
}

/// What the card says of the typed amount.
fn typed_line(op: Op, t: &Typed) -> String {
    let axis = match (op, t.axis) {
        (_, Some(a)) => AXES.get(a).copied().unwrap_or_default().to_string(),
        (Op::Move, None) => return i18n::tr("tool.typed.pick_axis").to_string(),
        (Op::Rotate, None) => "Z".to_string(),
        (Op::Scale, None) => i18n::tr("tool.typed.all_axes").to_string(),
    };
    let unit = match op {
        Op::Move => " mm",
        Op::Rotate => "°",
        Op::Scale => " ×",
    };
    let value = if t.text.is_empty() { "…".to_string() } else { format!("{}{unit}", t.text) };
    i18n::format("tool.typed.entry", &[("axis", &axis), ("value", &value)])
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
    // The selection changed under the typing (a click in the view): what was typed is kept.
    let now = placements(app);
    if let Some(tool) = app.tool.as_mut()
        && tool.typed.as_ref().is_some_and(|t| !t.start.iter().map(|(id, ..)| *id).eq(now.iter().map(|(id, ..)| *id)))
    {
        tool.typed = None;
    }
    let Some(op) = active(app) else {
        if app.tool.as_ref().is_some_and(|t| t.drag.is_none()) && app.mode != EditMode::Object {
            app.tool = None;
        }
        return;
    };
    let ids = selection(app);
    // Selecting other models keeps what was done to the last ones: Cancel now concerns these.
    let (now, mark) = (placements(app), app.session.history_mark());
    if let Some(tool) = app.tool.as_mut()
        && !tool.before.iter().map(|(id, ..)| *id).eq(now.iter().map(|(id, ..)| *id))
    {
        tool.before = now;
        tool.mark = mark;
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
    let typed = app.tool.as_ref().and_then(|tool| tool.typed.as_ref()).map(|ty| typed_line(op, ty));
    let action = card.show(ctx, &t, |ui| {
        if let Some(line) = &typed {
            ui.label(egui::RichText::new(line).strong());
            ui_kit::hint(ui, &t, &i18n::format("tool.typed.keys", &[("enter", &i18n::tr("key.enter")), ("escape", &i18n::tr("key.escape"))]));
            ui.add_space(SPACE_12);
        }
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
        if typed.is_none() {
            ui_kit::hint(ui, &t, i18n::tr("tool.typed.hint"));
        }
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
        // `lifted` too: a Z move let the models go, and Cancel must give them back their lift.
        let models: Vec<Value> = tool
            .before
            .iter()
            .map(|(id, p, lifted)| json!({ "id": id, "position": p.position, "rotation": p.rotation, "scale": p.scale, "lifted": lifted }))
            .collect();
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
