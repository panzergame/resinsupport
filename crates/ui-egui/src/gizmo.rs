// Ported from SolidCraft (MIT OR Apache-2.0), crates/ui-egui/src/gizmo.rs and `snap_step` from
// crates/ui-egui/src/canvas.rs at 4bdffef0; see ATTRIBUTION.md. Ours: the scale handles.

//! The move/rotate/scale triad: X/Y/Z arrows, XY/YZ/XZ plane squares and a centre ball for a free
//! drag in the view plane (Move); X/Y/Z rotation rings (Rotate); X/Y/Z axis handles with square
//! ends along the model's own axes and the centre ball for a uniform scale (Scale). It is a
//! constant size on screen. A drag reports its change since the drag began (distances snap to
//! round steps at the current zoom, angles to 5°, factors to 1 %; hold Ctrl or Alt for a free
//! drag), so the caller applies it to the values it had when the drag started.

use egui::{Color32, Pos2, Rect, Sense, Shape, Stroke, vec2};
use resin_engine::geom::Vec3;

use crate::navigation::Proj;
use crate::theme::Tokens;

/// A part of the triad.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Handle {
    /// Arrow along X, Y or Z (0, 1, 2).
    Axis(usize),
    /// Square in the plane across that axis (Plane(2) is XY).
    Plane(usize),
    /// Ring turning about that axis.
    Ring(usize),
    /// Centre ball: free drag in the view plane.
    Ball,
    /// Scale handle along the triad's scale axis 0, 1 or 2.
    Scale(usize),
    /// Centre ball while scaling: every axis at once.
    ScaleAll,
}

/// What a drag has done so far.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Change {
    Translate(Vec3),
    /// About X, Y or Z through the centre, in radians.
    Rotate {
        axis: usize,
        angle: f64,
    },
    /// Along scale axis `axis`, or along all of them.
    Scale {
        axis: Option<usize>,
        factor: f64,
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Drag {
    pub handle: Handle,
    pub change: Change,
    /// The first frame of the drag (take a snapshot of the values now).
    pub started: bool,
    /// The drag ended this frame.
    pub done: bool,
}

/// Where the triad is and which handles it shows.
#[derive(Clone, Copy, Debug)]
pub struct Triad {
    pub center: Vec3,
    pub translate: bool,
    pub rotate: bool,
    /// The scale handles' unit directions (the model's own axes), when scaling.
    pub scale: Option<[Vec3; 3]>,
}

#[derive(Clone, Copy, Debug)]
struct State {
    handle: Handle,
    center: Vec3,
    /// Grab point on the plane (planes, ball) or parameter along the axis.
    grab: Vec3,
    t0: f64,
    /// Ring: last angle seen and the angle turned so far.
    last: f64,
    turned: f64,
    /// Where the press was (uniform scale).
    from: Pos2,
}

const DIRS: [Vec3; 3] = [Vec3::X, Vec3::Y, Vec3::Z];
/// Arrow length on screen (px).
pub const LEN_PX: f64 = 110.0;

/// How near a handle a press still takes it (px): the widest, the centre ball.
pub const HIT_TOLERANCE: f32 = 9.0;

/// Ring radius as a share of the arrow length.
const RING: f64 = 0.62;
/// Uniform scale: pixels of upward drag that double the size.
const SCALE_ALL_PX: f64 = 150.0;

/// In-plane directions of the ring about an axis (right-handed: e1 × e2 = axis).
fn ring_basis(i: usize) -> (Vec3, Vec3) {
    match i {
        0 => (Vec3::Y, Vec3::Z),
        1 => (Vec3::Z, Vec3::X),
        _ => (Vec3::X, Vec3::Y),
    }
}

/// Distance from `p` to the screen segment a–b.
fn seg_dist(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_sq().max(1e-6)).clamp(0.0, 1.0);
    (a + ab * t).distance(p)
}

/// The closed polygon's edges.
fn edges(q: &[Pos2]) -> impl Iterator<Item = (Pos2, Pos2)> + '_ {
    q.iter().copied().zip(q.iter().copied().cycle().skip(1))
}

