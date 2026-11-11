//! The object settings card, shown in Object edition while no Move, Rotate or Scale tool is open
//! (that tool's card takes its place): the lift height every lifted model sits at, whether the
//! selected models are lifted, and whether imports are. The height is scene state, so changing
//! it is an undo step; `ResinApp::run` keeps it in the preferences.

use egui::DragValue;
use resin_engine::LIFT_RANGE;
use serde_json::json;

use crate::theme::Tokens;
use crate::theme::metrics::{CONTROL_HEIGHT, FORM_CARD_WIDTH, SPACE_12};
use crate::ui_kit::{self, Buttons, Card};
use crate::{EditMode, ResinApp, i18n};

pub(crate) const CARD_ID: &str = "object_settings";

pub fn panel(app: &mut ResinApp, ctx: &egui::Context) {
    if app.mode != EditMode::Object || app.tool.is_some() {
        return;
    }
    let t = Tokens::for_dark(ctx.global_style().visuals.dark_mode);
    let at = app.viewport_rect.map_or(egui::pos2(220.0, 140.0), |r| r.left_top() + egui::vec2(SPACE_12, SPACE_12));
    let scene = &app.session.scene;
    let before = scene.lift_height();
    let mut height = before;
    let (selected, lifted) =
        scene.models().iter().filter(|m| scene.selected_models.contains(&m.id)).fold((0, 0), |(n, l), m| (n + 1, l + usize::from(m.lifted)));
    let mixed = lifted > 0 && lifted < selected;
    let mut lift_selection = selected > 0 && lifted == selected;
    let mut toggled = false;
    let mut on_import = app.prefs.lift_on_import;
    let mut arrange_on_import = app.prefs.arrange_on_import;
    let card = Card {
        id: egui::Id::new(CARD_ID),
        title: i18n::tr("models.settings.title"),
        icon: "move",
        at,
        bounds: app.viewport_rect,
        width: FORM_CARD_WIDTH,
        buttons: Buttons::None,
    };
    card.show(ctx, &t, |ui| {
        ui_kit::row(ui, &t, i18n::tr("models.lift_height"), |ui| {
            ui.add_sized(
                [ui.available_width(), CONTROL_HEIGHT],
                DragValue::new(&mut height).range(LIFT_RANGE).speed(0.1).suffix(" mm").max_decimals(2),
            )
            .on_hover_text(i18n::tr("models.lift_height.help"));
        });
        ui.add_enabled_ui(selected > 0, |ui| {
            let help = if selected > 0 { i18n::tr("models.lift_selection.help") } else { i18n::tr("reason.select_model") };
            toggled = ui_kit::mixed_check_row(ui, &t, i18n::tr("models.lift_selection"), help, &mut lift_selection, mixed).clicked();
        });
        ui_kit::check_row(ui, &t, i18n::tr("prefs.lift_on_import"), i18n::tr("prefs.lift_on_import.help"), &mut on_import);
        ui_kit::check_row(ui, &t, i18n::tr("prefs.arrange_on_import"), i18n::tr("prefs.arrange_on_import.help"), &mut arrange_on_import);
        ui_kit::hint(ui, &t, i18n::tr("models.lift_hint"));
    });
    app.prefs.lift_on_import = on_import;
    app.prefs.arrange_on_import = arrange_on_import;
    if height != before {
        // A dragged or typed height is one step: the app closes it once the gesture is over.
        app.session.begin_step();
        app.run("scene.lift_height", json!({ "height": height }));
    }
    if toggled {
        // A mixed box shows clear underneath, so a click ticks it: every selected model gets lifted.
        app.run("model.lift", json!({ "lifted": lift_selection }));
    }
}

#[cfg(test)]
mod tests {
    use egui::{Pos2, Rect, vec2};
    use serde_json::json;

    use super::{CARD_ID, panel};
    use crate::prefs::Prefs;
    use crate::{EditMode, ResinApp, transform};

    /// The app on `prefs` with a 10 mm cube imported as the app imports, from 0 to 10 in Z.
    fn app_with_a_cube(prefs: Prefs) -> (ResinApp, u64) {
        let dir = std::env::temp_dir().join(format!("resin-ui-lift-{}-{:?}", std::process::id(), std::thread::current().id()));
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
        let mut app = ResinApp::build(prefs, &[]);
        app.import(&path);
        std::fs::remove_dir_all(dir).ok();
        let id = app.session.scene.models()[0].id;
        (app, id)
    }

    fn bottom(app: &ResinApp) -> f64 {
        app.session.scene.models()[0].bounds.min.z
    }

