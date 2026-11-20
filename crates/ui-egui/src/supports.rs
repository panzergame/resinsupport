//! Placing support tips by hand, in Support mode: the pointer over a model shows where a tip
//! would go, a left press places it, a left press on a placed tip selects the support, one
//! on a body segment or a joint's handle selects that part (the base goes with the segment
//! above it), one on a brace's strut the brace
//! (Shift toggles), Delete removes the selected supports and braces; a drag from empty space selects with a box (`box_select`). A double press
//! on a tip starts placing branches from that support's head: each press on a model's surface
//! places a branch's tip (`support.add` with its `parent`) until Esc. A
//! selected support shows a handle on its tip's contact: a press on it hides the tip, and the next
//! press on a model's surface puts the tip there (`support.move_tip`), any other press or Esc
//! puts it back. Selected joints and segments carry the Move arrows, always there in Support
//! edition: a drag places their joints (`support.place_joints`), and the support keeps that path
//! from then on. While a tip line is out (`support_line`), a press only places its other end.
//! While X is held, the supports are drawn see-through in a lens round the pointer and nothing
//! but the models' surface is picked: a press always places a tip.
//! Every change goes through the `support.*` commands.

use egui::Pos2;
use resin_engine::SupportPart;
use resin_engine::geom::Vec3;
use resin_engine::render::Camera;
use serde_json::{Value, json};

use crate::gizmo::{Change, Drag, Triad};
use crate::navigation::Proj;
use crate::support_routes::Piece;
use crate::{ActiveTool, ResinApp};

/// What is under the pointer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Hover {
    /// A model's surface, where a tip would go.
    Surface { model: u64, point: Vec3, normal: Vec3 },
    /// A placed support's tip: the support as a whole.
    Support(u64),
    /// A joint's handle or a body segment, the base standing for the segment above it.
    Part(u64, SupportPart),
    /// A selected support's contact handle, which starts moving its tip.
    TipHandle(u64),
    /// A brace's strut: the brace as a whole.
    Brace(u64),
}

/// The pointer's pick, kept until the pointer, the camera or the scene changes: a ray cast per
/// frame while nothing moves would be wasted.
#[derive(Default)]
pub struct SupportTool {
    pub hover: Option<Hover>,
    /// The pointer, the view, the scene revision, the routes' changes and `see_through` the pick
    /// was made at.
    key: Option<(Pos2, Camera, egui::Rect, u64, u64, bool)>,
    /// X is held over the view: only the surface is picked.
    pub see_through: bool,
    /// The pointer while `see_through`: the supports are drawn see-through round it.
    pub lens: Option<Pos2>,
    /// While the arrows are dragged: each joint they move and where it was when the drag began.
    drag: Option<Vec<(u64, u32, Vec3)>>,
    /// The last press on a tip: when (egui's clock) and on which support, the first of a double press.
    last_tip_press: Option<(f64, u64)>,
}

/// The support branches are being placed from, until Esc.
pub(crate) fn branching(app: &ResinApp) -> Option<u64> {
    match app.tool {
        Some(ActiveTool::Branch(id)) => Some(id),
        _ => None,
    }
}

/// The support whose tip is being moved.
pub(crate) fn moving(app: &ResinApp) -> Option<u64> {
    match app.tool {
        Some(ActiveTool::MoveTip(id)) => Some(id),
        _ => None,
    }
}

impl SupportTool {
    /// Forget the pick, as when leaving Support mode.
    /// Not the drag: this runs every frame a button is held, and a drag's first frame always
    /// takes its start afresh.
    pub fn clear(&mut self) {
        self.hover = None;
        self.key = None;
        self.see_through = false;
        self.lens = None;
    }
}

/// The joints the arrows move and where they are: each selected joint, and both ends of each
/// selected segment that has a joint there (the last one ends on the foot, which follows).
fn moved_joints(app: &ResinApp) -> Vec<(u64, u32, Vec3)> {
    let mut out: Vec<(u64, u32, Vec3)> = Vec::new();
    for &(id, part) in &app.session.scene.selected_parts {
        let Some(r) = app.routes.route(id) else { continue };
        let joints = match part {
            SupportPart::Joint(i) => [Some(i), None],
            SupportPart::Segment(i) => [Some(i), i.checked_add(1)],
        };
        for i in joints.into_iter().flatten() {
            // Only joints: every corner of the body but the foot.
            let at = r.body.split_last().and_then(|(_, corners)| corners.get(i as usize));
            if let Some(&at) = at
                && !out.iter().any(|(s, j, _)| (*s, *j) == (id, i))
            {
                out.push((id, i, at));
            }
        }
    }
    out
}

