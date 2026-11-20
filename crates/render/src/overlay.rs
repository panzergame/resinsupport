use resin_geom::Vec3;

use crate::Camera;

/// One coloured line segment (linear RGBA).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Line {
    pub a: Vec3,
    pub b: Vec3,
    pub color: [f32; 4],
}

/// Colours of the origin overlay; the front end fills them from its theme.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OverlayStyle {
    pub minor: [f32; 4],
    pub major: [f32; 4],
    pub x_axis: [f32; 4],
    pub y_axis: [f32; 4],
    pub z_axis: [f32; 4],
}

/// The ground grid on XY, its X and Y axis lines, and the +Z axis.
#[derive(Clone, Debug, PartialEq)]
pub struct Overlay {
    pub lines: Vec<Line>,
    /// Minor grid spacing in millimetres (a power of ten).
    pub step: f64,
    /// Where the grid is centred and how far it reaches; the shader fades lines toward `radius`.
    pub center: Vec3,
    pub radius: f64,
}

/// Lines per side of the centre, at most: keeps a grazing perspective view from asking for
/// millions of segments.
const MAX_HALF_LINES: i64 = 100;
/// Every this many minor lines, a major one.
const MAJOR_EVERY: i64 = 10;
/// The build plate's grid spacing, mm.
pub const PLATE_STEP: f64 = 10.0;

/// The build plate's solid parts, each triangle wound counter-clockwise seen from above: the
/// plate, and an arrow outside its front (−Y) edge pointing into it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlateSurface {
    pub plate: [[Vec3; 3]; 2],
    pub arrow: [[Vec3; 3]; 3],
}

/// Half the plate's sides, for a plate `x` by `y` mm; `None` for a size that is not one.
fn plate_half(x: f64, y: f64) -> Option<(f64, f64)> {
    (x.is_finite() && y.is_finite() && x > 0.0 && y > 0.0).then_some((x / 2.0, y / 2.0))
}

/// The build plate `x` by `y` mm centred on the origin, on the ground, and its front arrow.
pub fn plate_surface(x: f64, y: f64) -> Option<PlateSurface> {
    let (hx, hy) = plate_half(x, y)?;
    let quad = |x0: f64, y0: f64, x1: f64, y1: f64| {
        let [a, b, c, d] = [Vec3::new(x0, y0, 0.0), Vec3::new(x1, y0, 0.0), Vec3::new(x1, y1, 0.0), Vec3::new(x0, y1, 0.0)];
        [[a, b, c], [a, c, d]]
    };
    // Big enough to read from across the plate, small enough not to crowd a small one.
    let size = (x * 0.08).clamp(10.0, 25.0);
    let tip = -hy - size * 0.25;
    let head = tip - size * 0.5;
    let tail = tip - size;
    let [s0, s1] = quad(-size * 0.18, tail, size * 0.18, head);
    let point = [Vec3::new(-size * 0.5, head, 0.0), Vec3::new(size * 0.5, head, 0.0), Vec3::new(0.0, tip, 0.0)];
    Some(PlateSurface { plate: quad(-hx, -hy, hx, hy), arrow: [point, s0, s1] })
}

/// The grid spacing for a view showing `half_height` millimetres above and below its centre:
/// the power of ten that puts roughly 5 to 50 lines across the view's height.
pub fn grid_step(half_height: f64) -> f64 {
    if !(half_height > 0.0) || !half_height.is_finite() {
        return 1.0;
    }
    10f64.powf((half_height / 5.0).log10().floor()).clamp(0.01, 1e5)
}

