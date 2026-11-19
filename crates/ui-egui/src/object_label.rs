//! The label card (L in Object edition): the selected model's label on the raft, to tell prints
//! apart. Opening it on a model without one gives it its number in the model list (one step). The
//! card takes the object settings card's place; each edit runs `model.label`, a scene change, so
//! a dragged or typed value is one undo step. Delete runs `model.label.delete` and closes it.
//! It closes with the mode, when another tool opens, or once its model is not the one selected or
//! has no label (an undo).

use egui::DragValue;
use resin_engine::Model;
use resin_engine::geom::{LABEL_DEPTH_RANGE, LABEL_SIZE_RANGE, LabelSide, missing_chars};
use serde_json::{Map, Value, json};

use crate::theme::Tokens;
use crate::theme::metrics::{CONTROL_HEIGHT, FORM_CARD_WIDTH, SPACE_12};
use crate::ui_kit::{self, Action, Buttons, Card};
use crate::{EditMode, ResinApp, i18n, object_slicing};

pub(crate) const CARD_ID: &str = "object_label";

/// The sides' names, in [`LabelSide::ALL`]'s order.
const SIDES: [&str; 4] = ["tool.label.front", "tool.label.back", "tool.label.left", "tool.label.right"];

/// The model's name followed by its label's text (its lines joined), when it has one: how the
/// model list and the object slicing card name it.
pub(crate) fn titled(m: &Model) -> String {
    let text = m.label.as_ref().map(|l| l.text.split('\n').map(str::trim).filter(|l| !l.is_empty()).collect::<Vec<_>>().join(" / "));
    match text.filter(|t| !t.is_empty()) {
        Some(label) => i18n::format("models.labelled", &[("name", &m.name), ("label", &label)]),
        None => m.name.clone(),
    }
}

/// Open the card on the one selected model, giving it a label first if it has none.
pub fn open(app: &mut ResinApp) {
    if app.mode != EditMode::Object {
        app.status = Some((i18n::format("tool.needs_object_edition", &[("tool", &i18n::tr("tool.label"))]), true));
        return;
    }
    let Some((id, _)) = object_slicing::target(app) else {
        app.status = Some((i18n::tr("reason.select_one_model").into(), true));
        return;
    };
    if app.session.scene.model(id).is_some_and(|m| m.label.is_none()) && app.run("model.label", json!({ "id": id })).is_none() {
        return;
    }
    app.tool = None;
    app.label_tool = Some(id);
}

