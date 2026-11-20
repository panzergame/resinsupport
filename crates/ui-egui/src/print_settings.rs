//! The print settings card: the printer profile and how the resin is exposed and the plate moves,
//! on five tabs (Printer, with the layer height; Bottom Layers; Regular Layers; Slicing, with the
//! gamma's curve and the selected model's own slicing settings; Resin, for the estimates), and
//! Restore defaults. The
//! plate's travel shows each phase's value side by side, fast then slow. Every edit runs
//! `print.configure` at once; the configuration is not an undo step, and `ResinApp::run` keeps it
//! in the preferences. Shown for as long as the Slice edit mode lasts, and only then: nothing
//! else opens or closes it.

use egui::{Rect, Sense, Stroke, pos2, vec2};
use resin_engine::PrintConfig;
use resin_engine::slice::{ANTI_ALIASING, CtbFormat, GROUPS, MAX_NAME_CHARS, Motion, SlicingOverride, field};
use serde_json::{Value, json};

use crate::theme::Tokens;
use crate::theme::metrics::*;
use crate::ui_kit::{self, Action, Buttons, Card, Confirm};
use crate::{EditMode, ResinApp, i18n};

/// The card's tabs: the catalog ids of their names, in order.
pub const TABS: [&str; 5] = ["printing.tab.printer", "printing.tab.bottom", "printing.tab.regular", "printing.tab.slicing", "printing.tab.resin"];
const PRINTER: usize = 0;
const BOTTOM: usize = 1;
const REGULAR: usize = 2;
const SLICING: usize = 3;

/// The anti-aliasing choices' names: the levels, as [`ANTI_ALIASING`], then exact coverage.
const ANTI_ALIASING_NAMES: [(&str, &str); 5] = [
    ("1", "printing.anti_aliasing.off"),
    ("2", "printing.anti_aliasing.2"),
    ("4", "printing.anti_aliasing.4"),
    ("8", "printing.anti_aliasing.8"),
    (EXACT, "printing.anti_aliasing.exact"),
];

/// The Z anti-aliasing choices' names, as [`ANTI_ALIASING`].
const ANTI_ALIASING_Z_NAMES: [(&str, &str); 4] = [
    ("1", "printing.anti_aliasing.off"),
    ("2", "printing.anti_aliasing_z.2"),
    ("4", "printing.anti_aliasing_z.4"),
    ("8", "printing.anti_aliasing_z.8"),
];

/// The anti-aliasing choice that sets `slicing.exact`.
const EXACT: &str = "exact";

const CTB_FORMAT_NAMES: [(&str, &str); 2] = [("encrypted", "printing.printer.ctb_format.encrypted"), ("v2", "printing.printer.ctb_format.v2")];

pub(crate) const CARD_ID: &str = "print_settings";
const CONFIRM_ID: &str = "print_settings_reset";

/// Run `print.configure` with `params`: what every field of the card does.
pub(crate) fn configure(app: &mut ResinApp, params: Value) {
    app.run("print.configure", params);
}

