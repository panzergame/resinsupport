//! The solids supports are drawn with: a frustum (cone, cylinder or prism) between two points.

use crate::Vec3;

/// A corner of a drawn triangle and the normal it is shaded with.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShadedVertex {
    pub pos: [f32; 3],
    pub normal: [f32; 3],
}

/// A frustum from `a` (radius `ra`) to `b` (radius `rb`) with `sides` sides (3 to 64), as
/// triangles wound counter-clockwise seen from outside, appended to `out`. `smooth` shades it
/// as a round cone or cylinder; otherwise each side is flat, as a prism. `caps` closes the ends
/// at `a` and `b`. Nothing for a zero-length or non-finite frustum.
pub fn frustum(a: Vec3, b: Vec3, ra: f64, rb: f64, sides: u32, smooth: bool, caps: (bool, bool), out: &mut Vec<ShadedVertex>) {
    let Some(w) = (b - a).normalized() else { return };
    if !(a.is_finite() && b.is_finite() && ra.is_finite() && rb.is_finite()) {
        return;
    }
    let (ra, rb) = (ra.max(0.0), rb.max(0.0));
    let helper = if w.z.abs() < 0.9 { Vec3::Z } else { Vec3::X };
    let Some(u) = helper.cross(w).normalized() else { return };
    let v = w.cross(u);
    let n = sides.clamp(3, 64);
    let dir = |i: u32| {
        let t = std::f64::consts::TAU * f64::from(i % n) / f64::from(n);
        u * t.cos() + v * t.sin()
    };
    let h = a.dist(b);
    let slope = |d: Vec3| (d * h + w * (ra - rb)).normalized().unwrap_or(d);
    let mut push = |p: Vec3, normal: Vec3| out.push(ShadedVertex { pos: p.to_f32(), normal: normal.to_f32() });
    for i in 0..n {
        let (d0, d1) = (dir(i), dir(i + 1));
        let (a0, a1, b0, b1) = (a + d0 * ra, a + d1 * ra, b + d0 * rb, b + d1 * rb);
        let (n0, n1) = if smooth {
            (slope(d0), slope(d1))
        } else {
            let f = slope((d0 + d1).normalized().unwrap_or(d0));
            (f, f)
        };
        if ra > 0.0 {
            push(a0, n0);
            push(a1, n1);
            push(b1, n1);
        }
        if rb > 0.0 {
            push(a0, n0);
            push(b1, n1);
            push(b0, n0);
        }
        if caps.0 && ra > 0.0 {
            push(a, -w);
            push(a1, -w);
            push(a0, -w);
        }
        if caps.1 && rb > 0.0 {
            push(b, w);
            push(b0, w);
            push(b1, w);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frustum_faces_out() {
        let mut out = Vec::new();
        let (a, b) = (Vec3::new(1.0, 2.0, 3.0), Vec3::new(4.0, -1.0, 7.0));
        frustum(a, b, 2.0, 1.0, 12, false, (true, true), &mut out);
        assert_eq!(out.len(), 12 * 4 * 3);
        let mid = (a + b) * 0.5;
        for t in out.chunks(3) {
            let [p, q, r] = [t[0], t[1], t[2]].map(|c| Vec3::from(c.pos));
            let n = (q - p).cross(r - p);
            let centre = (p + q + r) / 3.0;
            assert!(n.dot(centre - mid) > 0.0, "a triangle faces in");
            assert!(n.dot(Vec3::from(t[0].normal)) > 0.0, "shading disagrees with the winding");
        }
        let before = out.len();
        frustum(a, a, 1.0, 1.0, 8, true, (true, true), &mut out);
        frustum(a, b, f64::NAN, 1.0, 8, true, (true, true), &mut out);
        assert_eq!(out.len(), before);
    }
}