/// The Move arrows, at the centre of the joints they move, while there are any.
pub fn triad(app: &ResinApp) -> Option<Triad> {
    let joints = moved_joints(app);
    let n = joints.len();
    (n > 0).then(|| {
        let sum = joints.iter().fold(Vec3::ZERO, |acc, (.., p)| acc + *p);
        Triad { center: sum / n as f64, translate: true, rotate: false, scale: None }
    })
}

/// Apply an arrow drag: from where the joints were when it began, every frame.
pub fn drag(app: &mut ResinApp, g: Drag) {
    if g.started || app.supports.drag.is_none() {
        app.supports.drag = Some(moved_joints(app));
    }
    let Some(start) = app.supports.drag.clone() else { return };
    if g.done {
        app.supports.drag = None;
    }
    let Change::Translate(by) = g.change else { return };
    let joints: Vec<Value> = start
        .iter()
        .map(|&(id, joint, p)| {
            let q = p + by;
            json!({ "support": id, "joint": joint, "position": Vec3::new(q.x, q.y, q.z.max(0.0)) })
        })
        .collect();
    // The app closes the step once the button is up (`ResinApp::ui`).
    app.session.begin_step();
    app.run("support.place_joints", json!({ "joints": joints }));
}

/// The pick and the clicks for one frame over the viewport. Runs after `navigation::handle`, so
/// a navigating press is known.
pub fn handle(app: &mut ResinApp, ui: &egui::Ui, proj: &Proj, resp: &egui::Response) {
    // A held right button may still become a click: the preview stays until it orbits.
    let busy = ui.input(|i| i.pointer.primary_down() || i.pointer.middle_down() || i.pointer.is_decidedly_dragging() || i.key_down(egui::Key::Space));
    let pointer = resp.hover_pos().filter(|_| !busy && ui.is_enabled());
    let see_through = ui.is_enabled() && !ui.ctx().text_edit_focused() && ui.input(|i| i.key_down(egui::Key::X));
    app.supports.see_through = see_through;
    match pointer {
        Some(p) => {
            let key = Some((p, proj.cam, proj.rect, app.session.scene.revision(), app.routes.changes(), app.supports.see_through));
            if app.supports.key != key {
                app.supports.hover = pick(app, proj, p);
                app.supports.key = key;
            }
        }
        None => app.supports.clear(),
    }
    // Not filtered as the pick is: the lens stays through the press that places a tip.
    app.supports.see_through = see_through;
    app.supports.lens = resp.hover_pos().filter(|_| see_through);
    if matches!(app.supports.hover, Some(Hover::Support(_) | Hover::Part(..) | Hover::TipHandle(_) | Hover::Brace(_))) {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    let known = |id: u64| app.session.scene.supports().iter().any(|s| s.id == id);
    if branching(app).or(moving(app)).is_some_and(|id| !known(id)) {
        app.tool = None;
    }
    let Some((at, mods)) = picking_press(app, ui, resp) else { return };
    let hit = pick(app, proj, at);
    app.supports.key = None;
    // While a line is out, a press only places its other end.
    if crate::support_line::tool(app).is_some() {
        crate::support_line::press(app, hit);
        return;
    }
    if let Some(parent) = branching(app) {
        if let Some(Hover::Surface { model, point, normal }) = hit {
            app.run("support.add", json!({ "model": model, "point": point, "normal": normal, "parent": parent }));
        }
        return;
    }
    let now = ui.input(|i| i.time);
    let delay = ui.ctx().options(|o| o.input_options.max_double_click_delay);
    // On the press, not egui's double click, which comes on the second release. Both presses are
    // tested against the tip, not only the pick: once the support is selected, its head's handle
    // is drawn over the tip.
    if let Some((t, id)) = app.supports.last_tip_press.take()
        && !app.supports.see_through
        && now - t <= delay
        && on_tip(app, proj, at, id)
    {
        // The first press of a double on a selected tip landed on its contact handle.
        app.tool = Some(ActiveTool::Branch(id));
        return;
    }
    if let Some(id) = moving(app) {
        app.tool = None;
        if let Some(Hover::Surface { model, point, normal }) = hit {
            app.run("support.move_tip", json!({ "support": id, "model": model, "point": point, "normal": normal }));
        }
        return;
    }
    if let Some(Hover::Support(id) | Hover::Part(id, _) | Hover::TipHandle(id)) = hit
        && on_tip(app, proj, at, id)
    {
        app.supports.last_tip_press = Some((now, id));
    }
    let mode = if mods.shift { "toggle" } else { "replace" };
    let scene = &app.session.scene;
    let any_selected = !scene.selected.is_empty() || !scene.selected_parts.is_empty() || !scene.selected_braces.is_empty();
    match hit {
        Some(Hover::Surface { model, point, normal }) => {
            if !mods.shift && any_selected {
                app.run("support.select", json!({ "ids": [] }));
            }
            app.run("support.add", json!({ "model": model, "point": point, "normal": normal }));
        }
        Some(Hover::TipHandle(id)) => {
            app.tool = Some(ActiveTool::MoveTip(id));
        }
        Some(Hover::Support(id)) => {
            app.run("support.select", json!({ "ids": [id], "mode": mode }));
        }
        Some(Hover::Part(id, part)) => {
            app.run("support.select", json!({ "parts": [part_param(id, part)], "mode": mode }));
        }
        Some(Hover::Brace(id)) => {
            app.run("support.select", json!({ "braces": [id], "mode": mode }));
        }
        None => {
            if !mods.shift && any_selected {
                app.run("support.select", json!({ "ids": [] }));
            }
            crate::box_select::press(app, at, mods);
        }
    }
}

/// Whether the ray through `at` meets support `id`'s tip, not its base.
fn on_tip(app: &ResinApp, proj: &Proj, at: Pos2, id: u64) -> bool {
    let (o, d) = proj.ray(at);
    let radius = tip_radius(app);
    let at_contact = app.session.scene.supports().iter().any(|s| s.id == id && resin_engine::geom::ray_sphere(o, d, s.tip, radius).is_some());
    at_contact || app.routes.route(id).is_some_and(|r| resin_engine::geom::ray_capsule(o, d, r.contact, r.head, radius).is_some())
}

/// While a branch is being placed or a tip moved, over a model: the tip, from where it would
/// touch to its head. A moved tip with a worked-out route takes its head along.
pub fn tip_preview(app: &ResinApp) -> Option<(Vec3, Vec3)> {
    let Some(Hover::Surface { point, normal, .. }) = app.supports.hover else { return None };
    let scene = &app.session.scene;
    let preset = app.session.support_settings().current();
    if let Some(parent) = branching(app) {
        let s = scene.supports().iter().find(|s| s.id == parent)?;
        return Some((point, resin_engine::support_head(scene, s, preset)?));
    }
    let mut s = scene.supports().iter().find(|s| Some(s.id) == moving(app))?.clone();
    (s.tip, s.normal) = (point, normal);
    Some((point, resin_engine::support_head(scene, &s, preset)?))
}

/// The contact handles: on each selected support's tip, in Support edition, while no tip is being
/// placed, the size of the contact sphere. Each one's support, centre and radius.
pub fn tip_handles(app: &ResinApp) -> Vec<(u64, Vec3, f64)> {
    if app.mode != crate::EditMode::Support || matches!(app.tool, Some(ActiveTool::Branch(_) | ActiveTool::MoveTip(_) | ActiveTool::Line(_))) {
        return Vec::new();
    }
    let radius = app.session.support_settings().current().tip.contact_diameter / 2.0;
    let scene = &app.session.scene;
    scene.supports().iter().filter(|s| scene.selected.contains(&s.id)).map(|s| (s.id, s.tip, radius)).collect()
}

/// A left press over the viewport that picks rather than navigates: where it happened and the
/// modifiers held. Acting on the press rather than egui's click: a hand still moving when it
/// clicks travels past `max_click_dist` before the release, and egui then calls it a drag, so
/// the click was lost. The press is picked where it happened, not where the last hover pick was.
pub(crate) fn picking_press(app: &ResinApp, ui: &egui::Ui, resp: &egui::Response) -> Option<(Pos2, egui::Modifiers)> {
    let (pressed_at, mods, space) =
        ui.input(|i| (i.pointer.press_origin().filter(|_| i.pointer.primary_pressed()), i.modifiers, i.key_down(egui::Key::Space)));
    let at = pressed_at.filter(|_| resp.contains_pointer() && ui.is_enabled() && !app.gizmo_grab)?;
    let navigates = app.nav.primary_navigates() || space || mods.ctrl || mods.command || mods.alt;
    (!navigates).then_some((at, mods))
}

/// The view's angle below the plate's plane where the supports' bodies start to fade, and how
/// opaque they must be to be picked.
const BODY_FADE_START: f64 = std::f64::consts::FRAC_PI_6;
const BODY_PICK_ALPHA: f32 = 0.5;

/// How opaque the supports' bodies, bases and braces are in Support edition: opaque down to
/// `BODY_FADE_START` below the plate, then fading with the square of the angle left to straight
/// up, where they are gone. They stand between a view from below and the surface tips go on.
pub(crate) fn body_alpha(app: &ResinApp) -> f32 {
    if app.mode != crate::EditMode::Support || !app.prefs.fade_bodies_from_below {
        return 1.0;
    }
    body_alpha_at(app.session.camera.back().z)
}

/// `body_alpha` for an eye in unit direction `back` from the target, by its Z.
fn body_alpha_at(back_z: f64) -> f32 {
    let below = (-back_z).clamp(-1.0, 1.0).asin();
    let t = ((below - BODY_FADE_START) / (std::f64::consts::FRAC_PI_2 - BODY_FADE_START)).clamp(0.0, 1.0);
    ((1.0 - t) * (1.0 - t)) as f32
}

/// The radius a tip is picked in: its widest part.
pub(crate) fn tip_radius(app: &ResinApp) -> f64 {
    let tip = app.session.support_settings().current().tip;
    tip.upper_diameter.max(tip.contact_diameter) / 2.0
}

fn part_param(id: u64, part: SupportPart) -> Value {
    match part {
        SupportPart::Joint(i) => json!({ "support": id, "joint": i }),
        SupportPart::Segment(i) => json!({ "support": id, "segment": i }),
    }
}

/// While a tip is moved, the surface alone. Otherwise a contact handle, then a selected joint's
/// handle, win over everything: they are drawn over everything. Otherwise the nearest of a
/// support's solids and the models' surface; a support wins when the ray reaches it first, or
/// within the tip's radius past the surface (the tip is half buried).
fn pick(app: &ResinApp, proj: &Proj, p: Pos2) -> Option<Hover> {
    // A line between two tips takes no pick; one waiting for its other end takes surfaces alone.
    if crate::support_line::tool(app).is_some() && !crate::support_line::placing(app) {
        return None;
    }
    let (o, d) = proj.ray(p);
    let scene = &app.session.scene;
    let preset = app.session.support_settings().current();
    let surface = scene.ray_hit(o, d);
    let on_surface = |(model, h): (u64, resin_engine::geom::RayHit)| {
        // In the scene, as `support.add` takes it: the mesh's own normal is off for a turned model.
        let normal = scene.model(model).and_then(|m| m.face_normal(h.triangle)).unwrap_or(-d);
        Hover::Surface { model, point: h.point, normal }
    };
    // Its own hidden tip must not stand in the way; nor may a tip under a line's other end, nor
    // any support while seeing through them.
    if moving(app).is_some() || crate::support_line::placing(app) || app.supports.see_through {
        return surface.map(on_surface);
    }
    let handle = tip_handles(app).into_iter().filter_map(|(id, c, r)| resin_engine::geom::ray_sphere(o, d, c, r).map(|t| (id, t)));
    if let Some((id, _)) = handle.min_by(|a, b| a.1.total_cmp(&b.1)) {
        return Some(Hover::TipHandle(id));
    }
    if let Some((id, joint, _)) = app.routes.joint_hit(scene, preset, o, d) {
        return Some(Hover::Part(id, SupportPart::Joint(joint)));
    }
    let radius = tip_radius(app);
    let tip = scene.support_hit(o, d, radius).map(|(id, t)| (Hover::Support(id), t));
    // Bodies faded this far take no pick: a press goes through them to the surface.
    let bodies = body_alpha(app) >= BODY_PICK_ALPHA;
    let piece = app.routes.piece_hit(scene, preset, o, d, bodies).map(|(id, piece, t)| match piece {
        Piece::Whole => (Hover::Support(id), t),
        Piece::Segment(i) => (Hover::Part(id, SupportPart::Segment(i)), t),
        Piece::Joint(i) => (Hover::Part(id, SupportPart::Joint(i)), t),
    });
    let brace = app.routes.brace_hit(scene, preset, o, d).filter(|_| bodies).map(|(id, t)| (Hover::Brace(id), t));
    let support = [tip, piece, brace].into_iter().flatten().min_by(|a, b| a.1.total_cmp(&b.1));
    match (surface, support) {
        (Some((_, h)), Some((hover, t))) if t <= h.t + radius => Some(hover),
        (None, Some((hover, _))) => Some(hover),
        (Some(hit), _) => Some(on_surface(hit)),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gizmo::Handle;
    use crate::prefs::Prefs;

    #[test]
    fn the_arrows_move_selected_joints_from_where_the_drag_began() {
        let dir = std::env::temp_dir().join(format!("resin-ui-joints-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("plate.stl");
        // A floating square plate, facing down.
        let text = "solid p\nfacet normal 0 0 0\nouter loop\nvertex 0 0 10\nvertex 0 20 10\nvertex 20 20 10\nendloop\nendfacet\n\
                    facet normal 0 0 0\nouter loop\nvertex 0 0 10\nvertex 20 20 10\nvertex 20 0 10\nendloop\nendfacet\nendsolid p\n";
        std::fs::write(&path, text).unwrap();
        let mut app = ResinApp::build(Prefs::default(), &[]);
        let m = app.run("file.import_stl", json!({ "path": path, "fit": false })).unwrap()["id"].as_u64().unwrap();
        std::fs::remove_dir_all(dir).ok();
        let id = app.run("support.add", json!({ "model": m, "point": [5, 5, 10], "normal": [0, 0, -1] })).unwrap()["id"].as_u64().unwrap();
        let preset = *app.session.support_settings().current();
        while app.routes.update(&app.session.scene, &preset) {}
        let head = app.routes.route(id).unwrap().head;

        assert!(triad(&app).is_none(), "nothing selected, no arrows");
        // A straight support's one segment ends on the foot: only its head joint moves.
        app.run("support.select", json!({ "parts": [{ "support": id, "segment": 0 }] }));
        assert_eq!(moved_joints(&app), vec![(id, 0, head)]);
        assert_eq!(triad(&app).map(|t| t.center), Some(head));

        let depth = app.session.undo_depth();
        let by = Vec3::new(1.0, 0.0, -2.0);
        for (k, started, done) in [(1.0, true, false), (2.0, false, true)] {
            drag(&mut app, Drag { handle: Handle::Ball, change: Change::Translate(by * k), started, done });
            while app.routes.update(&app.session.scene, &preset) {}
        }
        app.session.end_step();
        assert_eq!(app.session.scene.supports()[0].corners, Some(vec![head + by * 2.0]), "measured from the start, not piled up");
        assert_eq!(app.session.undo_depth(), depth + 1, "one drag, one step");
        assert_eq!(triad(&app).map(|t| t.center), Some(head + by * 2.0));
    }

    /// A double press on a tip, then each press on the model, places a branch of that support in
    /// one step each, until Esc; a slow second press is no double, a press off the model or an
    /// undo keeps placing.
    #[test]
    fn a_double_press_on_a_tip_places_branches_until_esc() {
        let dir = std::env::temp_dir().join(format!("resin-ui-branch-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("plate.stl");
        // A floating square plate, facing down.
        let text = "solid p\nfacet normal 0 0 0\nouter loop\nvertex 0 0 10\nvertex 0 20 10\nvertex 20 20 10\nendloop\nendfacet\n\
                    facet normal 0 0 0\nouter loop\nvertex 0 0 10\nvertex 20 20 10\nvertex 20 0 10\nendloop\nendfacet\nendsolid p\n";
        std::fs::write(&path, text).unwrap();
        let mut app = ResinApp::build(Prefs::default(), &[]);
        let m = app.run("file.import_stl", json!({ "path": path, "fit": false })).unwrap()["id"].as_u64().unwrap();
        std::fs::remove_dir_all(dir).ok();
        let id = app.run("support.add", json!({ "model": m, "point": [5, 5, 10], "normal": [0, 0, -1] })).unwrap()["id"].as_u64().unwrap();
        let preset = *app.session.support_settings().current();
        while app.routes.update(&app.session.scene, &preset) {}
        app.mode = crate::EditMode::Support;
        app.run("view.standard", json!({ "view": "bottom" })).unwrap();
        app.run("view.fit", json!({ "aspect": 800.0 / 600.0 })).unwrap();
        let rect = egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0));
        let proj = Proj { cam: app.session.camera, rect };
        let ctx = egui::Context::default();
        let at = |p: [f64; 3]| proj.to_screen(Vec3::new(p[0], p[1], p[2])).unwrap();
        let (tip, surface, away) = (at([5.0, 5.0, 10.0]), at([15.0, 15.0, 10.0]), rect.left_top() + egui::vec2(5.0, 5.0));
        let press = |app: &mut ResinApp, pos: Pos2, time: f64| {
            for (pressed, t) in [(true, time), (false, time + 0.05)] {
                let button = egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
                let raw = egui::RawInput {
                    screen_rect: Some(rect),
                    time: Some(t),
                    events: vec![egui::Event::PointerMoved(pos), button],
                    ..Default::default()
                };
                let mut out = ctx.run_ui(raw, |ui| {
                    let resp = ui.allocate_rect(rect, egui::Sense::click_and_drag());
                    handle(app, ui, &proj, &resp);
                });
                out.textures_delta.clear();
            }
        };

        press(&mut app, tip, 0.0);
        press(&mut app, tip, 1.0);
        assert_eq!(branching(&app), None, "a second press a second later is no double");
        assert!(app.session.scene.selected.contains(&id));
        press(&mut app, tip, 1.15);
        assert_eq!(branching(&app), Some(id));
        assert_eq!(tip_preview(&app), None, "nothing over the model yet");
        let depth = app.session.undo_depth();
        press(&mut app, surface, 2.0);
        let all = app.session.scene.supports();
        assert_eq!(all.len(), 2);
        assert_eq!(all[1].parent, Some(id));
        assert!((all[1].tip - Vec3::new(15.0, 15.0, 10.0)).len() < 1e-6, "{:?}", all[1].tip);
        assert_eq!(app.session.undo_depth(), depth + 1, "one press, one step");
        assert_eq!(branching(&app), Some(id), "still placing after a branch");

        press(&mut app, at([15.0, 5.0, 10.0]), 3.0);
        let all = app.session.scene.supports();
        assert_eq!((all.len(), all[2].parent), (3, Some(id)));
        assert_eq!(app.session.undo_depth(), depth + 2);

        // A press off the model places nothing and keeps placing; so does an undo.
        press(&mut app, away, 4.0);
        assert_eq!((branching(&app), app.session.scene.supports().len(), app.session.undo_depth()), (Some(id), 3, depth + 2));
        app.step_history(false);
        assert_eq!((branching(&app), app.session.scene.supports().len()), (Some(id), 2));

        // Esc puts it away, leaving the history alone.
        let depth = app.session.undo_depth();
        let raw = egui::RawInput {
            screen_rect: Some(rect),
            time: Some(5.0),
            events: vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            ..Default::default()
        };
        let mut out = ctx.run_ui(raw, |ui| app.keys(ui.ctx()));
        out.textures_delta.clear();
        assert_eq!((branching(&app), app.session.undo_depth()), (None, depth));
    }

    /// While X is held, the lens follows the pointer and a support under it takes no pick: a press
    /// on its tip places a tip on the model behind it. Let go, the support is picked again.
    #[test]
    fn holding_x_sees_through_supports_and_places_tips_behind_them() {
        let dir = std::env::temp_dir().join(format!("resin-ui-see-through-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("plate.stl");
        // A floating square plate, facing down.
        let text = "solid p\nfacet normal 0 0 0\nouter loop\nvertex 0 0 10\nvertex 0 20 10\nvertex 20 20 10\nendloop\nendfacet\n\
                    facet normal 0 0 0\nouter loop\nvertex 0 0 10\nvertex 20 20 10\nvertex 20 0 10\nendloop\nendfacet\nendsolid p\n";
        std::fs::write(&path, text).unwrap();
        let mut app = ResinApp::build(Prefs::default(), &[]);
        let m = app.run("file.import_stl", json!({ "path": path, "fit": false })).unwrap()["id"].as_u64().unwrap();
        std::fs::remove_dir_all(dir).ok();
        let id = app.run("support.add", json!({ "model": m, "point": [5, 5, 10], "normal": [0, 0, -1] })).unwrap()["id"].as_u64().unwrap();
        let preset = *app.session.support_settings().current();
        while app.routes.update(&app.session.scene, &preset) {}
        app.mode = crate::EditMode::Support;
        app.run("view.standard", json!({ "view": "bottom" })).unwrap();
        app.run("view.fit", json!({ "aspect": 800.0 / 600.0 })).unwrap();
        let rect = egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0));
        let proj = Proj { cam: app.session.camera, rect };
        let ctx = egui::Context::default();
        let tip = proj.to_screen(Vec3::new(5.0, 5.0, 10.0)).unwrap();
        let x = |pressed: bool| egui::Event::Key { key: egui::Key::X, physical_key: None, pressed, repeat: false, modifiers: egui::Modifiers::NONE };
        let button =
            |pressed: bool| egui::Event::PointerButton { pos: tip, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
        let frame = |app: &mut ResinApp, events: Vec<egui::Event>, time: f64| {
            let raw = egui::RawInput { screen_rect: Some(rect), time: Some(time), events, ..Default::default() };
            let mut out = ctx.run_ui(raw, |ui| {
                let resp = ui.allocate_rect(rect, egui::Sense::click_and_drag());
                handle(app, ui, &proj, &resp);
            });
            out.textures_delta.clear();
        };

        // egui knows the pointer is over the view from the frame before: bring it there first.
        frame(&mut app, vec![egui::Event::PointerMoved(tip)], 0.0);
        frame(&mut app, vec![egui::Event::PointerMoved(tip)], 0.1);
        assert_eq!(app.supports.hover, Some(Hover::Support(id)));
        assert_eq!(app.supports.lens, None);

        frame(&mut app, vec![x(true)], 0.2);
        assert!(matches!(app.supports.hover, Some(Hover::Surface { .. })), "{:?}", app.supports.hover);
        assert_eq!(app.supports.lens, Some(tip));
        let depth = app.session.undo_depth();
        frame(&mut app, vec![button(true)], 0.3);
        assert_eq!(app.supports.lens, Some(tip), "the press keeps the lens");
        frame(&mut app, vec![button(false)], 0.35);
        assert_eq!((app.session.scene.supports().len(), app.session.undo_depth()), (2, depth + 1), "a tip placed behind the support");
        assert!(app.session.scene.selected.is_empty(), "nothing selected");

        frame(&mut app, vec![x(false)], 0.4);
        assert!(!app.supports.see_through && app.supports.lens.is_none());
        assert!(matches!(app.supports.hover, Some(Hover::Support(_))), "{:?}", app.supports.hover);
    }

    /// Looking up at the supports, their bodies fade, from 30° below the plate to gone straight up,
    /// and a press on one reaches the surface behind it; its tip is still picked. With the
    /// preference off, the body is picked as before.
    #[test]
    fn bodies_seen_from_below_fade_and_take_no_pick() {
        let near = |a: f32, b: f32| (a - b).abs() < 1e-6;
        assert!(near(body_alpha_at(1.0), 1.0) && near(body_alpha_at(0.0), 1.0), "from above and level");
        assert!(near(body_alpha_at(-0.5), 1.0), "30° below");
        assert!(near(body_alpha_at(-60f64.to_radians().sin()), 0.25), "halfway: a quarter");
        assert!(near(body_alpha_at(-1.0), 0.0), "straight up");

        let dir = std::env::temp_dir().join(format!("resin-ui-fade-bodies-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("plate.stl");
        // A floating square plate, facing down.
        let text = "solid p\nfacet normal 0 0 0\nouter loop\nvertex 0 0 10\nvertex 0 20 10\nvertex 20 20 10\nendloop\nendfacet\n\
                    facet normal 0 0 0\nouter loop\nvertex 0 0 10\nvertex 20 20 10\nvertex 20 0 10\nendloop\nendfacet\nendsolid p\n";
        std::fs::write(&path, text).unwrap();
        let mut app = ResinApp::build(Prefs::default(), &[]);
        let m = app.run("file.import_stl", json!({ "path": path, "fit": false })).unwrap()["id"].as_u64().unwrap();
        std::fs::remove_dir_all(dir).ok();
        let id = app.run("support.add", json!({ "model": m, "point": [10, 10, 10], "normal": [0, 0, -1] })).unwrap()["id"].as_u64().unwrap();
        let preset = *app.session.support_settings().current();
        while app.routes.update(&app.session.scene, &preset) {}
        app.mode = crate::EditMode::Support;
        app.run("view.standard", json!({ "view": "bottom" })).unwrap();
        app.run("view.fit", json!({ "aspect": 800.0 / 600.0 })).unwrap();
        // 60° below the plate: the bodies a quarter opaque.
        app.session.camera.pitch = -60f64.to_radians();
        assert!(near(body_alpha(&app), 0.25));
        let proj = Proj { cam: app.session.camera, rect: egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0)) };
        let column = proj.to_screen(Vec3::new(10.0, 10.0, 7.0)).unwrap();
        let tip = proj.to_screen(Vec3::new(10.0, 10.0, 10.0)).unwrap();

        assert!(matches!(pick(&app, &proj, column), Some(Hover::Surface { .. })), "{:?}", pick(&app, &proj, column));
        assert_eq!(pick(&app, &proj, tip), Some(Hover::Support(id)));
        // Not in Object edition, nor with the preference off.
        app.mode = crate::EditMode::Object;
        assert_eq!(body_alpha(&app), 1.0);
        app.mode = crate::EditMode::Support;
        app.prefs.fade_bodies_from_below = false;
        assert_eq!(body_alpha(&app), 1.0);
        assert_eq!(pick(&app, &proj, column), Some(Hover::Part(id, SupportPart::Segment(0))));
        let base = proj.to_screen(Vec3::new(10.0, 10.0, 0.1)).unwrap();
        assert_eq!(pick(&app, &proj, base), Some(Hover::Part(id, SupportPart::Segment(0))), "the base picks the segment above it");
    }

    /// A press on a selected support's contact handle hides its tip; the next press on the model
    /// moves the tip there in one step, a press off it puts the tip back with no step. A double
    /// press on the selected tip still starts a branch.
    #[test]
    fn a_press_on_the_contact_handle_then_on_the_model_moves_the_tip() {
        let dir = std::env::temp_dir().join(format!("resin-ui-move-tip-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("plate.stl");
        // A floating square plate, facing down.
        let text = "solid p\nfacet normal 0 0 0\nouter loop\nvertex 0 0 10\nvertex 0 20 10\nvertex 20 20 10\nendloop\nendfacet\n\
                    facet normal 0 0 0\nouter loop\nvertex 0 0 10\nvertex 20 20 10\nvertex 20 0 10\nendloop\nendfacet\nendsolid p\n";
        std::fs::write(&path, text).unwrap();
        let mut app = ResinApp::build(Prefs::default(), &[]);
        let m = app.run("file.import_stl", json!({ "path": path, "fit": false })).unwrap()["id"].as_u64().unwrap();
        std::fs::remove_dir_all(dir).ok();
        let id = app.run("support.add", json!({ "model": m, "point": [5, 5, 10], "normal": [0, 0, -1] })).unwrap()["id"].as_u64().unwrap();
        let preset = *app.session.support_settings().current();
        while app.routes.update(&app.session.scene, &preset) {}
        app.mode = crate::EditMode::Support;
        app.run("view.standard", json!({ "view": "bottom" })).unwrap();
        app.run("view.fit", json!({ "aspect": 800.0 / 600.0 })).unwrap();
        let rect = egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0));
        let proj = Proj { cam: app.session.camera, rect };
        let ctx = egui::Context::default();
        let at = |p: [f64; 3]| proj.to_screen(Vec3::new(p[0], p[1], p[2])).unwrap();
        let (tip, surface, away) = (at([5.0, 5.0, 10.0]), at([15.0, 15.0, 10.0]), rect.left_top() + egui::vec2(5.0, 5.0));
        let press = |app: &mut ResinApp, pos: Pos2, time: f64| {
            for (pressed, t) in [(true, time), (false, time + 0.05)] {
                let button = egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
                let raw = egui::RawInput {
                    screen_rect: Some(rect),
                    time: Some(t),
                    events: vec![egui::Event::PointerMoved(pos), button],
                    ..Default::default()
                };
                let mut out = ctx.run_ui(raw, |ui| {
                    let resp = ui.allocate_rect(rect, egui::Sense::click_and_drag());
                    handle(app, ui, &proj, &resp);
                });
                out.textures_delta.clear();
            }
        };
        let tip_drawn = |app: &mut ResinApp| {
            let mesh = app.routes.mesh(&app.session.scene, &preset, crate::support_routes::Selection::Supports, moving(app));
            let contact = Vec3::from(mesh.vertices.iter().map(|v| v.pos).fold([0.0f32; 3], |a, p| if p[2] > a[2] { p } else { a }));
            (contact.z - 10.0).abs() < 1.0
        };

        assert!(tip_handles(&app).is_empty(), "nothing selected, no handle");
        // egui knows the pointer is over the view from the frame before: bring it there first.
        let raw = egui::RawInput { screen_rect: Some(rect), time: Some(0.0), events: vec![egui::Event::PointerMoved(tip)], ..Default::default() };
        let mut out = ctx.run_ui(raw, |ui| {
            ui.allocate_rect(rect, egui::Sense::click_and_drag());
        });
        out.textures_delta.clear();
        press(&mut app, tip, 0.5);
        assert_eq!(tip_handles(&app).iter().map(|h| (h.0, h.1)).collect::<Vec<_>>(), vec![(id, Vec3::new(5.0, 5.0, 10.0))]);
        press(&mut app, tip, 1.0);
        assert_eq!((moving(&app), branching(&app)), (Some(id), None), "a press on the handle, not a double");
        assert!(tip_handles(&app).is_empty() && !tip_drawn(&mut app), "the tip is hidden while it moves");
        let depth = app.session.undo_depth();
        press(&mut app, surface, 2.0);
        let s = &app.session.scene.supports()[0];
        assert!((s.tip - Vec3::new(15.0, 15.0, 10.0)).len() < 1e-6, "{:?}", s.tip);
        assert_eq!((moving(&app), app.session.undo_depth()), (None, depth + 1), "one press, one step");
        while app.routes.update(&app.session.scene, &preset) {}
        assert!(tip_drawn(&mut app));

        // Put back by a press off the model: the scene and the history untouched.
        press(&mut app, surface, 3.0);
        assert_eq!(moving(&app), Some(id));
        let (revision, depth) = (app.session.scene.revision(), app.session.undo_depth());
        press(&mut app, away, 4.0);
        assert_eq!((moving(&app), app.session.scene.revision(), app.session.undo_depth()), (None, revision, depth));

        // A double press on the selected tip: its first press lands on the handle, the second branches.
        app.run("support.select", json!({ "ids": [id] }));
        press(&mut app, surface, 5.0);
        press(&mut app, surface, 5.15);
        assert_eq!((moving(&app), branching(&app)), (None, Some(id)));
    }
}