fn in_quad(p: Pos2, q: &[Pos2]) -> bool {
    if q.len() < 3 {
        return false;
    }
    let mut sign = 0.0f32;
    for (a, b) in edges(q) {
        let c = (b - a).x * (p - a).y - (b - a).y * (p - a).x;
        if c.abs() < 1e-6 {
            continue;
        }
        if sign == 0.0 {
            sign = c.signum();
        } else if c.signum() != sign {
            return false;
        }
    }
    true
}

/// Where the view ray through `pos` meets the plane through `o` with normal `n`.
fn on_plane(proj: &Proj, pos: Pos2, o: Vec3, n: Vec3) -> Option<Vec3> {
    let (ro, rd) = proj.ray(pos);
    let den = rd.dot(n);
    if den.abs() < 1e-9 {
        return None;
    }
    Some(ro + rd * ((o - ro).dot(n) / den))
}

/// Parameter along the line `o + d t` closest to the view ray through `pos`.
fn along(proj: &Proj, pos: Pos2, o: Vec3, d: Vec3) -> Option<f64> {
    let (ro, r) = proj.ray(pos);
    let w0 = o - ro;
    let (b, dd, e) = (d.dot(r), d.dot(w0), r.dot(w0));
    let den = 1.0 - b * b;
    // Looking straight along the line: nothing to drag.
    if den.abs() < 1e-6 {
        return None;
    }
    Some((b * e - dd) / den)
}

/// The triad's shapes on screen.
struct Shapes {
    arrows: Vec<(usize, Pos2, Pos2)>,
    planes: Vec<(usize, Vec<Pos2>)>,
    rings: Vec<(usize, Vec<Pos2>)>,
    scales: Vec<(usize, Pos2, Pos2)>,
    ball: Pos2,
}

fn shapes(proj: &Proj, t: &Triad, l: f64) -> Option<Shapes> {
    let c = t.center;
    let ball = proj.to_screen(c)?;
    let mut s = Shapes { arrows: Vec::new(), planes: Vec::new(), rings: Vec::new(), scales: Vec::new(), ball };
    if t.translate {
        for (i, d) in DIRS.iter().enumerate() {
            if let (Some(a), Some(b)) = (proj.to_screen(c + *d * (0.18 * l)), proj.to_screen(c + *d * l)) {
                s.arrows.push((i, a, b));
            }
        }
        for i in 0..3 {
            let (e1, e2) = ring_basis(i);
            let q: Vec<Pos2> = [(0.25, 0.25), (0.45, 0.25), (0.45, 0.45), (0.25, 0.45)]
                .iter()
                .filter_map(|(a, b)| proj.to_screen(c + e1 * (a * l) + e2 * (b * l)))
                .collect();
            if q.len() == 4 {
                s.planes.push((i, q));
            }
        }
    }
    if t.rotate {
        for i in 0..3 {
            let (e1, e2) = ring_basis(i);
            let pts: Vec<Pos2> = (0..=64)
                .map(|k| f64::from(k) / 64.0 * std::f64::consts::TAU)
                .filter_map(|th| proj.to_screen(c + (e1 * th.cos() + e2 * th.sin()) * (RING * l)))
                .collect();
            if pts.len() > 2 {
                s.rings.push((i, pts));
            }
        }
    }
    if let Some(axes) = t.scale {
        for (i, d) in axes.iter().enumerate() {
            if let (Some(a), Some(b)) = (proj.to_screen(c + *d * (0.18 * l)), proj.to_screen(c + *d * (0.85 * l))) {
                s.scales.push((i, a, b));
            }
        }
    }
    Some(s)
}