pub fn panel(app: &mut ResinApp, ctx: &egui::Context) {
    let Some(id) = app.label_tool else { return };
    let target = object_slicing::target(app);
    let label = app.session.scene.model(id).and_then(|m| m.label.clone());
    let (Some(label), true, true, Some((_, index))) =
        (label, app.mode == EditMode::Object && app.tool.is_none(), target.map(|t| t.0) == Some(id), target)
    else {
        app.label_tool = None;
        return;
    };
    let name = app.session.scene.model(id).map_or_else(String::new, |m| m.name.clone());
    let title = i18n::format("tool.label.title", &[("name", &name), ("index", &index)]);
    let t = Tokens::for_dark(ctx.global_style().visuals.dark_mode);
    let at = app.viewport_rect.map_or(egui::pos2(220.0, 140.0), |r| r.left_top() + egui::vec2(SPACE_12, SPACE_12));
    let why_not = why_not_printed(app, id, &label.text);
    let (mut side, mut size, mut depth, mut text) =
        (LabelSide::ALL.iter().position(|s| *s == label.side).unwrap_or(0), label.size, label.depth, label.text.clone());
    let mut delete = false;
    let card = Card {
        id: egui::Id::new(CARD_ID),
        title: &title,
        icon: "",
        at,
        bounds: app.viewport_rect,
        width: FORM_CARD_WIDTH,
        buttons: Buttons::CloseOnly,
    };
    let action = card.show(ctx, &t, |ui| {
        ui_kit::row(ui, &t, i18n::tr("tool.label.side"), |ui| {
            let names = SIDES.map(i18n::tr);
            ui_kit::segmented(ui, &t, &mut side, &names);
        });
        ui_kit::row(ui, &t, i18n::tr("tool.label.size"), |ui| {
            ui.add_sized(
                [ui.available_width(), CONTROL_HEIGHT],
                DragValue::new(&mut size).range(LABEL_SIZE_RANGE).speed(0.1).suffix(" mm").max_decimals(2),
            )
            .on_hover_text(i18n::tr("tool.label.size_tip"));
        });
        ui_kit::row(ui, &t, i18n::tr("tool.label.depth"), |ui| {
            ui.add_sized(
                [ui.available_width(), CONTROL_HEIGHT],
                DragValue::new(&mut depth).range(LABEL_DEPTH_RANGE).speed(0.05).suffix(" mm").max_decimals(2),
            )
            .on_hover_text(i18n::tr("tool.label.depth_tip"));
        });
        ui_kit::row(ui, &t, i18n::tr("tool.label.text"), |_| {});
        ui.add(egui::TextEdit::multiline(&mut text).desired_rows(3).desired_width(f32::INFINITY));
        if let Some(why) = &why_not {
            ui.colored_label(t.warning, why);
        }
        ui_kit::hint(ui, &t, i18n::tr("tool.label.hint"));
        delete = ui_kit::button(ui, i18n::tr("tool.label.delete")).on_hover_text(i18n::tr("tool.label.delete_tip")).clicked();
    });
    if action == Action::Cancel {
        app.label_tool = None;
    }
    if delete {
        app.run("model.label.delete", json!({ "id": id }));
        app.label_tool = None;
        return;
    }
    let mut set = Map::new();
    if let Some(s) = LabelSide::ALL.get(side).filter(|s| **s != label.side) {
        set.insert("side".into(), json!(s.id()));
    }
    if size != label.size {
        set.insert("size".into(), json!(size));
    }
    if depth != label.depth {
        set.insert("depth".into(), json!(depth));
    }
    if text != label.text {
        set.insert("text".into(), json!(text));
    }
    if set.is_empty() {
        return;
    }
    set.insert("id".into(), json!(id));
    // A dragged or typed value is one step: the app closes it once the gesture is over.
    app.session.begin_step();
    app.run("model.label", Value::Object(set));
}

/// Why model `id`'s label is not printed, if it is not: no raft, no support of the model reaching
/// the plate, no letters; or which characters are left out.
fn why_not_printed(app: &ResinApp, id: u64, text: &str) -> Option<String> {
    if !app.session.support_settings().current().raft.enabled {
        return Some(i18n::tr("tool.label.no_raft").into());
    }
    let scene = &app.session.scene;
    if !scene.supports().iter().any(|s| s.model == id && app.routes.route(s.id).is_some_and(|r| r.foot)) {
        return Some(i18n::tr("tool.label.no_feet").into());
    }
    if text.trim().is_empty() {
        return Some(i18n::tr("tool.label.empty").into());
    }
    let missing = missing_chars(text);
    (!missing.is_empty()).then(|| i18n::format("tool.label.missing", &[("chars", &missing.iter().collect::<String>())]))
}

#[cfg(test)]
mod tests {
    use egui::{Pos2, Rect, vec2};

    use super::*;
    use crate::prefs::Prefs;

