//! The support settings card, shown for as long as Support edition lasts: a tab per part of a
//! support (Tip, Body, Base, Raft) beside a side view whose dimensions light up with their field,
//! Restore defaults, and the strip that picks the preset in use. Every edit runs
//! `support.configure` at once; settings are not undo steps, and `ResinApp::run` keeps them in
//! the preferences.

use egui::{Painter, Pos2, Rect, Stroke, pos2, vec2};
use resin_engine::{BodyShape, ContactShape, GROUPS, PresetId, RaftEdge, SEGMENTS, SupportPreset, range};
use serde_json::{Value, json};

use crate::theme::Tokens;
use crate::theme::metrics::*;
use crate::ui_kit::{self, Action, Buttons, Card, Confirm, FigureField, FigureRow, FigureSlot};
use crate::{EditMode, ResinApp, i18n};

/// The card's tabs: the catalog ids of their names, in order.
pub const TABS: [&str; 4] = ["supports.tab.tip", "supports.tab.body", "supports.tab.base", "supports.tab.raft"];
const TIP: usize = 0;
const BODY: usize = 1;
const BASE: usize = 2;

pub fn preset_label(id: PresetId) -> &'static str {
    match id {
        PresetId::Light => "supports.preset.light",
        PresetId::Medium => "supports.preset.medium",
        PresetId::Heavy => "supports.preset.heavy",
    }
}

pub(crate) const CARD_ID: &str = "support_settings";
const CONFIRM_ID: &str = "support_settings_reset";
/// The side view is as tall on every tab as on Tip, the tab with most dimensions.
const TIP_DIMENSIONS: usize = 4;

/// Run `support.configure` with `params`: what every field of the card does.
pub(crate) fn configure(app: &mut ResinApp, params: Value) {
    app.run("support.configure", params);
}

/// The card, in Support edition only, and Restore defaults' confirmation while it is open.
pub fn panel(app: &mut ResinApp, ctx: &egui::Context) {
    if app.mode != EditMode::Support {
        app.confirm_reset = false;
        return;
    }
    let t = Tokens::for_dark(ctx.global_style().visuals.dark_mode);
    let at = crate::models_card::beside(app);
    let settings = *app.session.support_settings();
    let active = settings.active;
    let before = *settings.current();
    let mut p = before;
    let mut tab = app.settings_tab.min(TABS.len() - 1);
    let mut preset = PresetId::ALL.iter().position(|id| *id == active).unwrap_or(0);
    let mut reset = false;
    let mut new_active = None;
    let preset_name = i18n::tr(preset_label(active));
    let card = Card {
        id: egui::Id::new(CARD_ID),
        title: i18n::tr("supports.title"),
        icon: "cone",
        at,
        bounds: app.viewport_rect,
        width: FORM_CARD_WIDTH,
        buttons: Buttons::None,
    };
    card.show(ctx, &t, |ui| {
        let tabs = TABS.map(i18n::tr);
        ui_kit::segmented(ui, &t, &mut tab, &tabs);
        match tab {
            TIP => tip_rows(ui, &t, &mut p),
            BODY => body_rows(ui, &t, &mut p),
            BASE => base_rows(ui, &t, &mut p),
            _ => raft_rows(ui, &t, &mut p),
        }
        if ui_kit::action_row(ui, &t, i18n::tr("ui.restore_defaults")).clicked() {
            reset = true;
        }
        ui_kit::hint(ui, &t, i18n::tr("supports.preset"));
        let presets = PresetId::ALL.map(|id| i18n::tr(preset_label(id)));
        if ui_kit::segmented(ui, &t, &mut preset, &presets) {
            new_active = PresetId::ALL.get(preset).copied().filter(|id| *id != active);
        }
    });
    app.settings_tab = tab;
    if let Some(edit) = changes(&before, &p) {
        configure(app, edit);
    }
    if let Some(id) = new_active {
        configure(app, json!({ "active": id.id() }));
    }
    if reset {
        app.confirm_reset = true;
    }
    if app.confirm_reset {
        let message = i18n::format("supports.reset_confirm", &[("preset", &preset_name)]);
        let confirm = Confirm {
            id: egui::Id::new(CONFIRM_ID),
            icon: "undo",
            title: i18n::tr("supports.reset_title"),
            message: &message,
            ok: "supports.reset_ok",
        };
        match confirm.show(ctx, &t) {
            Action::Ok => {
                app.confirm_reset = false;
                app.run("support.reset_preset", json!({ "preset": active.id() }));
            }
            Action::Cancel => app.confirm_reset = false,
            Action::Reset | Action::Fold | Action::None => {}
        }
    }
}

