//! The tip line (L in Support edition): tips evenly spaced between two tips, cast onto the
//! surface. With two supports selected the line runs between them; with one, or none (it then
//! starts from the newest tip), it runs to where the next press on a model's surface puts its
//! other end (a later press moves it). The points
//! show live, green where they land, in the warning colour on the straight line where they land
//! nowhere; Shift+wheel or the card changes the spacing. Enter (OK) runs `support.add_line`, one
//! step; Esc, Cancel, undo, a mode change or an end going away puts the line away, the scene
//! and the undo history untouched. The spacing and tips per body are kept in the preferences.

use egui::vec2;
use resin_engine::geom::Vec3;
use resin_engine::{DEFAULT_LINE_SPACING, DEFAULT_TIPS_PER_BODY, LinePoint, line_points};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::supports::Hover;
use crate::theme::Tokens;
use crate::theme::metrics::{CONTROL_HEIGHT, FORM_CARD_WIDTH, SPACE_12};
use crate::ui_kit::{self, Action, Buttons, Card};
use crate::{EditMode, ResinApp, i18n};

/// The spacing Shift+wheel and the card keep to, in mm.
pub const SPACING_RANGE: std::ops::RangeInclusive<f64> = 0.2..=50.0;
/// What one wheel notch adds to the spacing, in mm.
const SPACING_STEP: f64 = 0.1;
/// The most tips the card lets share one new body.
const MAX_TIPS_PER_BODY: usize = 50;

/// The line's other end.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum End {
    Support(u64),
    /// Where a new support goes when the line is made.
    Surface {
        model: u64,
        point: Vec3,
        normal: Vec3,
    },
}

/// A line being laid out.
#[derive(Clone, Debug)]
pub struct LineTool {
    pub from: u64,
    /// `None` until a press on a surface puts it there.
    pub to: Option<End>,
    /// The points and what they were cast at: the scene's revision, both ends and the spacing.
    /// Casting them every frame while nothing moves would be wasted.
    cache: Option<((u64, (Vec3, Vec3), (Vec3, Vec3), f64), Vec<LinePoint>)>,
}

/// The tool and what it keeps from one line to the next.
pub struct LineState {
    pub tool: Option<LineTool>,
    pub spacing: f64,
    pub per_body: usize,
}

impl Default for LineState {
    fn default() -> Self {
        LineState { tool: None, spacing: DEFAULT_LINE_SPACING, per_body: DEFAULT_TIPS_PER_BODY }
    }
}

/// The line's settings as the preferences keep them.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LineSettings {
    pub spacing: f64,
    pub per_body: usize,
}

impl Default for LineSettings {
    fn default() -> Self {
        LineState::default().settings()
    }
}

impl LineState {
    pub fn settings(&self) -> LineSettings {
        LineSettings { spacing: self.spacing, per_body: self.per_body }
    }
    /// Take the stored settings, each one out of range keeping its default; those, by name.
    pub fn restore(&mut self, s: &LineSettings) -> Vec<&'static str> {
        let mut bad = Vec::new();
        if SPACING_RANGE.contains(&s.spacing) {
            self.spacing = s.spacing;
        } else {
            bad.push("spacing");
        }
        if (1..=MAX_TIPS_PER_BODY).contains(&s.per_body) {
            self.per_body = s.per_body;
        } else {
            bad.push("per_body");
        }
        bad
    }
}

/// Start a line from the selected tips: two run between each other, one (or the newest tip when
/// none is selected) waits for a press.
pub fn open(app: &mut ResinApp) {
    if app.mode != EditMode::Support {
        app.status = Some((i18n::format("tool.needs_support_edition", &[("tool", &i18n::tr("tool.line"))]), true));
        return;
    }
    let scene = &app.session.scene;
    let (from, to) = match scene.selected.iter().copied().collect::<Vec<_>>().as_slice() {
        // Ids only grow, so the highest is the newest tip: the one just placed, or where the last
        // line ended, so that lines chain on without a selection.
        [] if scene.selected_parts.is_empty() => match scene.supports().iter().map(|t| t.id).max() {
            Some(newest) => (newest, None),
            None => {
                app.status = Some((i18n::tr("reason.add_tip_first").into(), true));
                return;
            }
        },
        [from] => (*from, None),
        [from, to] => (*from, Some(End::Support(*to))),
        _ => {
            app.status = Some((i18n::tr("reason.select_line_tips").into(), true));
            return;
        }
    };
    app.supports.branching = None;
    app.supports.moving = None;
    app.status = None;
    app.line.tool = Some(LineTool { from, to, cache: None });
}

