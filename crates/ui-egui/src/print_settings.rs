//! The print settings card: the printer profile and how the resin is exposed and the plate moves,
//! on four tabs (Printer, Bottom Layers, Regular Layers, Slicing), and Restore defaults. Every edit runs
//! `print.configure` at once; the configuration is not an undo step, and `ResinApp::run` keeps it
//! in the preferences. Opened from the ribbon until the Slice edit mode holds it; the header's
//! close button or the ribbon button closes it, Esc doesn't.

use egui::{DragValue, pos2, vec2};
use resin_engine::PrintConfig;
use resin_engine::slice::{ANTI_ALIASING, GROUPS, MAX_NAME_CHARS, field};
use serde_json::{Value, json};

use crate::theme::Tokens;
use crate::theme::metrics::*;
use crate::ui_kit::{self, Action, Buttons, Card, Confirm};
use crate::{ResinApp, i18n};

/// The card's tabs: the catalog ids of their names, in order.
pub const TABS: [&str; 4] = ["printing.tab.printer", "printing.tab.bottom", "printing.tab.regular", "printing.tab.slicing"];
const PRINTER: usize = 0;
const BOTTOM: usize = 1;
const REGULAR: usize = 2;

/// The anti-aliasing levels' names, as [`ANTI_ALIASING`].
const ANTI_ALIASING_NAMES: [(&str, &str); 4] =
    [("1", "printing.anti_aliasing.off"), ("2", "printing.anti_aliasing.2"), ("4", "printing.anti_aliasing.4"), ("8", "printing.anti_aliasing.8")];

pub(crate) const CARD_ID: &str = "print_settings";
const CONFIRM_ID: &str = "print_settings_reset";

/// Run `print.configure` with `params`: what every field of the card does.
pub(crate) fn configure(app: &mut ResinApp, params: Value) {
    app.run("print.configure", params);
}