    fn frame(ctx: &egui::Context, app: &mut ResinApp) {
        let raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1280.0, 800.0))), ..Default::default() };
        let mut out = ctx.run_ui(raw, |ui| panel(app, ui.ctx()));
        out.textures_delta.clear();
    }

    fn shown(ctx: &egui::Context) -> bool {
        ctx.memory(|m| m.areas().visible_layer_ids().iter().any(|l| l.id == egui::Id::new(CARD_ID)))
    }

    /// The app with two 10 mm cubes, in Object edition.
    fn app_with_two_cubes() -> (ResinApp, [u64; 2]) {
        let dir = std::env::temp_dir().join(format!("resin-ui-label-{}-{:?}", std::process::id(), std::thread::current().id()));
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
        let ids = [app.session.scene.models()[0].id, app.session.scene.models()[1].id];
        (app, ids)
    }

    #[test]
    fn opening_numbers_the_one_selected_model_and_the_card_follows_it() {
        let (mut app, [a, b]) = app_with_two_cubes();
        let ctx = egui::Context::default();
        app.run("model.select", json!({ "ids": [a, b] }));
        open(&mut app);
        assert_eq!(app.label_tool, None, "two models selected");
        app.run("model.select", json!({ "ids": [b] }));
        let depth = app.session.undo_depth();
        open(&mut app);
        assert_eq!(app.label_tool, Some(b));
        assert_eq!(app.session.undo_depth(), depth + 1, "the new label is one step");
        assert_eq!(app.session.scene.model(b).unwrap().label.as_ref().map(|l| l.text.as_str()), Some("2"));
        open(&mut app);
        assert_eq!(app.session.undo_depth(), depth + 1, "opening on a labelled model changes nothing");
        frame(&ctx, &mut app);
        frame(&ctx, &mut app);
        assert!(shown(&ctx));
        assert_eq!(app.session.undo_depth(), depth + 1, "shown, it makes no edit of its own");

        app.run("model.select", json!({ "ids": [a] }));
        frame(&ctx, &mut app);
        assert_eq!(app.label_tool, None, "closed once its model is not the one selected");
        app.run("model.select", json!({ "ids": [b] }));
        open(&mut app);
        app.step_history(false);
        frame(&ctx, &mut app);
        assert_eq!(app.label_tool, None, "closed when an undo takes the label away");
        open(&mut app);
        app.mode = EditMode::Support;
        frame(&ctx, &mut app);
        assert_eq!(app.label_tool, None, "closed with the mode");
        open(&mut app);
        assert_eq!(app.label_tool, None, "Object edition only");
    }

    #[test]
    fn a_labelled_model_is_named_with_its_label() {
        let (mut app, [a, _]) = app_with_two_cubes();
        let name = |app: &ResinApp| titled(app.session.scene.model(a).unwrap());
        let plain = app.session.scene.model(a).unwrap().name.clone();
        assert_eq!(name(&app), plain);
        app.run("model.label", json!({ "id": a, "text": " A1 \n\nB2" }));
        assert_eq!(name(&app), i18n::format("models.labelled", &[("name", &plain), ("label", &"A1 / B2")]));
        app.run("model.label", json!({ "id": a, "text": " \n " }));
        assert_eq!(name(&app), plain, "no letters, no label shown");
    }

    #[test]
    fn the_reason_it_is_not_printed_is_shown() {
        let (mut app, [a, _]) = app_with_two_cubes();
        app.run("model.select", json!({ "ids": [a] }));
        open(&mut app);
        assert_eq!(why_not_printed(&app, a, "1"), Some(i18n::tr("tool.label.no_feet").into()));
        app.run("support.configure", json!({ "raft": { "enabled": false } }));
        assert_eq!(why_not_printed(&app, a, "1"), Some(i18n::tr("tool.label.no_raft").into()));
    }

    #[test]
    fn every_card_word_is_in_the_catalog() {
        for id in SIDES.into_iter().chain([
            "tool.label",
            "tool.label.title",
            "tool.label.side",
            "tool.label.size",
            "tool.label.size_tip",
            "tool.label.depth",
            "tool.label.depth_tip",
            "tool.label.text",
            "tool.label.hint",
            "tool.label.delete",
            "tool.label.delete_tip",
            "tool.label.no_raft",
            "tool.label.no_feet",
            "tool.label.empty",
            "tool.label.missing",
            "models.labelled",
            "tool.needs_object_edition",
            "reason.select_one_model",
        ]) {
            assert!(i18n::has(id), "{id}");
        }
    }
}