/// Whether the line waits for its other end: the view then picks surfaces alone.
pub fn placing(app: &ResinApp) -> bool {
    app.line.tool.as_ref().is_some_and(|t| !matches!(t.to, Some(End::Support(_))))
}

/// A press in the view while the line is out: on a surface, the other end goes there.
pub fn press(app: &mut ResinApp, hit: Option<Hover>) {
    let waits = placing(app);
    if let (Some(tool), Some(Hover::Surface { model, point, normal }), true) = (app.line.tool.as_mut(), hit, waits) {
        tool.to = Some(End::Surface { model, point, normal });
    }
}

/// Take Shift+wheel over the view for the spacing, before the camera sees it.
pub fn wheel(app: &mut ResinApp, ui: &egui::Ui, resp: &egui::Response) {
    if app.line.tool.is_none() || !resp.contains_pointer() {
        return;
    }
    let line = ui.ctx().options(|o| o.input_options.line_scroll_speed).max(1.0);
    let mut notches = 0.0f32;
    ui.ctx().input_mut(|i| {
        i.events.retain(|e| match e {
            egui::Event::MouseWheel { unit, delta, modifiers, .. } if modifiers.shift => {
                // egui turns a Shift+wheel sideways on some systems.
                let d = if delta.y != 0.0 { delta.y } else { delta.x };
                notches += match unit {
                    egui::MouseWheelUnit::Line => d,
                    egui::MouseWheelUnit::Point => d / line,
                    egui::MouseWheelUnit::Page => d * 10.0,
                };
                false
            }
            _ => true,
        });
    });
    if notches.is_finite() && notches != 0.0 {
        let s = app.line.spacing + f64::from(notches) * SPACING_STEP;
        app.line.spacing = ((s / SPACING_STEP).round() * SPACING_STEP).clamp(*SPACING_RANGE.start(), *SPACING_RANGE.end());
    }
}

/// The two ends, each a point and its outward normal: the other end while it is placed is the
/// surface under the pointer.
fn ends(app: &ResinApp) -> Option<((Vec3, Vec3), (Vec3, Vec3))> {
    let tool = app.line.tool.as_ref()?;
    let scene = &app.session.scene;
    let tip = |id: u64| scene.supports().iter().find(|s| s.id == id).map(|s| (s.tip, s.normal));
    let b = match tool.to {
        Some(End::Support(id)) => tip(id)?,
        Some(End::Surface { point, normal, .. }) => (point, normal),
        None => match app.supports.hover {
            Some(Hover::Surface { point, normal, .. }) => (point, normal),
            _ => return None,
        },
    };
    Some((tip(tool.from)?, b))
}

/// Once a frame: put the line away when an end has gone, and cast its points afresh when an end,
/// the spacing or the scene changed.
pub fn refresh(app: &mut ResinApp) {
    let Some(tool) = app.line.tool.as_ref() else { return };
    let known = |id: u64| app.session.scene.supports().iter().any(|s| s.id == id);
    if app.mode != EditMode::Support || !known(tool.from) || matches!(tool.to, Some(End::Support(id)) if !known(id)) {
        app.line.tool = None;
        return;
    }
    let key = ends(app).map(|(a, b)| (app.session.scene.revision(), a, b, app.line.spacing));
    let fresh = match key {
        Some(k) if tool.cache.as_ref().is_some_and(|(c, _)| *c == k) => return,
        Some(k) => line_points(&app.session.scene, k.1, k.2, k.3).map(|p| (k, p)),
        None => None,
    };
    if let Some(tool) = app.line.tool.as_mut() {
        tool.cache = fresh;
    }
}