/// `support.configure`'s parameters for what differs from `before` to `after`.
fn changes(before: &SupportPreset, after: &SupportPreset) -> Option<Value> {
    let (Ok(a), Ok(b)) = (serde_json::to_value(before), serde_json::to_value(after)) else { return None };
    let mut out = serde_json::Map::new();
    for group in GROUPS {
        let (Some(ga), Some(gb)) = (a.get(group).and_then(Value::as_object), b.get(group).and_then(Value::as_object)) else { continue };
        let changed: serde_json::Map<String, Value> =
            gb.iter().filter(|(k, v)| ga.get(*k) != Some(*v)).map(|(k, v)| (k.clone(), v.clone())).collect();
        if !changed.is_empty() {
            out.insert(group.into(), Value::Object(changed));
        }
    }
    (!out.is_empty()).then_some(Value::Object(out))
}

fn field<'a>(label: &'static str, value: &'a mut f64, group: &str, key: &str, suffix: &'a str) -> FigureField<'a> {
    let degrees = suffix == "°";
    FigureField {
        label: i18n::tr(label),
        value,
        range: range(group, key).unwrap_or(0.0..=0.0),
        suffix,
        decimals: if degrees { 1 } else { 2 },
        speed: if degrees { 0.5 } else { 0.01 },
        enabled: true,
    }
}

/// A number row without a dimension in the figure.
fn number_row(ui: &mut egui::Ui, t: &Tokens, label: &'static str, value: &mut f64, group: &str, key: &str, suffix: &str) {
    let r = range(group, key).unwrap_or(0.0..=0.0);
    ui_kit::row(ui, t, i18n::tr(label), |ui| ui_kit::number_field(ui, value, r, 0.5, suffix, 1));
}