fn hit(t: &Triad, s: &Shapes, p: Pos2) -> Option<Handle> {
    if s.ball.distance(p) < HIT_TOLERANCE {
        return Some(if t.scale.is_some() && !t.translate { Handle::ScaleAll } else { Handle::Ball });
    }
    if let Some((i, ..)) = s.arrows.iter().find(|(_, a, b)| seg_dist(p, *a, *b) < 6.0) {
        return Some(Handle::Axis(*i));
    }
    if let Some((i, ..)) = s.scales.iter().find(|(_, a, b)| seg_dist(p, *a, *b) < 7.0) {
        return Some(Handle::Scale(*i));
    }
    if let Some((i, _)) = s.planes.iter().find(|(_, q)| in_quad(p, q)) {
        return Some(Handle::Plane(*i));
    }
    s.rings.iter().find(|(_, pts)| pts.windows(2).any(|w| seg_dist(p, w[0], w[1]) < 5.0)).map(|(i, _)| Handle::Ring(*i))
}

/// Millimetres per screen pixel at a point.
fn mm_per_px(proj: &Proj, c: Vec3) -> Option<f64> {
    let (_, view) = proj.ray(proj.to_screen(c)?);
    let (right, _, _) = proj.cam.basis();
    let u = view.cross(right).normalized().or_else(|| view.cross(Vec3::Z).normalized())?;
    let (a, b) = (proj.to_screen(c)?, proj.to_screen(c + u)?);
    let px = f64::from(a.distance(b));
    (px > 1e-9).then(|| 1.0 / px)
}

fn snap(v: f64, step: f64, free: bool) -> f64 {
    if free || step <= 0.0 { v } else { (v / step).round() * step }
}

/// A round step for dragged distances at this zoom (1, 2 or 5 times a power of ten).
pub fn snap_step(half_height: f64) -> f64 {
    let raw = (half_height / 40.0).max(1e-4);
    let p = 10f64.powf(raw.log10().floor());
    if raw / p < 2.0 {
        p
    } else if raw / p < 5.0 {
        2.0 * p
    } else {
        5.0 * p
    }
}

/// Whether a drag on the triad `id` is under way.
pub fn dragging(ctx: &egui::Context, id: egui::Id) -> bool {
    ctx.data(|d| d.get_temp::<State>(id).is_some())
}

/// Forget a drag whose triad has gone (the selection emptied, the tool was put away).
pub fn cancel(ctx: &egui::Context, id: egui::Id) {
    ctx.data_mut(|d| d.remove::<State>(id));
}

/// Hit-test before the camera sees the press: a press on a handle must not navigate or pick.
pub fn contains_pointer(proj: &Proj, t: &Triad, p: Pos2) -> bool {
    mm_per_px(proj, t.center).and_then(|scale| shapes(proj, t, scale * LEN_PX)).is_some_and(|sh| hit(t, &sh, p).is_some())
}

