//! Placing support tips by hand, in Support mode: the pointer over a model shows where a tip
//! would go, a left press places it, a left press on a placed tip or base selects the support, one
//! on a body segment or a joint's handle selects that part (Shift toggles), Delete removes the
//! selected supports. Selected joints and segments carry the Move arrows, always there in Support
//! edition: a drag places their joints (`support.place_joints`), and the support keeps that path
//! from then on. Every change goes through the `support.*` commands.

use egui::Pos2;
use resin_engine::SupportPart;
use resin_engine::geom::Vec3;
use resin_engine::render::Camera;
use serde_json::{Value, json};

use crate::ResinApp;
use crate::gizmo::{Change, Drag, Triad};
use crate::navigation::Proj;
use crate::support_routes::Piece;

/// What is under the pointer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Hover {
    /// A model's surface, where a tip would go.
    Surface { model: u64, point: Vec3, normal: Vec3 },
    /// A placed support's tip or base: the support as a whole.
    Support(u64),
    /// A joint's handle or a body segment.
    Part(u64, SupportPart),
}

/// The pointer's pick, kept until the pointer, the camera or the scene changes: a ray cast per
/// frame while nothing moves would be wasted.
#[derive(Default)]
pub struct SupportTool {
    pub hover: Option<Hover>,
    /// The pointer, the view, the scene revision and the routes' changes the pick was made at.
    key: Option<(Pos2, Camera, egui::Rect, u64, u64)>,
    /// While the arrows are dragged: each joint they move and where it was when the drag began.
    drag: Option<Vec<(u64, u32, Vec3)>>,
}

impl SupportTool {
    /// Forget the pick, as when leaving Support mode.
    /// Not the drag: this runs every frame a button is held, and a drag's first frame always
    /// takes its start afresh.
    pub fn clear(&mut self) {
        self.hover = None;
        self.key = None;
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
    match pointer {
        Some(p) => {
            let key = Some((p, proj.cam, proj.rect, app.session.scene.revision(), app.routes.changes()));
            if app.supports.key != key {
                app.supports.hover = pick(app, proj, p);
                app.supports.key = key;
            }
        }
        None => app.supports.clear(),
    }
    if matches!(app.supports.hover, Some(Hover::Support(_) | Hover::Part(..))) {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    let Some((at, mods)) = picking_press(app, ui, resp) else { return };
    let mode = if mods.shift { "toggle" } else { "replace" };
    let scene = &app.session.scene;
    let any_selected = !scene.selected.is_empty() || !scene.selected_parts.is_empty();
    match pick(app, proj, at) {
        Some(Hover::Surface { model, point, normal }) => {
            if !mods.shift && any_selected {
                app.run("support.select", json!({ "ids": [] }));
            }
            app.run("support.add", json!({ "model": model, "point": point, "normal": normal }));
        }
        Some(Hover::Support(id)) => {
            app.run("support.select", json!({ "ids": [id], "mode": mode }));
        }
        Some(Hover::Part(id, part)) => {
            app.run("support.select", json!({ "parts": [part_param(id, part)], "mode": mode }));
        }
        None if !mods.shift && any_selected => {
            app.run("support.select", json!({ "ids": [] }));
        }
        None => {}
    }
    app.supports.key = None;
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

/// The radius a tip is picked in and previewed with: its widest part.
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

/// A selected joint's handle wins over everything: it is drawn over everything. Otherwise the nearest of a
/// support's solids and the models' surface; a support wins when the ray reaches it first, or
/// within the tip's radius past the surface (the tip is half buried).
fn pick(app: &ResinApp, proj: &Proj, p: Pos2) -> Option<Hover> {
    let (o, d) = proj.ray(p);
    let scene = &app.session.scene;
    let preset = app.session.support_settings().current();
    if let Some((id, joint, _)) = app.routes.joint_hit(scene, preset, o, d) {
        return Some(Hover::Part(id, SupportPart::Joint(joint)));
    }
    let surface = scene.ray_hit(o, d);
    let radius = tip_radius(app);
    let tip = scene.support_hit(o, d, radius).map(|(id, t)| (Hover::Support(id), t));
    let piece = app.routes.piece_hit(scene, preset, o, d).map(|(id, piece, t)| match piece {
        Piece::Whole => (Hover::Support(id), t),
        Piece::Segment(i) => (Hover::Part(id, SupportPart::Segment(i)), t),
        Piece::Joint(i) => (Hover::Part(id, SupportPart::Joint(i)), t),
    });
    let support = [tip, piece].into_iter().flatten().min_by(|a, b| a.1.total_cmp(&b.1));
    match (surface, support) {
        (Some((_, h)), Some((hover, t))) if t <= h.t + radius => Some(hover),
        (None, Some((hover, _))) => Some(hover),
        (Some((model, h)), _) => {
            // In the scene, as `support.add` takes it: the mesh's own normal is off for a turned model.
            let normal = scene.model(model).and_then(|m| m.face_normal(h.triangle)).unwrap_or(-d);
            Some(Hover::Surface { model, point: h.point, normal })
        }
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
}