/// The card while it is open, and Restore defaults' confirmation.
pub fn panel(app: &mut ResinApp, ctx: &egui::Context) {
    if app.mode != EditMode::Slice {
        app.confirm_print_reset = false;
        return;
    }
    let t = Tokens::for_dark(ctx.global_style().visuals.dark_mode);
    let at = crate::models_card::beside(app);
    let before = app.session.print_config().clone();
    let mut c = before.clone();
    let mut tab = app.print_tab.min(TABS.len() - 1);
    let mut reset = false;
    let mut overrides = false;
    let one_model = crate::object_slicing::target(app).is_some();
    let card = Card {
        id: egui::Id::new(CARD_ID),
        title: i18n::tr("printing.title"),
        icon: "settings",
        at,
        bounds: app.viewport_rect,
        width: FORM_TABBED_CARD_WIDTH,
        buttons: Buttons::None,
    };
    card.show(ctx, &t, |ui| {
        let tabs = TABS.map(i18n::tr);
        ui_kit::segmented(ui, &t, &mut tab, &tabs);
        match tab {
            PRINTER => printer_rows(ui, &t, &mut c),
            BOTTOM => {
                rows(ui, &t, &mut c, BOTTOM_ROWS, &mut Marks::default());
                pairs(ui, &t, &mut c, BOTTOM_MOTION);
            }
            REGULAR => {
                rows(ui, &t, &mut c, REGULAR_ROWS, &mut Marks::default());
                pairs(ui, &t, &mut c, REGULAR_MOTION);
            }
            SLICING => {
                slicing_rows(ui, &t, &mut c, &mut Marks::default());
                ui.add_enabled_ui(one_model, |ui| {
                    let help = if one_model { i18n::tr("printing.object.open.help") } else { i18n::tr("reason.select_one_model") };
                    let open = ui_kit::action_row(ui, &t, i18n::tr("printing.object.open"));
                    if open.on_hover_text(help).on_disabled_hover_text(help).clicked() {
                        overrides = true;
                    }
                });
            }
            _ => rows(ui, &t, &mut c, RESIN_ROWS, &mut Marks::default()),
        }
        if ui_kit::action_row(ui, &t, i18n::tr("ui.restore_defaults")).clicked() {
            reset = true;
        }
        ui_kit::hint(ui, &t, i18n::tr("printing.hint"));
    });
    app.print_tab = tab;
    if overrides {
        app.object_slicing = true;
    }
    if let Some(edit) = changes(&before, &c) {
        configure(app, edit);
    }
    if reset {
        app.confirm_print_reset = true;
    }
    if app.confirm_print_reset {
        let confirm = Confirm {
            id: egui::Id::new(CONFIRM_ID),
            icon: "undo",
            title: i18n::tr("printing.reset_title"),
            message: i18n::tr("printing.reset_confirm"),
            ok: "printing.reset_ok",
        };
        match confirm.show(ctx, &t) {
            Action::Ok => {
                app.confirm_print_reset = false;
                app.run("print.reset", json!({}));
            }
            Action::Cancel => app.confirm_print_reset = false,
            Action::Reset | Action::Fold | Action::None => {}
        }
    }
}