impl Overlay {
    /// The overlay for `cam` in a view `aspect` wide per unit of height.
    pub fn build(cam: &Camera, aspect: f64, style: &OverlayStyle) -> Overlay {
        let aspect = if aspect.is_finite() && aspect > 0.0 { aspect.clamp(0.05, 20.0) } else { 1.0 };
        let hh = cam.half_height();
        let step = grid_step(hh);
        let major = step * MAJOR_EVERY as f64;
        // The grid follows the view's target over the ground, snapped to a major line so it does
        // not swim while panning.
        let snap = |v: f64| if v.is_finite() { (v / major).round() * major } else { 0.0 };
        let center = Vec3::new(snap(cam.target.x), snap(cam.target.y), 0.0);
        let reach = (hh * aspect.max(1.0) * 3.0).max(major * 2.0);
        let half_lines = ((reach / step).ceil() as i64).clamp(MAJOR_EVERY, MAX_HALF_LINES);
        let radius = half_lines as f64 * step;
        let mut lines = Vec::with_capacity((half_lines as usize * 2 + 1) * 2 + 3);
        let (x0, x1) = (center.x - radius, center.x + radius);
        let (y0, y1) = (center.y - radius, center.y + radius);
        for i in -half_lines..=half_lines {
            let x = center.x + i as f64 * step;
            let y = center.y + i as f64 * step;
            let index = |v: f64| (v / step).round() as i64;
            let color = |k: i64| if k % MAJOR_EVERY == 0 { style.major } else { style.minor };
            // The axis lines replace the grid lines through the origin.
            if index(x) != 0 {
                lines.push(Line { a: Vec3::new(x, y0, 0.0), b: Vec3::new(x, y1, 0.0), color: color(index(x)) });
            }
            if index(y) != 0 {
                lines.push(Line { a: Vec3::new(x0, y, 0.0), b: Vec3::new(x1, y, 0.0), color: color(index(y)) });
            }
        }
        if (y0..=y1).contains(&0.0) {
            lines.push(Line { a: Vec3::new(x0, 0.0, 0.0), b: Vec3::new(x1, 0.0, 0.0), color: style.x_axis });
        }
        if (x0..=x1).contains(&0.0) {
            lines.push(Line { a: Vec3::new(0.0, y0, 0.0), b: Vec3::new(0.0, y1, 0.0), color: style.y_axis });
        }
        lines.push(Line { a: Vec3::ZERO, b: Vec3::new(0.0, 0.0, major), color: style.z_axis });
        Overlay { lines, step, center, radius }
    }

    /// The build plate `x` by `y` mm centred on the origin: its outline, a grid every
    /// [`PLATE_STEP`] mm from its centre (ten times coarser while that would be too many lines),
    /// the X and Y axes across it and the +Z axis. `None` for a size that is not a plate.
    pub fn plate(x: f64, y: f64, style: &OverlayStyle) -> Option<Overlay> {
        let (hx, hy) = plate_half(x, y)?;
        let mut step = PLATE_STEP;
        while hx.max(hy) / step > MAX_HALF_LINES as f64 && step < f64::MAX / 10.0 {
            step *= 10.0;
        }
        let major = step * MAJOR_EVERY as f64;
        let color = |k: i64| if k % MAJOR_EVERY == 0 { style.major } else { style.minor };
        let mut lines = Vec::new();
        // Lines on the edge are the outline's.
        for i in (1..).take_while(|&i| (i as f64) * step < hx) {
            for x in [i as f64 * step, -(i as f64) * step] {
                lines.push(Line { a: Vec3::new(x, -hy, 0.0), b: Vec3::new(x, hy, 0.0), color: color(i) });
            }
        }
        for i in (1..).take_while(|&i| (i as f64) * step < hy) {
            for y in [i as f64 * step, -(i as f64) * step] {
                lines.push(Line { a: Vec3::new(-hx, y, 0.0), b: Vec3::new(hx, y, 0.0), color: color(i) });
            }
        }
        let c = [Vec3::new(-hx, -hy, 0.0), Vec3::new(hx, -hy, 0.0), Vec3::new(hx, hy, 0.0), Vec3::new(-hx, hy, 0.0)];
        lines.extend([0, 1, 2, 3].map(|i| Line { a: c[i], b: c[(i + 1) % 4], color: style.major }));
        lines.push(Line { a: Vec3::new(-hx, 0.0, 0.0), b: Vec3::new(hx, 0.0, 0.0), color: style.x_axis });
        lines.push(Line { a: Vec3::new(0.0, -hy, 0.0), b: Vec3::new(0.0, hy, 0.0), color: style.y_axis });
        lines.push(Line { a: Vec3::ZERO, b: Vec3::new(0.0, 0.0, major), color: style.z_axis });
        // The shader fades lines from 0.55 of the radius: none fade on the plate.
        let radius = (2.0 * hx.hypot(hy)).min(f64::MAX);
        Some(Overlay { lines, step, center: Vec3::ZERO, radius })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STYLE: OverlayStyle =
        OverlayStyle { minor: [0.0; 4], major: [0.5; 4], x_axis: [1.0, 0.0, 0.0, 1.0], y_axis: [0.0, 1.0, 0.0, 1.0], z_axis: [0.0, 0.0, 1.0, 1.0] };

    #[test]
    fn step_is_a_power_of_ten_matching_the_view() {
        assert_eq!(grid_step(50.0), 10.0);
        assert_eq!(grid_step(49.0), 1.0);
        assert_eq!(grid_step(500.0), 100.0);
        for bad in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert_eq!(grid_step(bad), 1.0);
        }
    }

