//! The object slicing card: the selected model's own slicing settings, in place of the plate's
//! Slicing tab for it and its supports, to calibrate one model against the others. Opened from
//! the Slicing tab's Object Overrides button; shown in Slice edition while exactly one model is
//! selected, following the selection, and closed once there is not. Its rows are the Slicing
//! tab's, showing the plate's values with the model's own put in; a row the model sets is
//! highlighted, with a button back to the plate's value. Every edit runs `model.slicing`, a scene
//! change, so it is an undo step.

use resin_engine::slice::{SLICING_KEYS, Slicing};
use serde_json::{Map, Value, json};

use crate::print_settings::{self, Marks};
use crate::theme::Tokens;
use crate::theme::metrics::*;
use crate::ui_kit::{self, Action, Buttons, Card};
use crate::{EditMode, ResinApp, i18n};

pub(crate) const CARD_ID: &str = "object_slicing";

/// The one selected model, and its place in the model list from 1: the card's model.
pub(crate) fn target(app: &ResinApp) -> Option<(u64, usize)> {
    let scene = &app.session.scene;
    let [id] = scene.selected_models.iter().copied().collect::<Vec<_>>()[..] else { return None };
    scene.models().iter().position(|m| m.id == id).map(|i| (id, i + 1))
}

pub fn panel(app: &mut ResinApp, ctx: &egui::Context) {
    let target = target(app);
    if app.mode != EditMode::Slice || target.is_none() {
        app.object_slicing = false;
    }
    let (Some((id, index)), true) = (target, app.object_slicing) else { return };
    let Some(model) = app.session.scene.model(id) else { return };
    let own = model.slicing;
    let title = i18n::format("printing.object.title", &[("name", &crate::object_label::titled(model)), ("index", &index)]);
    let t = Tokens::for_dark(ctx.global_style().visuals.dark_mode);
    // Beside the print settings card, which opens at the viewport's top left.
    // Beside the print settings card.
    let at = crate::models_card::beside(app) + egui::vec2(FORM_TABBED_CARD_WIDTH + SPACE_12, 0.0);
    let mut config = app.session.print_config().clone();
    let plate = config.settings.slicing;
    let before = own.apply(&plate);
    config.settings.slicing = before;
    let mut marks = Marks { own: Some(&own), cleared: Vec::new() };
    let mut clear_all = false;
    let card = Card {
        id: egui::Id::new(CARD_ID),
        title: &title,
        icon: "settings",
        at,
        bounds: app.viewport_rect,
        width: FORM_TABBED_CARD_WIDTH,
        buttons: Buttons::CloseOnly,
    };
    let action = card.show(ctx, &t, |ui| {
        print_settings::slicing_rows(ui, &t, &mut config, &mut marks);
        ui.add_enabled_ui(!own.is_empty(), |ui| {
            if ui_kit::action_row(ui, &t, i18n::tr("printing.object.clear_all")).on_hover_text(i18n::tr("printing.object.clear_all.help")).clicked() {
                clear_all = true;
            }
        });
        ui_kit::hint(ui, &t, i18n::tr("printing.object.hint"));
    });
    if action == Action::Cancel {
        app.object_slicing = false;
    }
    let cleared = if clear_all { SLICING_KEYS.to_vec() } else { marks.cleared };
    let set = changes(&before, &config.settings.slicing);
    if set.is_empty() && cleared.is_empty() {
        return;
    }
    // A dragged or typed value is one step: the app closes it once the gesture is over.
    app.session.begin_step();
    app.run("model.slicing", json!({ "ids": [id], "set": set, "clear": cleared }));
}

/// `model.slicing`'s `set` for what differs from `before` to `after`.
fn changes(before: &Slicing, after: &Slicing) -> Map<String, Value> {
    let (Ok(Value::Object(a)), Ok(Value::Object(b))) = (serde_json::to_value(before), serde_json::to_value(after)) else { return Map::new() };
    SLICING_KEYS.iter().filter_map(|k| b.get(*k).filter(|v| a.get(*k) != Some(*v)).map(|v| ((*k).to_owned(), v.clone()))).collect()
}

#[cfg(test)]
mod tests {
    use egui::{Pos2, Rect, vec2};
    use resin_engine::slice::SlicingOverride;

    use super::*;
    use crate::prefs::Prefs;

