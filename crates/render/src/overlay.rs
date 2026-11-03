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
    fn grid_away_from_the_origin_has_no_axes_through_it() {
        let cam = Camera { target: Vec3::new(1e6, 1e6, 0.0), ..Camera::default() };
        let o = Overlay::build(&cam, 1.0, &STYLE);
        assert_eq!(o.lines.iter().filter(|l| l.color == STYLE.x_axis || l.color == STYLE.y_axis).count(), 0);
    }
}