    fn frame(ctx: &egui::Context, app: &mut ResinApp) {
        let raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1280.0, 800.0))), ..Default::default() };
        let mut out = ctx.run_ui(raw, |ui| panel(app, ui.ctx()));
        out.textures_delta.clear();
    }

    fn shown(ctx: &egui::Context) -> bool {
        ctx.memory(|m| m.areas().visible_layer_ids().iter().any(|l| l.id == egui::Id::new(CARD_ID)))
    }

    #[test]
    fn imports_are_lifted_unless_the_preference_is_off() {
        let (app, _) = app_with_a_cube(Prefs::default());
        assert_eq!(bottom(&app), resin_engine::DEFAULT_LIFT_HEIGHT);
        let (app, _) = app_with_a_cube(Prefs { lift_on_import: false, ..Prefs::default() });
        assert_eq!(bottom(&app), 0.0, "off, a model stays where the file puts it, as before lifting");
    }

    #[test]
    fn imports_are_arranged_unless_the_preference_is_off() {
        let (app, _) = app_with_a_cube(Prefs::default());
        let c = app.session.scene.models()[0].bounds.center();
        assert!(c.x.abs() < 1e-6 && c.y.abs() < 1e-6, "on the plate's centre: {c:?}");
        let (app, _) = app_with_a_cube(Prefs { arrange_on_import: false, ..Prefs::default() });
        let b = app.session.scene.models()[0].bounds;
        assert_eq!((b.min.x, b.min.y, b.max.x, b.max.y), (0.0, 0.0, 10.0, 10.0), "off, it stays where the file puts it");
    }

    #[test]
    fn the_stored_height_is_used_and_kept() {
        let (mut app, _) = app_with_a_cube(Prefs { lift_height: 2.0, ..Prefs::default() });
        assert_eq!(bottom(&app), 2.0);
        app.run("scene.lift_height", json!({ "height": 7.5 }));
        assert_eq!((bottom(&app), app.prefs.lift_height), (7.5, 7.5));
        app.step_history(false);
        assert_eq!((bottom(&app), app.prefs.lift_height), (2.0, 2.0));
    }

    #[test]
    fn cancelling_a_move_gives_the_lift_back() {
        let (mut app, id) = app_with_a_cube(Prefs::default());
        app.run("model.select", json!({ "ids": [id] }));
        transform::open(&mut app, transform::Op::Move);
        let z = app.session.scene.models()[0].placement.position.z;
        app.run("model.place", json!({ "models": [{ "id": id, "position": [0, 0, z + 3.0] }] }));
        assert!(!app.session.scene.models()[0].lifted);
        transform::cancel(&mut app);
        assert!(app.session.scene.models()[0].lifted);
        assert_eq!(bottom(&app), resin_engine::DEFAULT_LIFT_HEIGHT);
    }

    /// A gizmo drag rebuilds from where it began every frame: a turn or a scale held on a snap
    /// must not read as a move in Z and let the model go.
    #[test]
    fn a_gizmo_drag_keeps_the_lift() {
        use crate::gizmo::{Change, Drag, Handle};
        let (mut app, id) = app_with_a_cube(Prefs::default());
        app.run("model.select", json!({ "ids": [id] }));
        let changes = [
            (transform::Op::Rotate, Handle::Ring(0), Change::Rotate { axis: 0, angle: std::f64::consts::FRAC_PI_4 }),
            (transform::Op::Scale, Handle::Ball, Change::Scale { axis: None, factor: 2.0 }),
        ];
        for (op, handle, change) in changes {
            transform::open(&mut app, op);
            for (started, done) in [(true, false), (false, false), (false, true)] {
                transform::drag(&mut app, Drag { handle, change, started, done });
                assert!(app.session.scene.models()[0].lifted, "{op:?}");
                assert!((bottom(&app) - resin_engine::DEFAULT_LIFT_HEIGHT).abs() < 1e-9, "{op:?}: {}", bottom(&app));
            }
        }
        transform::open(&mut app, transform::Op::Move);
        let up = Change::Translate(resin_engine::geom::Vec3::new(0.0, 0.0, 3.0));
        transform::drag(&mut app, Drag { handle: Handle::Axis(2), change: up, started: true, done: true });
        assert!(!app.session.scene.models()[0].lifted);
    }

    #[test]
    fn the_card_shows_in_object_edition_without_a_tool() {
        let (mut app, id) = app_with_a_cube(Prefs::default());
        let ctx = egui::Context::default();
        frame(&ctx, &mut app);
        assert!(shown(&ctx));
        app.run("model.select", json!({ "ids": [id] }));
        transform::open(&mut app, transform::Op::Rotate);
        frame(&ctx, &mut app);
        assert!(!shown(&ctx), "the tool's card takes its place");
        app.tool = None;
        app.set_mode(EditMode::Support);
        frame(&ctx, &mut app);
        assert!(!shown(&ctx));
    }
}