    #[test]
    fn origin_axes_are_drawn_once_and_line_count_is_capped() {
        for cam in [Camera::default(), Camera { distance: 1e7, fov: 0.8, pitch: 0.001, ..Camera::default() }] {
            let o = Overlay::build(&cam, 16.0 / 9.0, &STYLE);
            assert_eq!(o.lines.iter().filter(|l| l.color == STYLE.x_axis).count(), 1);
            assert_eq!(o.lines.iter().filter(|l| l.color == STYLE.y_axis).count(), 1);
            assert_eq!(o.lines.iter().filter(|l| l.color == STYLE.z_axis).count(), 1);
            assert!(o.lines.len() <= (MAX_HALF_LINES as usize * 2 + 1) * 2 + 3);
            assert!(o.lines.iter().all(|l| l.a.is_finite() && l.b.is_finite()));
        }
    }

    #[test]
    fn the_plate_grid_is_every_ten_millimetres_inside_the_plate() {
        let o = Overlay::plate(218.88, 122.88, &STYLE).unwrap();
        assert_eq!(o.step, 10.0);
        let (hx, hy) = (109.44, 61.44);
        assert!(o.lines.iter().all(|l| [l.a, l.b].iter().all(|p| p.x.abs() <= hx && p.y.abs() <= hy)));
        let mut xs: Vec<f64> =
            o.lines.iter().filter(|l| l.color == STYLE.minor || l.color == STYLE.major).filter(|l| l.a.x == l.b.x).map(|l| l.a.x).collect();
        xs.sort_by(f64::total_cmp);
        let inner: Vec<f64> = (-10..=10).filter(|&i| i != 0).map(|i| f64::from(i) * 10.0).collect();
        assert_eq!(xs, [vec![-hx], inner, vec![hx]].concat(), "every 10 mm, the edges, no line under the Y axis");
        for axis in [STYLE.x_axis, STYLE.y_axis, STYLE.z_axis] {
            assert_eq!(o.lines.iter().filter(|l| l.color == axis).count(), 1);
        }
        let far = o.lines.iter().flat_map(|l| [l.a, l.b]).map(|p| p.x.hypot(p.y)).fold(0.0, f64::max);
        assert!(far <= o.radius * 0.55, "no line fades on the plate");
    }

    #[test]
    fn a_plate_that_is_not_one_draws_nothing_and_a_huge_one_stays_bounded() {
        for (x, y) in [(0.0, 100.0), (-1.0, 100.0), (f64::NAN, 100.0), (100.0, f64::INFINITY)] {
            assert!(Overlay::plate(x, y, &STYLE).is_none() && plate_surface(x, y).is_none());
        }
        for size in [1e5, 1e300, f64::MAX] {
            let o = Overlay::plate(size, size, &STYLE).unwrap();
            assert!(o.lines.len() <= (MAX_HALF_LINES as usize * 2 + 2) * 2 + 3, "{}", o.lines.len());
            assert!(o.lines.iter().all(|l| l.a.is_finite() && l.b.is_finite()) && o.radius.is_finite());
        }
    }

    #[test]
    fn the_arrow_points_into_the_plate_from_outside_its_front_edge() {
        let s = plate_surface(200.0, 120.0).unwrap();
        let ccw = |[a, b, c]: [Vec3; 3]| (b - a).cross(c - a).z > 0.0;
        assert!(s.plate.into_iter().chain(s.arrow).all(ccw), "seen from above, every triangle faces up");
        let ys: Vec<f64> = s.arrow.iter().flatten().map(|p| p.y).collect();
        assert!(ys.iter().all(|&y| y < -60.0), "outside the front edge");
        let tip = s.arrow[0][2];
        assert!(ys.iter().all(|&y| y <= tip.y) && tip.x == 0.0, "pointing at the plate, centred");
    }

    #[test]
    fn grid_away_from_the_origin_has_no_axes_through_it() {
        let cam = Camera { target: Vec3::new(1e6, 1e6, 0.0), ..Camera::default() };
        let o = Overlay::build(&cam, 1.0, &STYLE);
        assert_eq!(o.lines.iter().filter(|l| l.color == STYLE.x_axis || l.color == STYLE.y_axis).count(), 0);
    }
}