    fn frame(ctx: &egui::Context, app: &mut ResinApp) {
        let raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1280.0, 800.0))), ..Default::default() };
        let mut out = ctx.run_ui(raw, |ui| {
            print_settings::panel(app, ui.ctx());
            panel(app, ui.ctx());
        });
        out.textures_delta.clear();
    }

    fn shown(ctx: &egui::Context) -> bool {
        ctx.memory(|m| m.areas().visible_layer_ids().iter().any(|l| l.id == egui::Id::new(CARD_ID)))
    }

    /// The app with two 10 mm cubes, in Slice edition (set directly: `set_mode` would slice).
    fn app_with_two_cubes() -> (ResinApp, [u64; 2]) {
        let dir = std::env::temp_dir().join(format!("resin-ui-object-slicing-{}-{:?}", std::process::id(), std::thread::current().id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("cube.stl");
        let v = |i: usize| format!("{} {} {}", (i & 1) * 10, (i >> 1 & 1) * 10, (i >> 2 & 1) * 10);
        let mut text = String::from("solid c\n");
        for [a, b, c, d] in [[0, 2, 3, 1], [4, 5, 7, 6], [0, 1, 5, 4], [2, 6, 7, 3], [0, 4, 6, 2], [1, 3, 7, 5]] {
            for [p, q, r] in [[a, b, c], [a, c, d]] {
                text += &format!("facet normal 0 0 0\nouter loop\nvertex {}\nvertex {}\nvertex {}\nendloop\nendfacet\n", v(p), v(q), v(r));
            }
        }
        text += "endsolid c\n";
        std::fs::write(&path, text).unwrap();
        let mut app = ResinApp::build(Prefs::default(), &[]);
        app.import(&path);
        app.import(&path);
        std::fs::remove_dir_all(dir).ok();
        app.mode = EditMode::Slice;
        let ids = [app.session.scene.models()[0].id, app.session.scene.models()[1].id];
        (app, ids)
    }

    #[test]
    fn the_card_needs_one_selected_model_and_follows_it() {
        let (mut app, [a, b]) = app_with_two_cubes();
        let ctx = egui::Context::default();
        app.object_slicing = true;
        app.run("model.select", json!({ "ids": [a, b] }));
        frame(&ctx, &mut app);
        assert!(!app.object_slicing && target(&app).is_none(), "two models: closed");
        app.run("model.select", json!({ "ids": [b] }));
        assert_eq!(target(&app), Some((b, 2)));
        app.object_slicing = true;
        frame(&ctx, &mut app);
        frame(&ctx, &mut app);
        assert!(shown(&ctx));
        app.mode = EditMode::Object;
        frame(&ctx, &mut app);
        assert!(!app.object_slicing, "closed with the mode");
    }

    #[test]
    fn an_edit_sets_only_what_changed_as_one_step_and_clearing_gives_the_plates_back() {
        let (mut app, [a, _]) = app_with_two_cubes();
        app.run("model.select", json!({ "ids": [a] }));
        let plate = app.session.print_config().settings.slicing;
        let mut after = plate;
        after.blur_passes = 3;
        after.exact = true;
        let set = changes(&plate, &after);
        assert_eq!(Value::Object(set.clone()), json!({ "blur_passes": 3, "exact": true }));
        let depth = app.session.undo_depth();
        app.run("model.slicing", json!({ "ids": [a], "set": set, "clear": [] }));
        assert_eq!(app.session.undo_depth(), depth + 1);
        let own = app.session.scene.model(a).unwrap().slicing;
        assert_eq!(own, SlicingOverride { blur_passes: Some(3), exact: Some(true), ..SlicingOverride::default() });
        assert!(changes(&after, &after).is_empty());
        app.run("model.slicing", json!({ "ids": [a], "clear": ["blur_passes"] }));
        assert_eq!(app.session.scene.model(a).unwrap().slicing.keys().collect::<Vec<_>>(), ["exact"]);
        // Shown, it makes no edit of its own.
        let ctx = egui::Context::default();
        app.object_slicing = true;
        let depth = app.session.undo_depth();
        for tab in 0..print_settings::TABS.len() {
            app.print_tab = tab;
            frame(&ctx, &mut app);
            frame(&ctx, &mut app);
        }
        assert!(shown(&ctx));
        assert_eq!(app.session.undo_depth(), depth);
    }

    #[test]
    fn every_card_word_is_in_the_catalog() {
        for id in [
            "printing.object.title",
            "printing.object.open",
            "printing.object.open.help",
            "printing.object.clear_all",
            "printing.object.clear_all.help",
            "printing.object.hint",
            "printing.object.use_plate",
            "reason.select_one_model",
        ] {
            assert!(i18n::has(id), "{id}");
        }
    }
}