/// The points as last cast, while the line is out.
pub fn points(app: &ResinApp) -> &[LinePoint] {
    app.line.tool.as_ref().and_then(|t| t.cache.as_ref()).map_or(&[], |(_, p)| p.as_slice())
}

/// The straight line between the ends, and the other end when it is a new support, to draw.
pub fn guide(app: &ResinApp) -> Option<(Vec3, Vec3, Option<Vec3>)> {
    let (a, b) = ends(app)?;
    let new_end = match app.line.tool.as_ref()?.to {
        Some(End::Surface { point, .. }) => Some(point),
        _ => None,
    };
    Some((a.0, b.0, new_end))
}

/// Whether both ends share one body: the new tips then join it.
fn same_body(app: &ResinApp) -> bool {
    let scene = &app.session.scene;
    match app.line.tool.as_ref().and_then(|t| t.to.map(|to| (t.from, to))) {
        Some((from, End::Support(to))) => scene.root_of(from).is_some_and(|r| scene.root_of(to) == Some(r)),
        _ => false,
    }
}

/// The card: the spacing, how many tips share a body, and what Enter would add.
pub fn card(app: &mut ResinApp, ctx: &egui::Context) {
    let Some(tool) = app.line.tool.as_ref() else { return };
    let (to, from) = (tool.to, tool.from);
    let t = Tokens::for_dark(ctx.global_style().visuals.dark_mode);
    let landed = points(app).iter().filter(|p| p.hit.is_some()).count();
    let off = points(app).len() - landed;
    let adds = landed + usize::from(matches!(to, Some(End::Surface { .. })));
    let same = same_body(app);
    let (mut spacing, mut per_body) = (app.line.spacing, app.line.per_body);
    let card = Card {
        id: egui::Id::new("support_line"),
        title: i18n::tr("tool.line"),
        icon: "cone",
        at: egui::Pos2::ZERO,
        bounds: app.viewport_rect,
        width: FORM_CARD_WIDTH,
        buttons: Buttons::OkCancel { ok_enabled: to.is_some() && adds > 0 },
    };
    // Pinned to the view's top right, clear of the support settings at its top left: placed by
    // its top left, egui's first sizing pass, at its default width, would push it leftwards.
    let action = card.show_anchored(ctx, &t, egui::Align2::RIGHT_TOP, vec2(-SPACE_12, SPACE_12), |ui| {
        ui_kit::row(ui, &t, i18n::tr("tool.line.spacing"), |ui| {
            ui.add_sized(
                [ui.available_width(), CONTROL_HEIGHT],
                egui::DragValue::new(&mut spacing).speed(0.05).range(SPACING_RANGE).suffix(" mm").max_decimals(2),
            )
        });
        ui_kit::row(ui, &t, i18n::tr("tool.line.per_body"), |ui| {
            ui.add_enabled_ui(!same, |ui| {
                ui.add_sized([ui.available_width(), CONTROL_HEIGHT], egui::DragValue::new(&mut per_body).speed(0.05).range(1..=MAX_TIPS_PER_BODY))
            })
            .inner
            .on_hover_text(i18n::tr("tool.line.per_body_tip"))
            .on_disabled_hover_text(i18n::tr("tool.line.same_body"))
        });
        if to.is_none() {
            ui_kit::hint(ui, &t, i18n::tr("status.line_place"));
        } else {
            ui_kit::hint(ui, &t, &i18n::count("tool.line.adds", adds, &[]));
        }
        if off > 0 {
            ui.label(egui::RichText::new(i18n::count("tool.line.off_surface", off, &[])).color(t.warning));
        }
        ui_kit::hint(ui, &t, i18n::tr("tool.line.wheel"));
    });
    app.line.spacing = spacing.clamp(*SPACING_RANGE.start(), *SPACING_RANGE.end());
    app.line.per_body = per_body.clamp(1, MAX_TIPS_PER_BODY);
    match action {
        Action::Ok => make(app, from, to),
        Action::Cancel => app.line.tool = None,
        Action::Reset | Action::None => {}
    }
}