/// Draw the triad and run a drag on it. `step` is the distance snap step; `free` (Ctrl or Alt
/// held) turns snapping off.
pub fn show(ui: &egui::Ui, painter: &egui::Painter, proj: &Proj, id: egui::Id, t: &Triad, step: f64, free: bool, tk: &Tokens) -> Option<Drag> {
    let l = mm_per_px(proj, t.center)? * LEN_PX;
    let sh = shapes(proj, t, l)?;
    let state: Option<State> = ui.data(|d| d.get_temp(id));
    let pointer = ui.input(|i| i.pointer.hover_pos());
    let hovered = if state.is_none() { pointer.filter(|_| ui.rect_contains_pointer(proj.rect)).and_then(|p| hit(t, &sh, p)) } else { None };
    let active = state.map(|s| s.handle).or(hovered);
    let hot = |h: Handle| active == Some(h);
    let axes = tk.axes();
    let col = |h: Handle, i: usize| if hot(h) { tk.gizmo_hot } else { axes[i.min(2)] };
    for (i, pts) in &sh.rings {
        painter.add(Shape::line(pts.clone(), Stroke::new(if hot(Handle::Ring(*i)) { 3.0 } else { 1.8 }, col(Handle::Ring(*i), *i))));
    }
    for (i, q) in &sh.planes {
        let c = col(Handle::Plane(*i), *i);
        painter.extend(projected_polygon(q, c.gamma_multiply(if hot(Handle::Plane(*i)) { 0.7 } else { 0.35 }), Stroke::new(1.0, c)));
    }
    for (i, a, b) in &sh.arrows {
        arrow(painter, *a, *b, col(Handle::Axis(*i), *i), hot(Handle::Axis(*i)));
    }
    for (i, a, b) in &sh.scales {
        scale_handle(painter, *a, *b, col(Handle::Scale(*i), *i), hot(Handle::Scale(*i)));
    }
    let ball_hot = hot(Handle::Ball) || hot(Handle::ScaleAll);
    painter.circle(sh.ball, 6.0, if ball_hot { tk.gizmo_hot } else { tk.gizmo_ball }, Stroke::new(1.5, tk.gizmo_ball_edge));
    let p = pointer?;
    active?;
    ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    let r = ui.interact(Rect::from_center_size(p, vec2(24.0, 24.0)), id.with("hit"), Sense::drag());
    let view = proj.ray(sh.ball).1;
    let plane_normal = |h: Handle| match h {
        Handle::Plane(i) | Handle::Ring(i) => DIRS[i],
        _ => view,
    };
    let scale_dir = |i: usize| t.scale.map_or(DIRS[i], |a| a[i]);
    let mut started = false;
    let mut st = match state {
        Some(s) => s,
        None => {
            if !r.drag_started() {
                return None;
            }
            let h = hovered?;
            let from = ui.input(|i| i.pointer.press_origin()).unwrap_or(p);
            let c = t.center;
            let mut s = State { handle: h, center: c, grab: c, t0: 0.0, last: 0.0, turned: 0.0, from };
            match h {
                Handle::Axis(i) => s.t0 = along(proj, from, c, DIRS[i])?,
                Handle::Scale(i) => s.t0 = along(proj, from, c, scale_dir(i)).filter(|t0| t0.abs() > 1e-9)?,
                Handle::Ring(i) => {
                    let q = on_plane(proj, from, c, DIRS[i])? - c;
                    let (e1, e2) = ring_basis(i);
                    s.last = q.dot(e2).atan2(q.dot(e1));
                }
                Handle::ScaleAll => {}
                _ => s.grab = on_plane(proj, from, c, plane_normal(h))?,
            }
            started = true;
            s
        }
    };
    let done = r.drag_stopped() || !r.dragged() && !started;
    // The frame's events can hold the release and further movement — the hand keeps moving as
    // the button goes up, or input queued during a busy frame — and `p` is already past the
    // release then. The release event itself carries where the button was let go: take the
    // final change from there, so the drag ends where the button was released, not where the
    // mouse has since got to.
    let p = if done {
        ui.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::PointerButton { pos, pressed: false, .. } => Some(*pos),
                _ => None,
            })
        })
        .unwrap_or(p)
    } else {
        p
    };
    let change = match st.handle {
        Handle::Axis(i) => {
            let v = along(proj, p, st.center, DIRS[i]).map(|v| v - st.t0).unwrap_or(0.0);
            Change::Translate(DIRS[i] * snap(v, step, free))
        }
        Handle::Ring(i) => {
            if let Some(q) = on_plane(proj, p, st.center, DIRS[i]).map(|q| q - st.center) {
                let (e1, e2) = ring_basis(i);
                let th = q.dot(e2).atan2(q.dot(e1));
                let mut d = th - st.last;
                while d > std::f64::consts::PI {
                    d -= std::f64::consts::TAU;
                }
                while d < -std::f64::consts::PI {
                    d += std::f64::consts::TAU;
                }
                st.turned += d;
                st.last = th;
            }
            Change::Rotate { axis: i, angle: snap(st.turned, 5f64.to_radians(), free) }
        }
        Handle::Scale(i) => {
            let k = along(proj, p, st.center, scale_dir(i)).map_or(1.0, |v| v / st.t0);
            Change::Scale { axis: Some(i), factor: snap(k, 0.01, free).max(0.01) }
        }
        Handle::ScaleAll => {
            let k = 2f64.powf(f64::from(st.from.y - p.y) / SCALE_ALL_PX);
            Change::Scale { axis: None, factor: snap(k, 0.01, free).max(0.01) }
        }
        h => {
            let d = on_plane(proj, p, st.center, plane_normal(h)).map(|q| q - st.grab).unwrap_or(Vec3::ZERO);
            Change::Translate(Vec3::new(snap(d.x, step, free), snap(d.y, step, free), snap(d.z, step, free)))
        }
    };
    if done {
        ui.data_mut(|d| d.remove::<State>(id));
    } else {
        ui.data_mut(|d| d.insert_temp(id, st));
    }
    Some(Drag { handle: st.handle, change, started, done })
}