fn shape_row<S: Copy + PartialEq>(
    ui: &mut egui::Ui,
    t: &Tokens,
    label: &'static str,
    id: &str,
    value: &mut S,
    all: &[S],
    name: impl Fn(S) -> (&'static str, &'static str),
) {
    let mut chosen = name(*value).0.to_string();
    let options: Vec<(&str, &'static str)> = all.iter().map(|s| name(*s)).collect();
    ui_kit::choice_row(ui, t, i18n::tr(label), id, &mut chosen, &options);
    if let Some(s) = all.iter().find(|s| name(**s).0 == chosen) {
        *value = *s;
    }
}

fn contact_name(s: ContactShape) -> (&'static str, &'static str) {
    match s {
        ContactShape::Cone => (s.id(), "supports.shape.cone"),
        ContactShape::Sphere => (s.id(), "supports.shape.sphere"),
    }
}

fn edge_name(e: RaftEdge) -> (&'static str, &'static str) {
    match e {
        RaftEdge::Straight => (e.id(), "supports.raft.edge.straight"),
        RaftEdge::Undercut => (e.id(), "supports.raft.edge.undercut"),
        RaftEdge::Chamfer => (e.id(), "supports.raft.edge.chamfer"),
    }
}

fn body_name(s: BodyShape) -> (&'static str, &'static str) {
    match s {
        BodyShape::Cylinder => (s.id(), "supports.shape.cylinder"),
        BodyShape::Prism => (s.id(), "supports.shape.prism"),
    }
}

fn tip_rows(ui: &mut egui::Ui, t: &Tokens, p: &mut SupportPreset) {
    let shown = *p;
    let tip = &mut p.tip;
    ui_kit::figure_rows(
        ui,
        t,
        "tip",
        ui_kit::figure_height(TIP_DIMENSIONS),
        &mut [
            field("supports.tip.contact", &mut tip.contact_diameter, "tip", "contact_diameter", " mm").into(),
            field("supports.tip.length", &mut tip.length, "tip", "length", " mm").into(),
            field("supports.tip.upper", &mut tip.upper_diameter, "tip", "upper_diameter", " mm").into(),
            field("supports.tip.penetration", &mut tip.penetration, "tip", "penetration", " mm").into(),
        ],
        |painter, rect, slots| {
            let s = Side::new(&shown, rect);
            s.paint(painter, t);
            let mid = s.x;
            s.width_dim(painter, t, slot(slots, 0), s.tip_top, s.contact / 2.0, mid);
            s.height_dim(painter, t, slot(slots, 1), mid + s.upper / 2.0 + SPACE_6, s.tip_top, s.tip_bottom);
            s.width_dim(painter, t, slot(slots, 2), s.tip_bottom, s.upper / 2.0, mid);
            s.height_dim(painter, t, slot(slots, 3), mid - s.upper / 2.0 - SPACE_6, s.tip_top, s.model_bottom);
        },
    );
    shape_row(ui, t, "supports.tip.shape", "contact_shape", &mut p.tip.contact_shape, &ContactShape::ALL, contact_name);
    number_row(ui, t, "supports.tip.max_angle", &mut p.tip.max_angle, "tip", "max_angle", "°");
}

fn body_rows(ui: &mut egui::Ui, t: &Tokens, p: &mut SupportPreset) {
    let shown = *p;
    ui_kit::figure_rows(
        ui,
        t,
        "body",
        ui_kit::figure_height(TIP_DIMENSIONS),
        &mut [field("supports.body.diameter", &mut p.body.diameter, "body", "diameter", " mm").into()],
        |painter, rect, slots| {
            let s = Side::new(&shown, rect);
            s.paint(painter, t);
            s.width_dim(painter, t, slot(slots, 0), (s.tip_bottom + s.base_top) / 2.0, s.body / 2.0, s.x);
        },
    );
    shape_row(ui, t, "supports.body.shape", "body_shape", &mut p.body.shape, &BodyShape::ALL, body_name);
    number_row(ui, t, "supports.body.max_angle", &mut p.body.max_angle, "body", "max_angle", "°");
    number_row(ui, t, "supports.body.clearance", &mut p.body.clearance, "body", "clearance", " mm");
    ui_kit::row(ui, t, i18n::tr("supports.body.segments"), |ui| ui_kit::number_field(ui, &mut p.body.segments, SEGMENTS, 0.1, "", 0));
}

fn base_rows(ui: &mut egui::Ui, t: &Tokens, p: &mut SupportPreset) {
    let shown = *p;
    let base = &mut p.base;
    ui_kit::figure_rows(
        ui,
        t,
        "base",
        ui_kit::figure_height(TIP_DIMENSIONS),
        &mut [
            field("supports.base.diameter", &mut base.diameter, "base", "diameter", " mm").into(),
            field("supports.base.height", &mut base.height, "base", "height", " mm").into(),
        ],
        |painter, rect, slots| {
            let s = Side::new(&shown, rect);
            s.paint(painter, t);
            s.width_dim(painter, t, slot(slots, 0), s.base_bottom + SPACE_4, s.base / 2.0, s.x);
            s.height_dim(painter, t, slot(slots, 1), s.x + s.base / 2.0 + SPACE_6, s.base_top, s.base_bottom);
        },
    );
}

fn raft_rows(ui: &mut egui::Ui, t: &Tokens, p: &mut SupportPreset) {
    let shown = *p;
    let raft = &mut p.raft;
    // Off, the raft's sizes stay in view with the values it gets back when turned on again.
    let on = raft.enabled;
    let sloped = on && raft.edge != RaftEdge::Straight;
    let rim = on && raft.rim_height > 0.0;
    ui_kit::figure_rows(
        ui,
        t,
        "raft",
        ui_kit::figure_height(TIP_DIMENSIONS),
        &mut [
            FigureRow::Check { label: i18n::tr("supports.raft.enabled"), help: i18n::tr("supports.raft.enabled.help"), value: &mut raft.enabled },
            FigureField { enabled: sloped, ..field("supports.raft.edge_angle", &mut raft.edge_angle, "raft", "edge_angle", "°") }.into(),
            FigureField { enabled: on, ..field("supports.raft.thickness", &mut raft.thickness, "raft", "thickness", " mm") }.into(),
            FigureField { enabled: on, ..field("supports.raft.margin", &mut raft.margin, "raft", "margin", " mm") }.into(),
            FigureField { enabled: on, ..field("supports.raft.rim_height", &mut raft.rim_height, "raft", "rim_height", " mm") }.into(),
            FigureField { enabled: rim, ..field("supports.raft.rim_width", &mut raft.rim_width, "raft", "rim_width", " mm") }.into(),
        ],
        |painter, rect, slots| {
            // As tall as on the other tabs, where they put it: this tab's extra rows go below.
            let rect = Rect::from_min_size(rect.min, vec2(rect.width(), rect.height().min(ui_kit::figure_height(TIP_DIMENSIONS))));
            let s = Side::new(&shown, rect);
            s.paint(painter, t);
            // No raft is drawn, so there is nothing to dimension.
            if s.raft {
                s.edge_dim(painter, t, slot(slots, 1));
                s.height_dim(painter, t, slot(slots, 2), s.x + s.raft_half.max(s.raft_bottom_half) + SPACE_4, s.base_bottom, s.plate);
                let base_edge = s.x + s.base / 2.0;
                let raft_edge = s.x + s.raft_half;
                s.width_dim(painter, t, slot(slots, 3), s.base_bottom - SPACE_4, (raft_edge - base_edge) / 2.0, (raft_edge + base_edge) / 2.0);
                if s.rim_top < s.base_bottom {
                    let left = s.x - s.raft_half.max(s.rim_top_half) - SPACE_4;
                    s.height_dim(painter, t, slot(slots, 4), left, s.rim_top, s.base_bottom);
                    let (outer, inner) = (s.x - s.raft_half.min(s.rim_top_half), s.x - s.rim_inner);
                    s.width_dim(painter, t, slot(slots, 5), s.rim_top - SPACE_4, (inner - outer) / 2.0, (inner + outer) / 2.0);
                }
            }
        },
    );
    shape_row(ui, t, "supports.raft.edge", "raft_edge", &mut p.raft.edge, &RaftEdge::ALL, edge_name);
    number_row(ui, t, "supports.raft.merge_distance", &mut p.raft.merge_distance, "raft", "merge_distance", " mm");
}

fn slot(slots: &[FigureSlot], i: usize) -> Option<&FigureSlot> {
    slots.get(i)
}

/// One support seen from the side, in figure coordinates: loosely to scale (the base fills most
/// of the width), each part clamped so it stays visible and the whole fits.
struct Side {
    x: f32,
    model_bottom: f32,
    tip_top: f32,
    tip_bottom: f32,
    base_top: f32,
    base_bottom: f32,
    plate: f32,
    contact: f32,
    upper: f32,
    body: f32,
    base: f32,
    /// Half the raft's width where the bases stand, at its bottom, and at its rim's top.
    raft_half: f32,
    raft_bottom_half: f32,
    rim_top_half: f32,
    /// The rim's top, at `base_bottom` without one, and half the width of the hollow inside it.
    rim_top: f32,
    rim_inner: f32,
    raft: bool,
    sloped: bool,
    sphere: bool,
    prism: bool,
    rect: Rect,
}

impl Side {
    fn new(p: &SupportPreset, rect: Rect) -> Side {
        let r = rect.shrink2(vec2(SPACE_4, SPACE_4));
        let raft = p.raft.enabled;
        let margin = if raft { p.raft.margin as f32 } else { 0.0 };
        // Millimetres to points: the base takes most of the width, so the tip stays legible, but
        // the raft's whole foot fits while it can, so its margin shows; past the smallest scale
        // the margin is cut at the figure's edge.
        let slope_mm = if raft { p.raft.slope() } else { 0.0 };
        let overhang_mm = (p.raft.thickness * slope_mm).max(-p.raft.rim_height * slope_mm).max(0.0);
        let foot = p.base.diameter + 2.0 * (margin as f64 + overhang_mm);
        let k = (r.width() * 0.65 / p.base.diameter as f32).min(r.width() * 0.9 / foot as f32).clamp(4.0, 60.0);
        let w = |mm: f64, min: f32| (mm as f32 * k).clamp(min, r.width());
        let h = |mm: f64, min: f32, max: f32| (mm as f32 * k).clamp(min, max);
        let model_bottom = r.top() + 16.0;
        let penetration = h(p.tip.penetration, 3.0, 10.0);
        let tip_top = model_bottom - penetration;
        let tip_bottom = tip_top + h(p.tip.length, 14.0, r.height() * 0.3);
        let plate = r.bottom() - SPACE_2;
        let raft_h = if raft { h(p.raft.thickness, 4.0, 12.0) } else { 0.0 };
        let base_bottom = plate - raft_h;
        let base_top = base_bottom - h(p.base.height, 4.0, 14.0);
        let base = w(p.base.diameter, 12.0).min(r.width() * 0.8);
        let body = w(p.body.diameter, 4.0).min(base);
        let upper = w(p.tip.upper_diameter, 3.0).min(body);
        let contact = w(p.tip.contact_diameter, 2.0).min(upper);
        let x = r.center().x;
        // The edge's run per point of height, drawn at its true angle.
        let slope = p.raft.slope() as f32;
        let edge = |half: f32, below: f32| (half + below * slope).clamp(base / 4.0, r.width() / 2.0);
        let rim_h = if raft && p.raft.rim_height > 0.0 { h(p.raft.rim_height, 4.0, 12.0) } else { 0.0 };
        // Room for the edge's widest point: a chamfer's foot, an undercut rim's top.
        let overhang = (raft_h * slope).max(-rim_h * slope).max(0.0);
        let raft_half = (base / 2.0 + margin * k).min(r.width() / 2.0 - overhang);
        let rim_top_half = edge(raft_half, -rim_h);
        let rim_inner = (raft_half.min(rim_top_half) - w(p.raft.rim_width, 3.0)).max(base / 2.0);
        Side {
            x,
            model_bottom,
            tip_top,
            tip_bottom,
            base_top,
            base_bottom,
            plate,
            contact,
            upper,
            body,
            base,
            raft_half,
            raft_bottom_half: edge(raft_half, raft_h),
            rim_top_half,
            rim_top: base_bottom - rim_h,
            rim_inner,
            raft,
            sloped: p.raft.edge != RaftEdge::Straight,
            sphere: p.tip.contact_shape == ContactShape::Sphere,
            prism: p.body.shape == BodyShape::Prism,
            rect,
        }
    }

    fn paint(&self, painter: &Painter, t: &Tokens) {
        let fill = t.selected_bg;
        let edge = Stroke::new(1.0, t.form_dim);
        let x = self.x;
        // The model's underside, hatched, with the contact sunk into it.
        let model = Rect::from_min_max(pos2(self.rect.left(), self.rect.top()), pos2(self.rect.right(), self.model_bottom));
        let hatch = painter.with_clip_rect(model);
        let mut hx = model.left() - model.height();
        while hx < model.right() {
            hatch.line_segment([pos2(hx, model.bottom()), pos2(hx + model.height(), model.top())], Stroke::new(1.0, t.form_dim.gamma_multiply(0.5)));
            hx += 6.0;
        }
        painter.hline(model.x_range(), self.model_bottom, edge);
        // Raft, base, body, then the tip over them.
        if self.raft {
            let (bottom, top) = (self.raft_bottom_half, self.raft_half);
            let plate =
                vec![pos2(x - bottom, self.plate), pos2(x + bottom, self.plate), pos2(x + top, self.base_bottom), pos2(x - top, self.base_bottom)];
            painter.add(egui::Shape::convex_polygon(plate, fill, edge));
            if self.rim_top < self.base_bottom {
                for side in [-1.0, 1.0] {
                    let at = |half: f32, y: f32| pos2(x + side * half, y);
                    let rim = vec![
                        at(self.rim_inner, self.base_bottom),
                        at(top, self.base_bottom),
                        at(self.rim_top_half, self.rim_top),
                        at(self.rim_inner, self.rim_top),
                    ];
                    painter.add(egui::Shape::convex_polygon(rim, fill, edge));
                }
            }
        }
        // The base is a cone from its diameter on the raft to the body's.
        let base = vec![
            pos2(x - self.base / 2.0, self.base_bottom),
            pos2(x + self.base / 2.0, self.base_bottom),
            pos2(x + self.body / 2.0, self.base_top),
            pos2(x - self.body / 2.0, self.base_top),
        ];
        painter.add(egui::Shape::convex_polygon(base, fill, edge));
        let body = Rect::from_min_max(pos2(x - self.body / 2.0, self.tip_bottom), pos2(x + self.body / 2.0, self.base_top));
        painter.rect(body, 0.0, fill, edge, egui::StrokeKind::Middle);
        if self.prism {
            // A prism shows its front edge.
            painter.vline(x, body.y_range(), Stroke::new(1.0, t.form_dim.gamma_multiply(0.6)));
        }
        let tip = vec![
            pos2(x - self.contact / 2.0, self.tip_top),
            pos2(x + self.contact / 2.0, self.tip_top),
            pos2(x + self.upper / 2.0, self.tip_bottom),
            pos2(x - self.upper / 2.0, self.tip_bottom),
        ];
        painter.add(egui::Shape::convex_polygon(tip, fill, edge));
        if self.sphere {
            painter.circle(pos2(x, self.tip_top), self.contact / 2.0, fill, edge);
        }
        painter.hline(self.rect.x_range(), self.plate, Stroke::new(1.5, t.form_ink));
    }

    fn ink(t: &Tokens, s: Option<&FigureSlot>) -> Stroke {
        if s.is_some_and(|s| s.hot) { Stroke::new(1.6, t.form_primary) } else { Stroke::new(1.0, t.form_dim) }
    }

    /// A thin leader from `from` to the field's left edge.
    fn leader(painter: &Painter, t: &Tokens, from: Pos2, s: Option<&FigureSlot>) {
        let Some(s) = s else { return };
        let to = pos2(s.rect.left() - SPACE_2, s.rect.center().y);
        let stroke = if s.hot { Stroke::new(1.0, t.form_primary) } else { Stroke::new(0.5, t.form_dim.gamma_multiply(0.6)) };
        painter.line_segment([from, to], stroke);
    }

    /// A width across the support at height `y`, `half` either side of `x`.
    fn width_dim(&self, painter: &Painter, t: &Tokens, s: Option<&FigureSlot>, y: f32, half: f32, x: f32) {
        let ink = Self::ink(t, s);
        let (a, b) = (pos2(x - half, y), pos2(x + half, y));
        painter.line_segment([a, b], ink);
        for end in [a, b] {
            painter.vline(end.x, end.y - 3.0..=end.y + 3.0, ink);
        }
        Self::leader(painter, t, b, s);
    }

    /// A height at `x` from `top` to `bottom`.
    fn height_dim(&self, painter: &Painter, t: &Tokens, s: Option<&FigureSlot>, x: f32, top: f32, bottom: f32) {
        let ink = Self::ink(t, s);
        painter.vline(x, top..=bottom, ink);
        for y in [top, bottom] {
            painter.hline(x - 3.0..=x + 3.0, y, ink);
        }
        Self::leader(painter, t, pos2(x, (top + bottom) / 2.0), s);
    }

    /// The raft's sloped edge and the angle it makes with the plate: inside the raft for a
    /// chamfer, outside it for an undercut, where the edge overhangs.
    fn edge_dim(&self, painter: &Painter, t: &Tokens, s: Option<&FigureSlot>) {
        if !self.raft {
            return;
        }
        let ink = Self::ink(t, s);
        let corner = pos2(self.x + self.raft_bottom_half, self.plate);
        let top = pos2(self.x + self.raft_half, self.base_bottom);
        if !self.sloped {
            Self::leader(painter, t, pos2(corner.x, (corner.y + top.y) / 2.0), s);
            return;
        }
        painter.line_segment([corner, top], ink);
        let r = 9.0;
        let rise = (self.plate - self.base_bottom).max(0.01);
        let run = (corner.x - top.x).abs().max(0.01);
        let angle = rise.atan2(run);
        // A chamfer's angle opens inwards (towards -X), an undercut's outwards.
        let towards = if top.x <= corner.x { -1.0 } else { 1.0 };
        let arc: Vec<Pos2> =
            (0..=8).map(|i| angle * i as f32 / 8.0).map(|a| pos2(corner.x + towards * r * a.cos(), corner.y - r * a.sin())).collect();
        painter.add(egui::Shape::line(arc, ink));
        Self::leader(painter, t, pos2(corner.x + towards * r, corner.y - SPACE_2), s);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prefs::Prefs;
    use crate::transform;

    fn app_with_a_model() -> (ResinApp, u64) {
        let dir = std::env::temp_dir().join(format!("resin-ui-settings-{}-{:?}", std::process::id(), std::thread::current().id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("tetra.stl");
        let tri = |a: [f32; 3], b: [f32; 3], c: [f32; 3]| {
            format!(
                "facet normal 0 0 0\nouter loop\nvertex {:?} {:?} {:?}\nvertex {:?} {:?} {:?}\nvertex {:?} {:?} {:?}\nendloop\nendfacet\n",
                a[0], a[1], a[2], b[0], b[1], b[2], c[0], c[1], c[2]
            )
        };
        let (o, x, y, z) = ([0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [0.0, 10.0, 0.0], [0.0, 0.0, 10.0]);
        let text = format!("solid t\n{}{}{}{}endsolid t\n", tri(o, y, x), tri(o, x, z), tri(o, z, y), tri(x, y, z));
        std::fs::write(&path, text).unwrap();
        let mut app = ResinApp::build(Prefs::default(), &[]);
        let id = app.run("file.import_stl", json!({ "path": path, "fit": false })).unwrap()["id"].as_u64().unwrap();
        std::fs::remove_dir_all(dir).ok();
        (app, id)
    }

    fn key(k: egui::Key) -> egui::Event {
        egui::Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }
    }

    /// One frame of the card and its confirmation, with these events.
    fn frame(ctx: &egui::Context, app: &mut ResinApp, events: Vec<egui::Event>) {
        let raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1280.0, 800.0))), events, ..Default::default() };
        let mut out = ctx.run_ui(raw, |ui| panel(app, ui.ctx()));
        out.textures_delta.clear();
    }

    fn shown(ctx: &egui::Context) -> bool {
        ctx.memory(|m| m.areas().visible_layer_ids().iter().any(|l| l.id == egui::Id::new(CARD_ID)))
    }

    #[test]
    fn support_edition_shows_the_card_and_cancels_an_open_tool() {
        let (mut app, id) = app_with_a_model();
        app.run("model.select", json!({ "ids": [id] }));
        let (depth, start) = (app.session.undo_depth(), app.session.scene.models()[0].placement);
        transform::open(&mut app, transform::Op::Move);
        app.session.begin_step();
        app.run("model.place", json!({ "models": [{ "id": id, "position": [40, 0, 0] }] }));
        app.session.end_step();
        assert_eq!(app.session.undo_depth(), depth + 1);

        app.set_mode(EditMode::Support);
        assert!(app.tool.is_none(), "the tool is put away");
        assert_eq!(app.session.scene.models()[0].placement, start, "as its Cancel does");
        assert_eq!(app.session.undo_depth(), depth, "and its steps go");
        let ctx = egui::Context::default();
        frame(&ctx, &mut app, vec![]);
        frame(&ctx, &mut app, vec![]);
        assert!(shown(&ctx));

        app.set_mode(EditMode::Object);
        frame(&ctx, &mut app, vec![]);
        frame(&ctx, &mut app, vec![]);
        assert!(!shown(&ctx));
    }

    #[test]
    fn the_card_has_no_footer_and_esc_leaves_it() {
        let ctx = egui::Context::default();
        let t = Tokens::for_dark(false);
        let height = |buttons| {
            let mut action = Action::None;
            let mut out = ctx.run_ui(egui::RawInput { events: vec![key(egui::Key::Escape)], ..Default::default() }, |ui| {
                let card = Card {
                    id: egui::Id::new(("card", format!("{buttons:?}"))),
                    title: "",
                    icon: "",
                    at: Pos2::ZERO,
                    bounds: None,
                    width: 200.0,
                    buttons,
                };
                action = card.show(ui.ctx(), &t, |ui| {
                    ui.label("x");
                });
            });
            out.textures_delta.clear();
            let rect = ctx.memory(|m| m.area_rect(egui::Id::new(("card", format!("{buttons:?}"))))).unwrap();
            (rect.height(), action)
        };
        let (permanent, action) = height(Buttons::None);
        let (tool, cancel) = height(Buttons::OkCancel { ok_enabled: true });
        assert_eq!((action, cancel), (Action::None, Action::Cancel), "Esc cancels a tool, not a permanent panel");
        assert!((tool - permanent - FORM_FOOTER_HEIGHT).abs() < 1.0, "{tool} {permanent}");

        let (mut app, _) = app_with_a_model();
        app.set_mode(EditMode::Support);
        let ctx = egui::Context::default();
        frame(&ctx, &mut app, vec![]);
        frame(&ctx, &mut app, vec![key(egui::Key::Escape)]);
        frame(&ctx, &mut app, vec![]);
        assert!(shown(&ctx) && app.mode == EditMode::Support);
    }

    #[test]
    fn switching_tabs_changes_no_setting() {
        let (mut app, _) = app_with_a_model();
        app.set_mode(EditMode::Support);
        let ctx = egui::Context::default();
        // A window's first frame only sizes it.
        frame(&ctx, &mut app, vec![]);
        frame(&ctx, &mut app, vec![]);
        let card = ctx.memory(|m| m.area_rect(egui::Id::new(CARD_ID))).unwrap();
        // The tab strip is the body's first row: under the header, inside the 12 pt margin.
        let width = (card.width() - 2.0 * SPACE_12) / TABS.len() as f32;
        let at = pos2(card.left() + SPACE_12 + width * 2.5, card.top() + FORM_HEADER_HEIGHT + SPACE_12 + CONTROL_HEIGHT / 2.0);
        let (settings, depth) = (*app.session.support_settings(), app.session.undo_depth());
        let button =
            |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
        frame(&ctx, &mut app, vec![egui::Event::PointerMoved(at)]);
        frame(&ctx, &mut app, vec![button(true)]);
        frame(&ctx, &mut app, vec![button(false)]);
        frame(&ctx, &mut app, vec![]);
        assert_eq!(app.settings_tab, BASE);
        assert_eq!((*app.session.support_settings(), app.session.undo_depth()), (settings, depth));
        assert_eq!(app.prefs.support_settings, settings);
    }

    #[test]
    fn a_field_edit_makes_no_undo_step_and_is_kept_in_the_preferences() {
        let (mut app, id) = app_with_a_model();
        app.run("support.add", json!({ "model": id, "point": [1, 1, 0] }));
        let (undo, redo) = (app.session.undo_depth(), app.session.redo_depth());
        let mut p = *app.session.support_settings().current();
        let before = p;
        p.tip.length = 2.9;
        p.body.shape = BodyShape::Prism;
        configure(&mut app, changes(&before, &p).unwrap());
        assert_eq!((app.session.undo_depth(), app.session.redo_depth()), (undo, redo));
        assert_eq!(app.prefs.support_settings.medium.tip.length, 2.9);
        assert_eq!(app.prefs.support_settings.medium.body.shape, BodyShape::Prism);
        assert_eq!(app.prefs.support_settings, *app.session.support_settings());
        // So does one run from elsewhere (a script).
        app.run("support.configure", json!({ "preset": "light", "base": { "height": 0.9 } }));
        assert_eq!(app.prefs.support_settings.light.base.height, 0.9);
        assert_eq!(changes(&p, &p), None);
    }

    #[test]
    fn restore_defaults_waits_for_its_confirmation_and_resets_the_active_preset_only() {
        let (mut app, _) = app_with_a_model();
        app.set_mode(EditMode::Support);
        app.run("support.configure", json!({ "tip": { "length": 2.9 } }));
        app.run("support.configure", json!({ "preset": "light", "tip": { "length": 2.8 } }));
        let edited = *app.session.support_settings();
        let ctx = egui::Context::default();

        // Restore defaults opens the question; Esc answers Cancel.
        app.confirm_reset = true;
        frame(&ctx, &mut app, vec![]);
        frame(&ctx, &mut app, vec![]);
        assert_eq!(*app.session.support_settings(), edited, "nothing until confirmed");
        assert!(app.confirm_reset);
        frame(&ctx, &mut app, vec![key(egui::Key::Escape)]);
        assert!(!app.confirm_reset);
        assert_eq!(*app.session.support_settings(), edited);

        // Asked again and confirmed (Enter, as the Restore button): the active preset only.
        app.confirm_reset = true;
        frame(&ctx, &mut app, vec![]);
        frame(&ctx, &mut app, vec![key(egui::Key::Enter)]);
        assert!(!app.confirm_reset);
        let s = *app.session.support_settings();
        assert_eq!(s.medium, PresetId::Medium.defaults());
        assert_eq!(s.light, edited.light);
        assert_eq!(app.prefs.support_settings, s);
    }

    #[test]
    fn every_card_word_is_in_the_catalog() {
        for id in TABS.into_iter().chain(PresetId::ALL.map(preset_label)) {
            assert!(i18n::has(id), "{id}");
        }
        for s in ContactShape::ALL {
            assert!(i18n::has(contact_name(s).1));
        }
        for s in BodyShape::ALL {
            assert!(i18n::has(body_name(s).1));
        }
        for e in RaftEdge::ALL {
            assert!(i18n::has(edge_name(e).1));
        }
    }
}
