// `BoxSel` (window and crossing rules, without the lasso), `segment_hits_rect` and the box's
// painting are ported from SolidCraft (MIT OR Apache-2.0), crates/ui-egui/src/selection.rs and
// crates/ui-egui/src/viewport.rs (`paint_box_selection`) at 4bdffef0; see ATTRIBUTION.md. The
// drag and the model and support tests are ours.
//! Box selection, in Object and Support edition: a plain left drag from empty space draws a box.
//! Dragged rightwards it takes what lies wholly inside (window), leftwards what it touches
//! (crossing). Shift adds to the selection, else the box replaces it. Through `model.select`
//! and `support.select`.

use egui::{Pos2, Rect, Shape, Stroke};
use resin_engine::Model;
use resin_engine::geom::Vec3;
use serde_json::json;

use crate::navigation::Proj;
use crate::theme::Tokens;
use crate::{EditMode, ResinApp};

/// How far, in points, the pointer must go before a press becomes a box: egui's click distance,
/// so a click that wobbles still only clicks.
const MIN_DRAG: f32 = 6.0;

/// The box being dragged.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoxDrag {
    pub start: Pos2,
    pub end: Pos2,
    /// Shift was held at the press: the box adds to the selection.
    pub add: bool,
}

impl BoxDrag {
    fn sel(&self) -> Option<BoxSel> {
        (self.start.distance(self.end) >= MIN_DRAG).then(|| BoxSel::from_drag(self.start, self.end))
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoxSel {
    pub rect: Rect,
    /// Dragged leftwards: takes what it touches, not only what lies wholly inside.
    pub crossing: bool,
}

impl BoxSel {
    pub fn from_drag(start: Pos2, end: Pos2) -> BoxSel {
        BoxSel { rect: Rect::from_two_pos(start, end), crossing: end.x < start.x }
    }

    /// Does the polyline (screen points) count as selected?
    pub fn takes(&self, pts: &[Pos2]) -> bool {
        if pts.is_empty() {
            return false;
        }
        if !self.crossing {
            return pts.iter().all(|p| self.rect.contains(*p));
        }
        pts.iter().any(|p| self.rect.contains(*p)) || pts.windows(2).any(|w| segment_hits_rect(w[0], w[1], self.rect))
    }

    /// Does the screen triangle count as touched (crossing)? Its outline, or the box wholly
    /// inside it.
    fn touches_triangle(&self, t: [Pos2; 3]) -> bool {
        self.takes(&[t[0], t[1], t[2], t[0]]) || inside_triangle(self.rect.center(), t)
    }
}

pub fn segment_hits_rect(a: Pos2, b: Pos2, r: Rect) -> bool {
    if r.contains(a) || r.contains(b) {
        return true;
    }
    let c = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom()];
    (0..4).any(|i| segments_cross(a, b, c[i], c[(i + 1) % 4]))
}

fn segments_cross(p: Pos2, q: Pos2, r: Pos2, s: Pos2) -> bool {
    let o = |a: Pos2, b: Pos2, c: Pos2| (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
    let (d1, d2, d3, d4) = (o(r, s, p), o(r, s, q), o(p, q, r), o(p, q, s));
    (d1 > 0.0) != (d2 > 0.0) && (d3 > 0.0) != (d4 > 0.0)
}

fn inside_triangle(p: Pos2, t: [Pos2; 3]) -> bool {
    let o = |a: Pos2, b: Pos2| (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x);
    let (d1, d2, d3) = (o(t[0], t[1]), o(t[1], t[2]), o(t[2], t[0]));
    (d1 >= 0.0 && d2 >= 0.0 && d3 >= 0.0) || (d1 <= 0.0 && d2 <= 0.0 && d3 <= 0.0)
}

/// A plain press on empty space, from the mode's picking: a box may follow.
pub fn press(app: &mut ResinApp, at: Pos2, mods: egui::Modifiers) {
    app.boxsel = Some(BoxDrag { start: at, end: at, add: mods.shift });
}

/// Follow the pointer while the button is held; on its release, select what the box takes.
/// Runs every frame in Object and Support edition, after the mode's picking.
pub fn track(app: &mut ResinApp, ui: &egui::Ui, proj: &Proj) {
    let Some(mut drag) = app.boxsel else { return };
    let (down, pointer) = ui.input(|i| (i.pointer.primary_down(), i.pointer.latest_pos()));
    if let Some(p) = pointer {
        drag.end = p;
    }
    if down {
        app.boxsel = Some(drag);
        return;
    }
    app.boxsel = None;
    if let Some(bx) = drag.sel() {
        apply(app, proj, bx, drag.add);
    }
}

/// Select what `bx` takes in the current mode, added to the selection or in its place.
pub fn apply(app: &mut ResinApp, proj: &Proj, bx: BoxSel, add: bool) {
    let mode = if add { "add" } else { "replace" };
    match app.mode {
        EditMode::Object => {
            let ids: Vec<u64> = app.session.scene.models().iter().filter(|m| takes_model(&bx, proj, m)).map(|m| m.id).collect();
            app.run("model.select", json!({ "ids": ids, "mode": mode }));
        }
        EditMode::Support => {
            let ids: Vec<u64> = app
                .session
                .scene
                .supports()
                .iter()
                .filter(|s| {
                    // The tip and the body down to the foot; the contact alone until the route is in.
                    let mut pts = vec![s.tip];
                    if let Some(r) = app.routes.route(s.id) {
                        pts.push(r.head);
                        pts.extend(r.body.iter().skip(1));
                    }
                    let screen: Option<Vec<Pos2>> = pts.iter().map(|p| proj.to_screen(*p)).collect();
                    screen.is_some_and(|s| bx.takes(&s))
                })
                .map(|s| s.id)
                .collect();
            app.run("support.select", json!({ "ids": ids, "mode": mode }));
        }
        EditMode::Slice => {}
    }
}

/// Is model `m` wholly inside the box (window), or touched by it (crossing)? Its bounds' corners
/// settle most models; the rest walk the mesh on screen, exact for a turned model.
fn takes_model(bx: &BoxSel, proj: &Proj, m: &Model) -> bool {
    let (lo, hi) = (m.bounds.min, m.bounds.max);
    let corners: Option<Vec<Pos2>> = (0..8)
        .map(|i| {
            proj.to_screen(Vec3::new(
                if i & 1 == 0 { lo.x } else { hi.x },
                if i & 2 == 0 { lo.y } else { hi.y },
                if i & 4 == 0 { lo.z } else { hi.z },
            ))
        })
        .collect();
    if let Some(c) = &corners {
        if c.iter().all(|p| bx.rect.contains(*p)) {
            return true;
        }
        let around = c.iter().fold(Rect::NOTHING, |r, p| r.union(Rect::from_min_max(*p, *p)));
        if !around.intersects(bx.rect) {
            return false;
        }
    }
    let at = |v: [f32; 3]| proj.to_screen(m.transform.point(Vec3::from(v)));
    if bx.crossing {
        m.mesh.triangles.iter().any(|t| matches!((at(t[0]), at(t[1]), at(t[2])), (Some(a), Some(b), Some(c)) if bx.touches_triangle([a, b, c])))
    } else {
        m.mesh.triangles.iter().flatten().all(|v| at(*v).is_some_and(|p| bx.rect.contains(p)))
    }
}

/// The box while it is dragged: filled faintly, its outline solid for a window, dashed for a
/// crossing.
pub fn paint(app: &ResinApp, painter: &egui::Painter, tokens: &Tokens) {
    let Some(bx) = app.boxsel.and_then(|d| d.sel()) else { return };
    let col = if bx.crossing { tokens.box_crossing } else { tokens.box_window };
    painter.rect_filled(bx.rect, 0.0, col.gamma_multiply(0.12));
    if bx.crossing {
        let c = [bx.rect.left_top(), bx.rect.right_top(), bx.rect.right_bottom(), bx.rect.left_bottom()];
        for i in 0..4 {
            painter.add(Shape::dashed_line(&[c[i], c[(i + 1) % 4]], Stroke::new(1.2, col), 6.0, 4.0));
        }
    } else {
        painter.rect_stroke(bx.rect, 0.0, Stroke::new(1.2, col), egui::StrokeKind::Inside);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prefs::Prefs;
    use egui::pos2;

    #[test]
    fn a_window_takes_what_is_inside_a_crossing_what_it_touches() {
        let window = BoxSel::from_drag(pos2(0.0, 0.0), pos2(10.0, 10.0));
        let crossing = BoxSel::from_drag(pos2(10.0, 10.0), pos2(0.0, 0.0));
        assert!(!window.crossing && crossing.crossing);
        let inside = [pos2(2.0, 2.0), pos2(8.0, 8.0)];
        let across = [pos2(-5.0, 5.0), pos2(15.0, 5.0)];
        let away = [pos2(20.0, 20.0), pos2(30.0, 20.0)];
        assert!(window.takes(&inside) && crossing.takes(&inside));
        assert!(!window.takes(&across) && crossing.takes(&across), "crosses with no point inside");
        assert!(!window.takes(&away) && !crossing.takes(&away));
        let around = [pos2(-50.0, -50.0), pos2(50.0, -50.0), pos2(0.0, 50.0)];
        assert!(crossing.touches_triangle(around), "the box inside a large triangle");
    }

    /// Two flat 10 mm triangles 30 mm apart, a support under the first, seen from the top.
    #[test]
    fn a_box_selects_the_models_and_supports_it_takes() {
        let dir = std::env::temp_dir().join(format!("resin-ui-box-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let stl = dir.join("tri.stl");
        std::fs::write(&stl, "solid t\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 10 0 0\nvertex 0 10 0\nendloop\nendfacet\nendsolid t\n")
            .unwrap();
        let mut app = ResinApp::build(Prefs::default(), &[]);
        let import =
            |app: &mut ResinApp| app.run("file.import_stl", json!({ "path": stl, "fit": false, "lifted": true })).unwrap()["id"].as_u64().unwrap();
        let (a, b) = (import(&mut app), import(&mut app));
        std::fs::remove_dir_all(dir).ok();
        app.run("model.move", json!({ "ids": [b], "by": [30, 0, 0] })).unwrap();
        let tip = app.run("support.add", json!({ "model": a, "point": [2, 2, 5], "normal": [0, 0, -1] })).unwrap()["id"].as_u64().unwrap();
        let preset = *app.session.support_settings().current();
        while app.routes.update(&app.session.scene, &preset) {}
        let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0));
        app.run("view.standard", json!({ "view": "top" })).unwrap();
        app.run("view.fit", json!({ "aspect": 800.0 / 600.0 })).unwrap();
        let proj = Proj { cam: app.session.camera, rect };
        let s = |p: [f64; 3]| proj.to_screen(Vec3::new(p[0], p[1], p[2])).unwrap();
        let selected = |app: &ResinApp| app.session.scene.selected_models.iter().copied().collect::<Vec<_>>();

        // Around the first triangle, rightwards: it alone.
        let (lo, hi) = (s([-1.0, -1.0, 5.0]), s([11.0, 11.0, 5.0]));
        apply(&mut app, &proj, BoxSel::from_drag(pos2(lo.x, hi.y), pos2(hi.x, lo.y)), false);
        assert_eq!(selected(&app), vec![a]);
        // From the first's middle to the second's, rightwards: wholly inside, neither.
        let (m1, m2) = (s([3.0, 3.0, 5.0]), s([33.0, 6.0, 5.0]));
        apply(&mut app, &proj, BoxSel::from_drag(pos2(m1.x, m2.y), pos2(m2.x, m1.y)), false);
        assert_eq!(selected(&app), Vec::<u64>::new());
        // The same leftwards: both, touched.
        apply(&mut app, &proj, BoxSel::from_drag(pos2(m2.x, m1.y), pos2(m1.x, m2.y)), false);
        assert_eq!(selected(&app), vec![a, b]);
        // Shift adds.
        apply(&mut app, &proj, BoxSel::from_drag(pos2(lo.x, hi.y), pos2(hi.x, lo.y)), false);
        let (lo2, hi2) = (s([29.0, -1.0, 5.0]), s([41.0, 11.0, 5.0]));
        apply(&mut app, &proj, BoxSel::from_drag(pos2(lo2.x, hi2.y), pos2(hi2.x, lo2.y)), true);
        assert_eq!(selected(&app), vec![a, b]);

        // In Support edition, the same box around the first model takes its support.
        app.mode = EditMode::Support;
        apply(&mut app, &proj, BoxSel::from_drag(pos2(lo.x, hi.y), pos2(hi.x, lo.y)), false);
        assert_eq!(app.session.scene.selected.iter().copied().collect::<Vec<_>>(), vec![tip]);
        apply(&mut app, &proj, BoxSel::from_drag(pos2(lo2.x, hi2.y), pos2(hi2.x, lo2.y)), false);
        assert!(app.session.scene.selected.is_empty());
    }

    /// A press on empty space and a drag select on the release; a press that navigates draws no box.
    #[test]
    fn a_drag_from_empty_space_draws_the_box() {
        let mut app = ResinApp::build(Prefs::default(), &[]);
        let ctx = egui::Context::default();
        let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0));
        let proj = Proj { cam: app.session.camera, rect };
        let frame = |app: &mut ResinApp, events: Vec<egui::Event>| {
            let raw = egui::RawInput { screen_rect: Some(rect), events, ..Default::default() };
            let mut out = ctx.run_ui(raw, |ui| track(app, ui, &proj));
            out.textures_delta.clear();
        };
        let button =
            |pos, pressed| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
        frame(&mut app, vec![egui::Event::PointerMoved(pos2(100.0, 100.0)), button(pos2(100.0, 100.0), true)]);
        press(&mut app, pos2(100.0, 100.0), egui::Modifiers::NONE);
        frame(&mut app, vec![egui::Event::PointerMoved(pos2(103.0, 102.0))]);
        assert!(app.boxsel.and_then(|d| d.sel()).is_none(), "a wobble is still a click");
        frame(&mut app, vec![egui::Event::PointerMoved(pos2(300.0, 200.0))]);
        assert_eq!(app.boxsel.and_then(|d| d.sel()).map(|b| b.crossing), Some(false));
        frame(&mut app, vec![button(pos2(300.0, 200.0), false)]);
        assert!(app.boxsel.is_none(), "the release ends the box");
    }
}