/// The card while it is open, and Restore defaults' confirmation.
pub fn panel(app: &mut ResinApp, ctx: &egui::Context) {
    if !app.print_open {
        app.confirm_print_reset = false;
        return;
    }
    let t = Tokens::for_dark(ctx.global_style().visuals.dark_mode);
    let at = app.viewport_rect.map_or(pos2(700.0, 140.0), |r| r.right_top() + vec2(-SPACE_12 - FORM_TABBED_CARD_WIDTH, SPACE_12));
    let before = app.session.print_config().clone();
    let mut c = before.clone();
    let mut tab = app.print_tab.min(TABS.len() - 1);
    let mut reset = false;
    let card = Card {
        id: egui::Id::new(CARD_ID),
        title: i18n::tr("printing.title"),
        icon: "settings",
        at,
        bounds: app.viewport_rect,
        width: FORM_TABBED_CARD_WIDTH,
        buttons: Buttons::CloseOnly,
    };
    let action = card.show(ctx, &t, |ui| {
        let tabs = TABS.map(i18n::tr);
        ui_kit::segmented(ui, &t, &mut tab, &tabs);
        match tab {
            PRINTER => printer_rows(ui, &t, &mut c),
            BOTTOM => rows(ui, &t, &mut c, BOTTOM_ROWS),
            REGULAR => rows(ui, &t, &mut c, REGULAR_ROWS),
            _ => slicing_rows(ui, &t, &mut c),
        }
        if ui_kit::button(ui, i18n::tr("ui.restore_defaults")).clicked() {
            reset = true;
        }
        ui_kit::hint(ui, &t, i18n::tr("printing.hint"));
    });
    app.print_tab = tab;
    if action == Action::Cancel {
        app.print_open = false;
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
            Action::Reset | Action::None => {}
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
    group: &'static str,
    key: &'static str,
    suffix: &'static str,
    decimals: usize,
    speed: f64,
}

const fn num(label: &'static str, group: &'static str, key: &'static str, suffix: &'static str, decimals: usize, speed: f64) -> Num {
    Num { label, help: None, group, key, suffix, decimals, speed }
}

impl Num {
    const fn help(self, help: &'static str) -> Num {
        Num { help: Some(help), ..self }
    }

    /// A row with the setting's drag field, in its range.
    fn row(&self, ui: &mut egui::Ui, t: &Tokens, c: &mut PrintConfig) {
        let Some(f) = field(self.group, self.key) else { return };
        let Some(mut v) = f.get(&c.printer, &c.settings) else { return };
        let changed = ui_kit::wide_row(ui, t, i18n::tr(self.label), |ui| {
            let mut drag = DragValue::new(&mut v).range(f.min..=f.max).speed(self.speed).suffix(self.suffix).max_decimals(self.decimals);
            if f.integer {
                drag = drag.fixed_decimals(0);
            }
            let response = ui.add_sized([ui.available_width(), CONTROL_HEIGHT], drag);
            let changed = response.changed();
            if let Some(help) = self.help {
                response.on_hover_text(i18n::tr(help));
            }
            changed
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
];

/// The bottom layers' values; a row's name is its regular twin's, the tab says which.
const BOTTOM_ROWS: &[Num] = &[
    num("printing.layers.bottom_count", "layers", "bottom_count", "", 0, 0.1).help("printing.layers.bottom_count.help"),
    num("printing.layers.transition_count", "layers", "transition_count", "", 0, 0.1).help("printing.layers.transition_count.help"),
    num("printing.exposure.normal", "exposure", "bottom", " s", 2, 0.1),
    num("printing.exposure.light_off", "exposure", "bottom_light_off", " s", 2, 0.05).help("printing.exposure.light_off.help"),
    num("printing.exposure.power", "exposure", "bottom_power", "", 0, 1.0).help("printing.exposure.power.help"),
    num("printing.motion.lift_height", "motion", "bottom_lift_height", " mm", 2, 0.1),
    num("printing.motion.lift_speed", "motion", "bottom_lift_speed", " mm/min", 0, 5.0),
];

/// Layer height and retract speed are one value for every layer: they sit here, and say so.
const REGULAR_ROWS: &[Num] = &[
    num("printing.layers.height", "layers", "height", " mm", 3, 0.001).help("printing.all_layers.help"),
    num("printing.exposure.normal", "exposure", "normal", " s", 2, 0.05),
    num("printing.exposure.light_off", "exposure", "light_off", " s", 2, 0.05).help("printing.exposure.light_off.help"),
    num("printing.exposure.power", "exposure", "power", "", 0, 1.0).help("printing.exposure.power.help"),
    num("printing.motion.lift_height", "motion", "lift_height", " mm", 2, 0.1),
    num("printing.motion.lift_speed", "motion", "lift_speed", " mm/min", 0, 5.0),
    num("printing.motion.retract_speed", "motion", "retract_speed", " mm/min", 0, 5.0).help("printing.all_layers.help"),
];

/// After the anti-aliasing choice.
const SLICING_ROWS: &[Num] = &[
    num("printing.estimates.resin_density", "estimates", "resin_density", " g/ml", 2, 0.01),
    num("printing.estimates.bottle_volume", "estimates", "bottle_volume", " ml", 0, 10.0),
    num("printing.estimates.bottle_cost", "estimates", "bottle_cost", "", 2, 0.5),
];

fn rows(ui: &mut egui::Ui, t: &Tokens, c: &mut PrintConfig, rows: &[Num]) {
    for r in rows {
        r.row(ui, t, c);
    }
}

fn printer_rows(ui: &mut egui::Ui, t: &Tokens, c: &mut PrintConfig) {
    ui_kit::wide_row(ui, t, i18n::tr("printing.printer.name"), |ui| {
        ui.add_sized(
            [ui.available_width(), CONTROL_HEIGHT],
            egui::TextEdit::singleline(&mut c.printer.name).char_limit(MAX_NAME_CHARS).vertical_align(egui::Align::Center),
        )
        .on_hover_text(i18n::tr("printing.printer.name.help"));
    });
    rows(ui, t, c, PRINTER_ROWS);
    let p = &mut c.printer;
    ui_kit::check_row(ui, t, i18n::tr("printing.printer.mirror_x"), i18n::tr("printing.printer.mirror.help"), &mut p.mirror_x);
    ui_kit::check_row(ui, t, i18n::tr("printing.printer.mirror_y"), i18n::tr("printing.printer.mirror.help"), &mut p.mirror_y);
    let (x, y) = p.pixel_pitch();
    let microns = |mm: f64| format!("{:.1}", mm * 1000.0);
    ui_kit::hint(ui, t, &i18n::format("printing.printer.pixel", &[("x", &microns(x)), ("y", &microns(y))]));
}

fn slicing_rows(ui: &mut egui::Ui, t: &Tokens, c: &mut PrintConfig) {
    let mut aa = c.settings.slicing.anti_aliasing.to_string();
    ui_kit::choice_row(ui, t, i18n::tr("printing.anti_aliasing"), "anti_aliasing", &mut aa, &ANTI_ALIASING_NAMES);
    if let Some(level) = ANTI_ALIASING.iter().find(|a| a.to_string() == aa) {
        c.settings.slicing.anti_aliasing = *level;
    }
    rows(ui, t, c, SLICING_ROWS);
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
    fn the_card_shows_while_open_on_every_tab_without_changing_a_setting() {
        let mut app = ResinApp::build(Prefs::default(), &[]);
        let ctx = egui::Context::default();
        frame(&ctx, &mut app, vec![]);
        assert!(!shown(&ctx));
        app.print_open = true;
        let (config, depth) = (app.session.print_config().clone(), app.session.undo_depth());
        for tab in 0..TABS.len() {
            app.print_tab = tab;
            frame(&ctx, &mut app, vec![]);
            frame(&ctx, &mut app, vec![]);
            assert!(shown(&ctx), "tab {tab}");
        }
        // Esc leaves a permanent card alone.
        frame(&ctx, &mut app, vec![key(egui::Key::Escape)]);
        assert!(app.print_open);
        assert_eq!((app.session.print_config(), app.session.undo_depth()), (&config, depth));
        app.print_open = false;
        frame(&ctx, &mut app, vec![]);
        frame(&ctx, &mut app, vec![]);
        assert!(!shown(&ctx));
    }

    #[test]
    fn the_close_button_closes_the_card() {
        let mut app = ResinApp::build(Prefs::default(), &[]);
        app.print_open = true;
        let ctx = egui::Context::default();
        // A window's first frame only sizes it.
        frame(&ctx, &mut app, vec![]);
        frame(&ctx, &mut app, vec![]);
        let card = ctx.memory(|m| m.area_rect(egui::Id::new(CARD_ID))).unwrap();
        let at = pos2(card.right() - SPACE_8 - FORM_CLOSE_SIZE / 2.0, card.top() + FORM_HEADER_HEIGHT / 2.0);
        let button =
            |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
        let config = app.session.print_config().clone();
        frame(&ctx, &mut app, vec![egui::Event::PointerMoved(at)]);
        frame(&ctx, &mut app, vec![button(true)]);
        frame(&ctx, &mut app, vec![button(false)]);
        assert!(!app.print_open);
        frame(&ctx, &mut app, vec![]);
        frame(&ctx, &mut app, vec![]);
        assert!(!shown(&ctx));
        assert_eq!(*app.session.print_config(), config, "closing changes no setting");
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
        app.print_open = true;
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
        let all = [PRINTER_ROWS, BOTTOM_ROWS, REGULAR_ROWS, SLICING_ROWS];
        let mut shown: Vec<(&str, &str)> = all.iter().flat_map(|rows| rows.iter().map(|r| (r.group, r.key))).collect();
        // The anti-aliasing choice, before the Slicing tab's numbers.
        shown.push(("slicing", "anti_aliasing"));
        shown.sort_unstable();
        let mut every: Vec<(&str, &str)> = resin_engine::slice::FIELDS.iter().map(|f| (f.group, f.key)).collect();
        every.sort_unstable();
        assert_eq!(shown, every);
    }

    #[test]
    fn every_card_word_is_in_the_catalog() {
        for id in TABS.into_iter().chain(ANTI_ALIASING_NAMES.map(|(_, id)| id)) {
            assert!(i18n::has(id), "{id}");
        }
        for r in [PRINTER_ROWS, BOTTOM_ROWS, REGULAR_ROWS, SLICING_ROWS].into_iter().flatten() {
            assert!(i18n::has(r.label) && r.help.is_none_or(i18n::has), "{}", r.label);
        }
        assert_eq!(ANTI_ALIASING_NAMES.map(|(v, _)| v.parse::<u8>().unwrap_or(0)), ANTI_ALIASING);
    }
}