/// `print.configure`'s parameters for what differs from `before` to `after`.
fn changes(before: &PrintConfig, after: &PrintConfig) -> Option<Value> {
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

/// A number field: which setting, how it shows and drags.
struct Num {
    label: &'static str,
    help: Option<&'static str>,
    /// Whether the field can be edited, from the other settings; always when `None`.
    when: Option<fn(&PrintConfig) -> bool>,
    group: &'static str,
    key: &'static str,
    suffix: &'static str,
    decimals: usize,
    speed: f64,
}

const fn num(label: &'static str, group: &'static str, key: &'static str, suffix: &'static str, decimals: usize, speed: f64) -> Num {
    Num { label, help: None, when: None, group, key, suffix, decimals, speed }
}

impl Num {
    const fn help(self, help: &'static str) -> Num {
        Num { help: Some(help), ..self }
    }

    const fn when(self, when: fn(&PrintConfig) -> bool) -> Num {
        Num { when: Some(when), ..self }
    }

    /// A row with the setting's drag field, in its range.
    fn row(&self, ui: &mut egui::Ui, t: &Tokens, c: &mut PrintConfig, marks: &mut Marks) {
        let Some(f) = field(self.group, self.key) else { return };
        let Some(mut v) = f.get(&c.printer, &c.settings) else { return };
        let enabled = self.when.is_none_or(|when| when(c));
        let changed = marks.row(ui, t, &[self.key], |ui| {
            ui_kit::row(ui, t, i18n::tr(self.label), |ui| {
                let decimals = if f.integer { 0 } else { self.decimals };
                let response =
                    ui.add_enabled_ui(enabled, |ui| ui_kit::number_field(ui, &mut v, f.min..=f.max, self.speed, self.suffix, decimals)).inner;
                let changed = response.changed();
                if let Some(help) = self.help {
                    response.on_hover_text(i18n::tr(help)).on_disabled_hover_text(i18n::tr(help));
                }
                changed
            })
        });
        if changed {
            f.set(&mut c.printer, &mut c.settings, if f.integer { v.round() } else { v });
        }
    }
}

const PRINTER_ROWS: &[Num] = &[
    num("printing.printer.resolution_x", "printer", "resolution_x", " px", 0, 1.0),
    num("printing.printer.resolution_y", "printer", "resolution_y", " px", 0, 1.0),
    num("printing.printer.display_x", "printer", "display_x", " mm", 3, 0.1),
    num("printing.printer.display_y", "printer", "display_y", " mm", 3, 0.1),
    num("printing.printer.build_height", "printer", "build_height", " mm", 2, 0.5),
    num("printing.layers.height", "layers", "height", " mm", 3, 0.001).help("printing.all_layers.help"),
];

/// The bottom layers' values; a row's name is its regular twin's, the tab says which.
const BOTTOM_ROWS: &[Num] = &[
    num("printing.layers.bottom_count", "layers", "bottom_count", "", 0, 0.1).help("printing.layers.bottom_count.help"),
    num("printing.layers.transition_count", "layers", "transition_count", "", 0, 0.1).help("printing.layers.transition_count.help"),
    num("printing.exposure.normal", "exposure", "bottom", " s", 2, 0.1),
    num("printing.exposure.light_off", "exposure", "bottom_light_off", " s", 2, 0.05).help("printing.exposure.light_off.help"),
    num("printing.exposure.power", "exposure", "bottom_power", "", 0, 1.0).help("printing.exposure.power.help"),
];

const REGULAR_ROWS: &[Num] = &[
    num("printing.exposure.normal", "exposure", "normal", " s", 2, 0.05),
    num("printing.exposure.light_off", "exposure", "light_off", " s", 2, 0.05).help("printing.exposure.light_off.help"),
    num("printing.exposure.power", "exposure", "power", "", 0, 1.0).help("printing.exposure.power.help"),
];

/// One phase's value in a [`Pair`]: a `motion` setting, or one worked out from the others and
/// shown read-only.
#[derive(Clone, Copy)]
enum Phase {
    Setting(&'static str),
    Derived(fn(&Motion) -> f64),
}

/// A travel row: its fast phase's value and its slow phase's, under the Fast and Slow heading.
struct Pair {
    label: &'static str,
    help: Option<&'static str>,
    fast: Phase,
    slow: Phase,
    /// Below the setting's own maximum, from the others.
    slow_max: Option<fn(&Motion) -> f64>,
    suffix: &'static str,
    decimals: usize,
    speed: f64,
}

const fn pair(label: &'static str, fast: Phase, slow: Phase, suffix: &'static str, decimals: usize, speed: f64) -> Pair {
    Pair { label, help: None, fast, slow, slow_max: None, suffix, decimals, speed }
}

impl Pair {
    const fn help(self, help: &'static str) -> Pair {
        Pair { help: Some(help), ..self }
    }

    const fn slow_max(self, max: fn(&Motion) -> f64) -> Pair {
        Pair { slow_max: Some(max), ..self }
    }

    fn row(&self, ui: &mut egui::Ui, t: &Tokens, c: &mut PrintConfig) {
        ui_kit::row(ui, t, i18n::tr(self.label), |ui| {
            let width = phase_width(ui);
            self.phase(ui, width, c, self.fast, None);
            self.phase(ui, width, c, self.slow, self.slow_max);
        });
    }

    fn phase(&self, ui: &mut egui::Ui, width: f32, c: &mut PrintConfig, phase: Phase, max: Option<fn(&Motion) -> f64>) {
        let help = |r: egui::Response| match self.help {
            Some(help) => r.on_hover_text(i18n::tr(help)).on_disabled_hover_text(i18n::tr(help)),
            None => r,
        };
        match phase {
            Phase::Derived(get) => {
                let mut v = get(&c.settings.motion);
                let only = v..=v;
                help(cell(ui, width, |ui| {
                    ui.add_enabled_ui(false, |ui| ui_kit::number_field(ui, &mut v, only, 0.0, self.suffix, self.decimals)).inner
                }));
            }
            Phase::Setting(key) => {
                let Some(f) = field("motion", key) else { return };
                let Some(mut v) = f.get(&c.printer, &c.settings) else { return };
                let max = max.map_or(f.max, |m| m(&c.settings.motion).clamp(f.min, f.max));
                if help(cell(ui, width, |ui| ui_kit::number_field(ui, &mut v, f.min..=max, self.speed, self.suffix, self.decimals))).changed() {
                    f.set(&mut c.printer, &mut c.settings, v);
                }
            }
        }
    }
}

/// Each phase's share of a row's control column.
fn phase_width(ui: &egui::Ui) -> f32 {
    ((ui.available_width() - ui.spacing().item_spacing.x) / 2.0).max(0.0)
}

/// `add`'s control in a phase's share of the row.
fn cell(ui: &mut egui::Ui, width: f32, add: impl FnOnce(&mut egui::Ui) -> egui::Response) -> egui::Response {
    ui.allocate_ui_with_layout(vec2(width, CONTROL_HEIGHT), egui::Layout::left_to_right(egui::Align::Center), add).inner
}

fn regular_retract_fast(m: &Motion) -> f64 {
    m.regular().retract_distance_fast()
}

fn regular_lift(m: &Motion) -> f64 {
    m.regular().lift_distance()
}

fn bottom_retract_fast(m: &Motion) -> f64 {
    m.bottom().retract_distance_fast()
}

fn bottom_lift(m: &Motion) -> f64 {
    m.bottom().lift_distance()
}

const BOTTOM_MOTION: &[Pair] = &[
    pair("printing.motion.lift_distance", Phase::Setting("bottom_lift_distance_fast"), Phase::Setting("bottom_lift_distance_slow"), " mm", 2, 0.1)
        .help("printing.motion.lift_distance.help"),
    pair("printing.motion.lift_speed", Phase::Setting("bottom_lift_speed_fast"), Phase::Setting("bottom_lift_speed_slow"), " mm/min", 0, 5.0),
    pair("printing.motion.retract_distance", Phase::Derived(bottom_retract_fast), Phase::Setting("bottom_retract_distance_slow"), " mm", 2, 0.1)
        .help("printing.motion.retract_distance.help")
        .slow_max(bottom_lift),
    pair(
        "printing.motion.retract_speed",
        Phase::Setting("bottom_retract_speed_fast"),
        Phase::Setting("bottom_retract_speed_slow"),
        " mm/min",
        0,
        5.0,
    ),
];

const REGULAR_MOTION: &[Pair] = &[
    pair("printing.motion.lift_distance", Phase::Setting("lift_distance_fast"), Phase::Setting("lift_distance_slow"), " mm", 2, 0.1)
        .help("printing.motion.lift_distance.help"),
    pair("printing.motion.lift_speed", Phase::Setting("lift_speed_fast"), Phase::Setting("lift_speed_slow"), " mm/min", 0, 5.0),
    pair("printing.motion.retract_distance", Phase::Derived(regular_retract_fast), Phase::Setting("retract_distance_slow"), " mm", 2, 0.1)
        .help("printing.motion.retract_distance.help")
        .slow_max(regular_lift),
    pair("printing.motion.retract_speed", Phase::Setting("retract_speed_fast"), Phase::Setting("retract_speed_slow"), " mm/min", 0, 5.0),
];

/// The phases' heading, in the order a [`Pair`] shows them.
const PHASES: [&str; 2] = ["printing.motion.fast", "printing.motion.slow"];

fn pairs(ui: &mut egui::Ui, t: &Tokens, c: &mut PrintConfig, pairs: &[Pair]) {
    ui_kit::row(ui, t, "", |ui| {
        let width = phase_width(ui);
        for id in PHASES {
            let text = egui::RichText::new(i18n::tr(id)).size(FONT_CAPTION).color(t.form_dim);
            ui.add_sized([width, CONTROL_HEIGHT], egui::Label::new(text));
        }
    });
    for p in pairs {
        p.row(ui, t, c);
    }
}

/// After the anti-aliasing choices, in the order the slicer applies them.
const SLICING_ROWS: &[Num] = &[
    num("printing.blur_passes", "slicing", "blur_passes", "", 0, 0.05).help("printing.blur_passes.help"),
    num("printing.blur_radius", "slicing", "blur_radius", " px", 0, 0.05).help("printing.blur_radius.help").when(blurs),
    num("printing.dither_floor", "slicing", "dither_floor", " %", 0, 0.2).help("printing.dither_floor.help").when(dithers),
    num("printing.dither_max_angle", "slicing", "dither_max_angle", "°", 1, 0.2).help("printing.dither_max_angle.help").when(dithers),
    num("printing.gamma", "slicing", "gamma", "", 2, 0.01).help("printing.gamma.help"),
];

/// The resin, for the volume and cost estimates: no slicing setting, so a model never has its own.
const RESIN_ROWS: &[Num] = &[
    num("printing.estimates.resin_density", "estimates", "resin_density", " g/ml", 2, 0.01),
    num("printing.estimates.bottle_volume", "estimates", "bottle_volume", " ml", 0, 10.0),
    num("printing.estimates.bottle_cost", "estimates", "bottle_cost", "", 2, 0.5),
];

fn blurs(c: &PrintConfig) -> bool {
    c.settings.slicing.blur_passes > 0
}

fn dithers(c: &PrintConfig) -> bool {
    c.settings.slicing.dither_z || c.settings.slicing.dither_xy
}

fn rows(ui: &mut egui::Ui, t: &Tokens, c: &mut PrintConfig, rows: &[Num], marks: &mut Marks) {
    for r in rows {
        if (r.group, r.key) == ("slicing", "dither_floor") {
            let s = &mut c.settings.slicing;
            marks.row(ui, t, &["dither_z"], |ui| {
                ui_kit::check_row(ui, t, i18n::tr("printing.dither_z"), i18n::tr("printing.dither_z.help"), &mut s.dither_z);
            });
            marks.row(ui, t, &["dither_xy"], |ui| {
                ui_kit::check_row(ui, t, i18n::tr("printing.dither_xy"), i18n::tr("printing.dither_xy.help"), &mut s.dither_xy);
            });
        }
        r.row(ui, t, c, marks);
        if (r.group, r.key) == ("slicing", "gamma") {
            // Under the gamma row, in step with its control column.
            marks.row(ui, t, &[], |ui| gamma_curve(ui, t, &c.settings.slicing.gamma_table()));
        }
    }
}

/// The plot's height, px.
const CURVE_HEIGHT: f32 = 88.0;

/// Under the gamma row, in its control column: each grey in (along) and what it becomes (up),
/// over the diagonal that leaves them as they are.
fn gamma_curve(ui: &mut egui::Ui, t: &Tokens, table: &[u8; 256]) {
    ui_kit::sized_row(ui, t, "", CURVE_HEIGHT, |ui| {
        let (plot, response) = ui.allocate_exact_size(vec2(ui.available_width(), CURVE_HEIGHT), Sense::hover());
        plot_gamma(ui, t, plot, table);
        response.on_hover_text(i18n::tr("printing.gamma.curve.help"));
    });
}

fn plot_gamma(ui: &egui::Ui, t: &Tokens, plot: Rect, table: &[u8; 256]) {
    let painter = ui.painter_at(plot);
    painter.rect(plot, RADIUS_SM, t.field, Stroke::new(1.0, t.control_border), egui::StrokeKind::Inside);
    let inner = plot.shrink(SPACE_3);
    let at = |i: usize, g: u8| pos2(inner.left() + inner.width() * i as f32 / 255.0, inner.bottom() - inner.height() * f32::from(g) / 255.0);
    painter.line_segment([at(0, 0), at(255, 255)], Stroke::new(1.0, t.form_dim));
    let points = table.iter().enumerate().map(|(i, &g)| at(i, g)).collect();
    painter.add(egui::Shape::line(points, Stroke::new(1.5, t.accent)));
}

fn printer_rows(ui: &mut egui::Ui, t: &Tokens, c: &mut PrintConfig) {
    ui_kit::row(ui, t, i18n::tr("printing.printer.name"), |ui| {
        ui_kit::text_field(ui, t, &mut c.printer.name, "", MAX_NAME_CHARS).on_hover_text(i18n::tr("printing.printer.name.help"));
    });
    rows(ui, t, c, PRINTER_ROWS, &mut Marks::default());
    let p = &mut c.printer;
    ui_kit::check_row(ui, t, i18n::tr("printing.printer.mirror_x"), i18n::tr("printing.printer.mirror.help"), &mut p.mirror_x);
    ui_kit::check_row(ui, t, i18n::tr("printing.printer.mirror_y"), i18n::tr("printing.printer.mirror.help"), &mut p.mirror_y);
    let mut format = p.ctb_format.id().to_owned();
    let help = i18n::tr("printing.printer.ctb_format.help");
    ui_kit::choice_row_help(ui, t, i18n::tr("printing.printer.ctb_format"), Some(help), "ctb_format", &mut format, &CTB_FORMAT_NAMES);
    if let Some(f) = CtbFormat::from_id(&format) {
        p.ctb_format = f;
    }
    let (x, y) = p.pixel_pitch();
    let microns = |mm: f64| format!("{:.1}", mm * 1000.0);
    ui_kit::hint(ui, t, &i18n::format("printing.printer.pixel", &[("x", &microns(x)), ("y", &microns(y))]));
}

/// The Slicing tab's settings, the plate's or (with `marks` on a model's own settings) a model's.
pub(crate) fn slicing_rows(ui: &mut egui::Ui, t: &Tokens, c: &mut PrintConfig, marks: &mut Marks) {
    let s = &mut c.settings.slicing;
    let mut aa = if s.exact { EXACT.to_owned() } else { s.anti_aliasing.to_string() };
    marks.row(ui, t, &["anti_aliasing", "exact"], |ui| {
        ui_kit::choice_row(ui, t, i18n::tr("printing.anti_aliasing"), "anti_aliasing", &mut aa, &ANTI_ALIASING_NAMES);
    });
    if aa == EXACT {
        s.exact = true;
    } else if let Some(level) = ANTI_ALIASING.iter().find(|a| a.to_string() == aa) {
        (s.exact, s.anti_aliasing) = (false, *level);
    }
    let mut z = s.anti_aliasing_z.to_string();
    let help = i18n::tr("printing.anti_aliasing_z.help");
    marks.row(ui, t, &["anti_aliasing_z"], |ui| {
        ui_kit::choice_row_help(ui, t, i18n::tr("printing.anti_aliasing_z"), Some(help), "anti_aliasing_z", &mut z, &ANTI_ALIASING_Z_NAMES);
    });
    if let Some(level) = ANTI_ALIASING.iter().find(|a| a.to_string() == z) {
        s.anti_aliasing_z = *level;
    }
    rows(ui, t, c, SLICING_ROWS, marks);
}

/// What the Slicing tab's rows show of a model's own settings: with `own`, each row leaves room
/// on its right for a button back to the plate's value, and a row the model sets for itself is
/// highlighted with that button shown. `cleared` collects the keys whose button was clicked.
#[derive(Default)]
pub(crate) struct Marks<'a> {
    pub(crate) own: Option<&'a SlicingOverride>,
    pub(crate) cleared: Vec<&'static str>,
}

impl Marks<'_> {
    /// `add`'s row, for the settings `keys`.
    fn row<R>(&mut self, ui: &mut egui::Ui, t: &Tokens, keys: &[&'static str], add: impl FnOnce(&mut egui::Ui) -> R) -> R {
        let Some(own) = self.own else { return add(ui) };
        let set = keys.iter().any(|k| own.has(k));
        // The highlight goes under the row, which is only measured once it is laid out.
        let under = ui.painter().add(egui::Shape::Noop);
        let laid = ui.horizontal(|ui| {
            let width = (ui.available_width() - CONTROL_HEIGHT - SPACE_4).max(0.0);
            let inner = ui.allocate_ui_with_layout(vec2(width, CONTROL_HEIGHT), egui::Layout::top_down(egui::Align::Min), |ui| {
                ui.set_width(width);
                add(ui)
            });
            let back = if set {
                ui_kit::icon_button(ui, t, "undo", i18n::tr("printing.object.use_plate"), true).clicked()
            } else {
                ui.allocate_exact_size(vec2(CONTROL_HEIGHT, CONTROL_HEIGHT), Sense::hover());
                false
            };
            (inner.inner, back)
        });
        let (inner, back) = laid.inner;
        if set {
            let rect = laid.response.rect.expand2(vec2(SPACE_4, SPACE_2));
            let bar = Rect::from_min_size(rect.min, vec2(SPACE_3, rect.height()));
            ui.painter().set(
                under,
                egui::Shape::Vec(vec![egui::Shape::rect_filled(rect, RADIUS_SM, t.selected_bg), egui::Shape::rect_filled(bar, RADIUS_SM, t.accent)]),
            );
        }
        if back {
            self.cleared.extend_from_slice(keys);
        }
        inner
    }
}

#[cfg(test)]
mod tests {
    use egui::{Pos2, Rect};
    use resin_engine::slice::{PrintSettings, PrinterProfile};

    use super::*;
    use crate::prefs::Prefs;

    fn key(k: egui::Key) -> egui::Event {
        egui::Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }
    }

    fn frame(ctx: &egui::Context, app: &mut ResinApp, events: Vec<egui::Event>) {
        let raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1280.0, 800.0))), events, ..Default::default() };
        let mut out = ctx.run_ui(raw, |ui| panel(app, ui.ctx()));
        out.textures_delta.clear();
    }

    fn shown(ctx: &egui::Context) -> bool {
        ctx.memory(|m| m.areas().visible_layer_ids().iter().any(|l| l.id == egui::Id::new(CARD_ID)))
    }

    #[test]
    fn the_card_shows_in_slice_mode_on_every_tab_without_changing_a_setting() {
        let mut app = ResinApp::build(Prefs::default(), &[]);
        let ctx = egui::Context::default();
        frame(&ctx, &mut app, vec![]);
        assert!(!shown(&ctx));
        // Set directly: entering the mode through `set_mode` would start a slice.
        app.mode = EditMode::Slice;
        let (config, depth) = (app.session.print_config().clone(), app.session.undo_depth());
        for tab in 0..TABS.len() {
            app.print_tab = tab;
            frame(&ctx, &mut app, vec![]);
            frame(&ctx, &mut app, vec![]);
            assert!(shown(&ctx), "tab {tab}");
        }
        // Esc leaves it alone: it lasts as long as the mode.
        frame(&ctx, &mut app, vec![key(egui::Key::Escape)]);
        assert!(shown(&ctx));
        assert_eq!((app.session.print_config(), app.session.undo_depth()), (&config, depth));
        for mode in [EditMode::Object, EditMode::Support] {
            app.mode = mode;
            frame(&ctx, &mut app, vec![]);
            frame(&ctx, &mut app, vec![]);
            assert!(!shown(&ctx), "{mode:?}");
        }
    }

    #[test]
    fn a_field_edit_makes_no_undo_step_and_is_kept_in_the_preferences() {
        let mut app = ResinApp::build(Prefs::default(), &[]);
        let (undo, redo) = (app.session.undo_depth(), app.session.redo_depth());
        let before = app.session.print_config().clone();
        let mut c = before.clone();
        c.settings.exposure.normal = 2.2;
        c.printer.mirror_x = true;
        let edit = changes(&before, &c).unwrap();
        assert_eq!(edit, json!({ "printer": { "mirror_x": true }, "exposure": { "normal": 2.2 } }));
        configure(&mut app, edit);
        assert_eq!((app.session.undo_depth(), app.session.redo_depth()), (undo, redo));
        assert_eq!(*app.session.print_config(), c);
        assert_eq!(app.prefs.print_config, c);
        // So does one run from elsewhere (a script).
        app.run("print.configure", json!({ "layers": { "height": 0.03 } }));
        assert_eq!(app.prefs.print_config.settings.layers.height, 0.03);
        assert_eq!(changes(&c, &c), None);
    }

    #[test]
    fn restore_defaults_waits_for_its_confirmation_and_keeps_the_printer() {
        let mut app = ResinApp::build(Prefs::default(), &[]);
        app.mode = EditMode::Slice;
        app.run("print.configure", json!({ "printer": { "name": "Mine" }, "exposure": { "bottom": 25 } }));
        let edited = app.session.print_config().clone();
        let ctx = egui::Context::default();

        app.confirm_print_reset = true;
        frame(&ctx, &mut app, vec![]);
        frame(&ctx, &mut app, vec![]);
        assert_eq!(*app.session.print_config(), edited, "nothing until confirmed");
        frame(&ctx, &mut app, vec![key(egui::Key::Escape)]);
        assert!(!app.confirm_print_reset);
        assert_eq!(*app.session.print_config(), edited);

        app.confirm_print_reset = true;
        frame(&ctx, &mut app, vec![]);
        frame(&ctx, &mut app, vec![key(egui::Key::Enter)]);
        assert!(!app.confirm_print_reset);
        let c = app.session.print_config();
        assert_eq!((&c.printer, c.settings), (&PrinterProfile { name: "Mine".into(), ..PrinterProfile::default() }, PrintSettings::default()));
        assert_eq!(app.prefs.print_config, *c);
    }

    #[test]
    fn every_number_setting_is_on_exactly_one_tab() {
        let all = [PRINTER_ROWS, BOTTOM_ROWS, REGULAR_ROWS, SLICING_ROWS, RESIN_ROWS];
        let mut shown: Vec<(&str, &str)> = all.iter().flat_map(|rows| rows.iter().map(|r| (r.group, r.key))).collect();
        for p in [BOTTOM_MOTION, REGULAR_MOTION].into_iter().flatten() {
            for phase in [p.fast, p.slow] {
                if let Phase::Setting(key) = phase {
                    shown.push(("motion", key));
                }
            }
        }
        // The anti-aliasing choices, before the Slicing tab's numbers.
        shown.extend([("slicing", "anti_aliasing"), ("slicing", "anti_aliasing_z")]);
        shown.sort_unstable();
        let mut every: Vec<(&str, &str)> = resin_engine::slice::FIELDS.iter().map(|f| (f.group, f.key)).collect();
        every.sort_unstable();
        assert_eq!(shown, every);
    }

    #[test]
    fn each_tab_moves_its_own_layers_and_the_slow_retract_stays_within_the_lift() {
        let m = PrintSettings::default().motion;
        for (pairs, bottom, lift) in [(BOTTOM_MOTION, true, m.bottom().lift_distance()), (REGULAR_MOTION, false, m.regular().lift_distance())] {
            for p in pairs {
                for phase in [p.fast, p.slow] {
                    if let Phase::Setting(key) = phase {
                        assert_eq!(key.starts_with("bottom_"), bottom, "{key}");
                    }
                }
            }
            let retract = pairs.iter().find(|p| p.label == "printing.motion.retract_distance").unwrap();
            assert!(matches!(retract.fast, Phase::Derived(_)));
            assert_eq!(retract.slow_max.map(|max| max(&m)), Some(lift));
        }
    }

    /// A model's own settings are the Slicing tab's, every one of them, and nothing else is.
    #[test]
    fn the_slicing_tab_shows_every_setting_a_model_may_have_of_its_own() {
        let mut keys: Vec<&str> =
            SLICING_ROWS.iter().map(|r| r.key).chain(["anti_aliasing", "exact", "anti_aliasing_z", "dither_z", "dither_xy"]).collect();
        assert!(SLICING_ROWS.iter().all(|r| r.group == "slicing"));
        keys.sort_unstable();
        let mut own = resin_engine::slice::SLICING_KEYS.to_vec();
        own.sort_unstable();
        assert_eq!(keys, own);
    }

    #[test]
    fn the_blur_radius_waits_for_a_pass_and_the_layer_height_is_on_the_printer_tab() {
        let radius = SLICING_ROWS.iter().find(|r| r.key == "blur_radius").and_then(|r| r.when).unwrap();
        let mut c = PrintConfig::default();
        assert_eq!(c.settings.slicing.blur_passes, 0);
        assert!(!radius(&c));
        c.settings.slicing.blur_passes = 1;
        assert!(radius(&c));
        assert!(PRINTER_ROWS.iter().any(|r| (r.group, r.key) == ("layers", "height")));
    }

    #[test]
    fn the_dither_floor_waits_for_a_dither() {
        let floor = SLICING_ROWS.iter().find(|r| r.key == "dither_floor").and_then(|r| r.when).unwrap();
        let angle = SLICING_ROWS.iter().find(|r| r.key == "dither_max_angle").and_then(|r| r.when).unwrap();
        let mut c = PrintConfig::default();
        assert!(!floor(&c) && !angle(&c));
        c.settings.slicing.dither_xy = true;
        assert!(floor(&c) && angle(&c));
        c.settings.slicing = PrintConfig::default().settings.slicing;
        c.settings.slicing.dither_z = true;
        assert!(floor(&c) && angle(&c));
    }

    #[test]
    fn every_card_word_is_in_the_catalog() {
        for id in TABS
            .into_iter()
            .chain(ANTI_ALIASING_NAMES.map(|(_, id)| id))
            .chain(CTB_FORMAT_NAMES.map(|(_, id)| id))
            .chain(ANTI_ALIASING_Z_NAMES.map(|(_, id)| id))
            .chain(PHASES)
        {
            assert!(i18n::has(id), "{id}");
        }
        for r in [PRINTER_ROWS, BOTTOM_ROWS, REGULAR_ROWS, SLICING_ROWS, RESIN_ROWS].into_iter().flatten() {
            assert!(i18n::has(r.label) && r.help.is_none_or(i18n::has), "{}", r.label);
        }
        for p in [BOTTOM_MOTION, REGULAR_MOTION].into_iter().flatten() {
            assert!(i18n::has(p.label) && p.help.is_none_or(i18n::has), "{}", p.label);
        }
        let levels: Vec<u8> = ANTI_ALIASING_NAMES.iter().filter_map(|(v, _)| v.parse().ok()).collect();
        assert_eq!(levels, ANTI_ALIASING);
        assert_eq!(ANTI_ALIASING_NAMES.last().map(|(v, _)| *v), Some(EXACT));
        assert_eq!(ANTI_ALIASING_Z_NAMES.map(|(v, _)| v.parse::<u8>().unwrap_or(0)), ANTI_ALIASING);
        assert!(i18n::has("printing.anti_aliasing_z") && i18n::has("printing.anti_aliasing_z.help"));
        assert!(i18n::has("printing.gamma.curve.help"));
        for id in ["printing.dither_z", "printing.dither_z.help", "printing.dither_xy", "printing.dither_xy.help"] {
            assert!(i18n::has(id), "{id}");
        }
    }
}