/// A filled polygon from projected points (a fan from the first), outlined.
fn projected_polygon(pts: &[Pos2], fill: Color32, stroke: Stroke) -> Vec<Shape> {
    let mut mesh = egui::Mesh::default();
    for (i, p) in (0u32..).zip(pts) {
        mesh.colored_vertex(*p, fill);
        if i >= 2 {
            mesh.add_triangle(0, i - 1, i);
        }
    }
    let mut out = vec![Shape::mesh(mesh)];
    out.extend(edges(pts).map(|(a, b)| Shape::line_segment([a, b], stroke)));
    out
}

fn arrow(painter: &egui::Painter, a: Pos2, b: Pos2, col: Color32, hot: bool) {
    let v = b - a;
    let len = v.length();
    if len < 1.0 {
        return;
    }
    let u = v / len;
    let n = vec2(-u.y, u.x);
    painter.line_segment([a, b], Stroke::new(if hot { 3.0 } else { 2.0 }, col));
    let head = 12.0f32.min(len * 0.5);
    painter.add(Shape::convex_polygon(vec![b + u * 2.0, b - u * head + n * head * 0.4, b - u * head - n * head * 0.4], col, Stroke::NONE));
}

fn scale_handle(painter: &egui::Painter, a: Pos2, b: Pos2, col: Color32, hot: bool) {
    if (b - a).length() < 1.0 {
        return;
    }
    painter.line_segment([a, b], Stroke::new(if hot { 3.0 } else { 2.0 }, col));
    painter.rect_filled(Rect::from_center_size(b, vec2(9.0, 9.0)), 1.0, col);
}

#[cfg(test)]
mod tests {
    use super::*;
    use resin_engine::render::Camera;

    fn proj() -> Proj {
        let mut cam = Camera::default();
        cam.fit(&[Vec3::new(-10.0, -10.0, -10.0), Vec3::new(10.0, 10.0, 10.0)].into_iter().collect());
        Proj { cam, rect: Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0)) }
    }

    #[test]
    fn the_triad_is_a_fixed_size_on_screen() {
        let p = proj();
        let t = Triad { center: Vec3::ZERO, translate: true, rotate: true, scale: None };
        let l = mm_per_px(&p, t.center).unwrap() * LEN_PX;
        let sh = shapes(&p, &t, l).unwrap();
        for (_, a, b) in &sh.arrows {
            assert!(a.distance(sh.ball) < b.distance(sh.ball));
            assert!(b.distance(sh.ball) <= LEN_PX as f32 + 1.0);
        }
        assert_eq!(sh.rings.len(), 3);
    }

    #[test]
    fn handles_are_hit_where_they_are_drawn() {
        let p = proj();
        let t = Triad { center: Vec3::ZERO, translate: false, rotate: false, scale: Some(DIRS) };
        let l = mm_per_px(&p, t.center).unwrap() * LEN_PX;
        let sh = shapes(&p, &t, l).unwrap();
        assert_eq!(hit(&t, &sh, sh.ball), Some(Handle::ScaleAll));
        for (i, _, b) in &sh.scales {
            assert_eq!(hit(&t, &sh, *b), Some(Handle::Scale(*i)));
        }
        assert!(!contains_pointer(&p, &t, sh.ball + vec2(300.0, 0.0)));
    }

    #[test]
    fn snap_steps_are_round() {
        assert_eq!(snap_step(40.0), 1.0);
        assert_eq!(snap_step(100.0), 2.0);
        assert_eq!(snap_step(300.0), 5.0);
        assert_eq!(snap(7.4, 5.0, false), 5.0);
        assert_eq!(snap(7.4, 5.0, true), 7.4);
    }
}
