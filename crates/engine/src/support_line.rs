//! Tips along a line between two tips (`support.add_line`): evenly spaced points on the straight
//! line between them, each cast onto the surface along the normal blended from the two tips'.
//! The UI's preview and the command share [`line_points`], so what is shown is what is made.

use resin_geom::Vec3;

use crate::Scene;

/// The most tips one line makes: a smaller spacing is refused rather than hanging the app.
pub const MAX_LINE_TIPS: usize = 1000;
/// The spacing a line starts with, in mm.
pub const DEFAULT_LINE_SPACING: f64 = 2.0;
/// How many of a line's tips share one new body when its two ends have bodies of their own.
pub const DEFAULT_TIPS_PER_BODY: usize = 3;

/// One point of a line.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LinePoint {
    /// On the straight line between the two tips.
    pub at: Vec3,
    /// Where it lands: the model, the point and the face's outward normal there. `None` when no
    /// surface facing the tips' way is within the line's length.
    pub hit: Option<(u64, Vec3, Vec3)>,
}

/// The points between tip `a` and tip `b` (each a point and its outward unit normal), every gap
/// the same and at most `spacing` mm; the two tips themselves are not among them. `None` when
/// `spacing` is not a positive number or the line would have over [`MAX_LINE_TIPS`] points.
pub fn line_points(scene: &Scene, a: (Vec3, Vec3), b: (Vec3, Vec3), spacing: f64) -> Option<Vec<LinePoint>> {
    if !(spacing.is_finite() && spacing > 0.0) {
        return None;
    }
    let length = a.0.dist(b.0);
    let gaps = (length / spacing).ceil();
    if !gaps.is_finite() || gaps > (MAX_LINE_TIPS + 1) as f64 {
        return None;
    }
    let gaps = (gaps as usize).max(1);
    let points = (1..gaps)
        .map(|i| {
            let t = i as f64 / gaps as f64;
            let at = a.0.lerp(b.0, t);
            // Opposite normals blend to nothing halfway: a tip from below is the usual case.
            let normal = a.1.lerp(b.1, t).normalized().unwrap_or(-Vec3::Z);
            LinePoint { at, hit: land(scene, at, normal, length) }
        })
        .collect();
    Some(points)
}

/// The surface nearest `p` along `normal` either way, within `reach`, whose face looks the same
/// way as `normal`: a face looking away would put the tip on the far side of a wall.
fn land(scene: &Scene, p: Vec3, normal: Vec3, reach: f64) -> Option<(u64, Vec3, Vec3)> {
    // A ray picks nothing at its very origin, and `p` is often on the surface already.
    const BACK: f64 = 0.01;
    [-normal, normal]
        .into_iter()
        .filter_map(|dir| {
            let (model, h) = scene.ray_hit(p - dir * BACK, dir)?;
            let off = (h.t - BACK).abs();
            let face = scene.model(model)?.face_normal(h.triangle)?;
            (off <= reach && face.dot(normal) > 0.0).then_some((off, (model, h.point, face)))
        })
        .min_by(|x, y| x.0.total_cmp(&y.0))
        .map(|(_, hit)| hit)
}