/// Run `support.add_line` and put the line away.
fn make(app: &mut ResinApp, from: u64, to: Option<End>) {
    let to = match to {
        Some(End::Support(id)) => json!(id),
        Some(End::Surface { model, point, normal }) => json!({ "model": model, "point": point, "normal": normal }),
        None => return,
    };
    let params = json!({ "from": from, "to": to, "spacing": app.line.spacing, "per_body": app.line.per_body });
    if let Some(v) = app.run("support.add_line", params) {
        let n = v["ids"].as_array().map_or(0, Vec::len);
        app.status = Some((i18n::count("status.line_added", n, &[]), false));
        app.line.tool = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::navigation::Proj;
    use crate::prefs::Prefs;
    use egui::{Event, Modifiers, Pos2, Rect};

    /// L opens the line between the two selected tips, Shift+wheel changes its spacing and leaves
    /// the camera alone, Enter adds its tips in one step, Esc puts it away with no step; with one
    /// tip selected, or none (the newest then), a press on the model places the other end, a new
    /// support.
    #[test]
    fn l_lays_a_line_of_tips_that_enter_adds() {
        let dir = std::env::temp_dir().join(format!("resin-ui-line-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("plate.stl");
        // A floating square plate, facing down.
        let text = "solid p\nfacet normal 0 0 0\nouter loop\nvertex 0 0 10\nvertex 0 20 10\nvertex 20 20 10\nendloop\nendfacet\n\
                    facet normal 0 0 0\nouter loop\nvertex 0 0 10\nvertex 20 20 10\nvertex 20 0 10\nendloop\nendfacet\nendsolid p\n";
        std::fs::write(&path, text).unwrap();
        let mut app = ResinApp::build(Prefs::default(), &[]);
        let m = app.run("file.import_stl", json!({ "path": path, "fit": false })).unwrap()["id"].as_u64().unwrap();
        std::fs::remove_dir_all(dir).ok();
        let add = |app: &mut ResinApp, x: f64, y: f64| {
            app.run("support.add", json!({ "model": m, "point": [x, y, 10], "normal": [0, 0, -1] })).unwrap()["id"].as_u64().unwrap()
        };
        let (a, b) = (add(&mut app, 2.0, 10.0), add(&mut app, 18.0, 10.0));
        app.mode = EditMode::Support;
        app.run("view.standard", json!({ "view": "bottom" })).unwrap();
        app.run("view.fit", json!({ "aspect": 800.0 / 600.0 })).unwrap();
        let rect = Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0));
        let proj = Proj { cam: app.session.camera, rect };
        // The card then sits in the view's top right, clear of the model.
        app.viewport_rect = Some(rect);
        let ctx = egui::Context::default();
        let mut time = 0.0;
        // One frame as the app runs it: the keys, the view (wheel, camera, picking, the line's
        // points), the card.
        let mut frame = |app: &mut ResinApp, events: Vec<Event>| {
            time += 0.5;
            let raw = egui::RawInput { screen_rect: Some(rect), time: Some(time), events, ..Default::default() };
            let mut out = ctx.run_ui(raw, |ui| {
                app.keys(ui.ctx());
                let resp = ui.allocate_rect(rect, egui::Sense::click_and_drag());
                wheel(app, ui, &resp);
                crate::navigation::handle(app, ui, rect, &resp);
                let proj = Proj { cam: app.session.camera, rect };
                crate::supports::handle(app, ui, &proj, &resp);
                refresh(app);
                card(app, ui.ctx());
            });
            out.textures_delta.clear();
        };
        // Pressed and let go: egui takes a press of a key still down for a repeat.
        let key = |key: egui::Key, modifiers: Modifiers| {
            [true, false].map(|pressed| Event::Key { key, physical_key: None, pressed, repeat: false, modifiers }).to_vec()
        };
        let centre = rect.center();
        // egui knows the pointer is over the view from the frame before.
        frame(&mut app, vec![Event::PointerMoved(centre)]);

        app.run("support.select", json!({ "ids": [a, b] }));
        frame(&mut app, key(egui::Key::L, Modifiers::NONE));
        assert_eq!(app.line.tool.as_ref().map(|t| (t.from, t.to)), Some((a, Some(End::Support(b)))));
        // 16 mm in gaps of at most 2: 7 points, all on the plate.
        assert_eq!(points(&app).iter().filter(|p| p.hit.is_some()).count(), 7);

        let cam = app.session.camera;
        let notch =
            Event::MouseWheel { unit: egui::MouseWheelUnit::Line, delta: vec2(0.0, 1.0), modifiers: Modifiers::SHIFT, phase: egui::TouchPhase::Move };
        frame(&mut app, vec![Event::PointerMoved(centre), notch.clone(), notch]);
        assert!((app.line.spacing - 2.2).abs() < 1e-9, "{}", app.line.spacing);
        assert_eq!(app.session.camera, cam, "the wheel went to the spacing, not the camera");
        // 16 mm in gaps of at most 2.2: still 7 points.
        assert_eq!(points(&app).len(), 7);

        // Esc: nothing added, no step.
        let (revision, depth) = (app.session.scene.revision(), app.session.undo_depth());
        frame(&mut app, key(egui::Key::Escape, Modifiers::NONE));
        assert!(app.line.tool.is_none());
        assert_eq!((app.session.scene.revision(), app.session.undo_depth()), (revision, depth));

        // Enter: the 7 tips in one step.
        frame(&mut app, key(egui::Key::L, Modifiers::NONE));
        frame(&mut app, vec![]);
        frame(&mut app, key(egui::Key::Enter, Modifiers::NONE));
        assert!(app.line.tool.is_none());
        assert_eq!((app.session.scene.supports().len(), app.session.undo_depth()), (9, depth + 1));

        // One tip: the other end goes where the model is pressed, a new support with the line.
        app.run("support.select", json!({ "ids": [a] }));
        frame(&mut app, key(egui::Key::L, Modifiers::NONE));
        assert!(placing(&app));
        let end = proj.to_screen(Vec3::new(10.0, 2.0, 10.0)).unwrap();
        let press = |pressed| Event::PointerButton { pos: end, button: egui::PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
        frame(&mut app, vec![Event::PointerMoved(end)]);
        frame(&mut app, vec![Event::PointerMoved(end), press(true)]);
        frame(&mut app, vec![press(false)]);
        assert!(matches!(app.line.tool.as_ref().and_then(|t| t.to), Some(End::Surface { .. })), "{:?}", app.line.tool);
        assert_eq!(app.session.scene.supports().len(), 9, "nothing added before Enter");
        frame(&mut app, key(egui::Key::Enter, Modifiers::NONE));
        // About 11.3 mm in gaps of at most 2.2: 5 points, and the new end.
        let all = app.session.scene.supports();
        assert_eq!(all.len(), 9 + 6);
        assert!(all.iter().any(|s| (s.tip - Vec3::new(10.0, 2.0, 10.0)).len() < 1e-3 && s.parent.is_none()), "{all:?}");

        // Nothing selected: the line starts from the newest tip, where the last one ended.
        let newest = all.iter().max_by_key(|s| s.id).map(|s| (s.id, s.tip)).unwrap();
        assert!((newest.1 - Vec3::new(10.0, 2.0, 10.0)).len() < 1e-3, "{newest:?}");
        app.run("support.select", json!({ "ids": [] }));
        frame(&mut app, key(egui::Key::L, Modifiers::NONE));
        assert_eq!(app.line.tool.as_ref().map(|t| (t.from, t.to)), Some((newest.0, None)));
        frame(&mut app, key(egui::Key::Escape, Modifiers::NONE));

        // No tip at all: nothing to start from.
        let ids: Vec<u64> = app.session.scene.supports().iter().map(|s| s.id).collect();
        app.run("support.delete", json!({ "ids": ids })).unwrap();
        app.status = None;
        frame(&mut app, key(egui::Key::L, Modifiers::NONE));
        assert!(app.line.tool.is_none());
        assert_eq!(app.status.as_ref().map(|s| s.1), Some(true), "{:?}", app.status);
    }
}
