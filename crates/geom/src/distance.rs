//! Closest points between points, segments and triangles: what keeps a support clear of a model.
//! The methods are the textbook ones from Ericson, *Real-Time Collision Detection* (2005), §5.1.5
//! (point to triangle, by Voronoi regions) and §5.1.9 (segment to segment), written from the
//! book's description. Degenerate input (a zero-length segment, a collinear triangle) gives a
//! finite answer, never NaN, as long as the points are finite.

use crate::Vec3;

/// Squared lengths below this are zero.
const TINY2: f64 = 1e-24;

/// The point of triangle `a b c` nearest `p`.
pub fn closest_point_triangle(p: Vec3, a: Vec3, b: Vec3, c: Vec3) -> Vec3 {
    let (ab, ac, ap) = (b - a, c - a, p - a);
    let (d1, d2) = (ab.dot(ap), ac.dot(ap));
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let bp = p - b;
    let (d3, d4) = (ab.dot(bp), ac.dot(bp));
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        return a + ab * ratio(d1, d1 - d3);
    }
    let cp = p - c;
    let (d5, d6) = (ab.dot(cp), ac.dot(cp));
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        return a + ac * ratio(d2, d2 - d6);
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && d4 - d3 >= 0.0 && d5 - d6 >= 0.0 {
        return b + (c - b) * ratio(d4 - d3, (d4 - d3) + (d5 - d6));
    }
    let sum = va + vb + vc;
    if !(sum > 0.0) {
        // A collinear triangle has no inside: the nearest of its edges answers.
        return [(a, b), (b, c), (c, a)]
            .into_iter()
            .map(|(u, v)| closest_points_segments(p, p, u, v).1)
            .min_by(|x, y| x.dist(p).total_cmp(&y.dist(p)))
            .unwrap_or(a);
    }
    a + ab * (vb / sum) + ac * (vc / sum)
}

/// `n / d` in `[0, 1]`, and 0 when `d` is zero.
fn ratio(n: f64, d: f64) -> f64 {
    if d == 0.0 { 0.0 } else { (n / d).clamp(0.0, 1.0) }
}

/// The nearest points of segments `p1 q1` and `p2 q2`: one on each.
pub fn closest_points_segments(p1: Vec3, q1: Vec3, p2: Vec3, q2: Vec3) -> (Vec3, Vec3) {
    let (d1, d2, r) = (q1 - p1, q2 - p2, p1 - p2);
    let (a, e, f) = (d1.len2(), d2.len2(), d2.dot(r));
    let (s, t) = if a <= TINY2 && e <= TINY2 {
        (0.0, 0.0)
    } else if a <= TINY2 {
        (0.0, (f / e).clamp(0.0, 1.0))
    } else {
        let c = d1.dot(r);
        if e <= TINY2 {
            ((-c / a).clamp(0.0, 1.0), 0.0)
        } else {
            let b = d1.dot(d2);
            let denom = a * e - b * b;
            // Parallel segments: any s will do; 0 is as good as the next.
            let s = if denom > 0.0 { ((b * f - c * e) / denom).clamp(0.0, 1.0) } else { 0.0 };
            let t = (b * s + f) / e;
            if t < 0.0 {
                ((-c / a).clamp(0.0, 1.0), 0.0)
            } else if t > 1.0 {
                (((b - c) / a).clamp(0.0, 1.0), 1.0)
            } else {
                (s, t)
            }
        }
    };
    (p1 + d1 * s, p2 + d2 * t)
}

/// The distance between segment `p q` and triangle `t`; 0 when they cross.
pub fn segment_triangle_distance(p: Vec3, q: Vec3, t: [Vec3; 3]) -> f64 {
    let [a, b, c] = t;
    let n = (b - a).cross(c - a);
    if n.len2() > TINY2 {
        let (dp, dq) = (n.dot(p - a), n.dot(q - a));
        if (dp <= 0.0 && dq >= 0.0) || (dp >= 0.0 && dq <= 0.0) {
            let x = if dp == dq { p } else { p.lerp(q, dp / (dp - dq)) };
            if closest_point_triangle(x, a, b, c).dist(x) <= 1e-12 * (1.0 + x.len()) {
                return 0.0;
            }
        }
    }
    // Apart, the nearest pair has an end of the segment against the face, or the segment against
    // an edge.
    let ends = [p, q].map(|e| closest_point_triangle(e, a, b, c).dist(e));
    let edges = [(a, b), (b, c), (c, a)].map(|(u, v)| {
        let (x, y) = closest_points_segments(p, q, u, v);
        x.dist(y)
    });
    ends.into_iter().chain(edges).fold(f64::INFINITY, f64::min)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(x: f64, y: f64, z: f64) -> Vec3 {
        Vec3::new(x, y, z)
    }
    const T: [Vec3; 3] = [Vec3::new(0.0, 0.0, 0.0), Vec3::new(4.0, 0.0, 0.0), Vec3::new(0.0, 4.0, 0.0)];

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn point_to_triangle_in_each_region() {
        let [a, b, c] = T;
        assert_eq!(closest_point_triangle(v(1.0, 1.0, 3.0), a, b, c), v(1.0, 1.0, 0.0));
        assert_eq!(closest_point_triangle(v(-1.0, -1.0, 0.0), a, b, c), a);
        assert_eq!(closest_point_triangle(v(6.0, -1.0, 2.0), a, b, c), b);
        assert_eq!(closest_point_triangle(v(2.0, -3.0, 0.0), a, b, c), v(2.0, 0.0, 0.0));
        assert_eq!(closest_point_triangle(v(3.0, 3.0, 0.0), a, b, c), v(2.0, 2.0, 0.0));
        // Degenerate: every corner on one line, or all three at one point.
        let p = closest_point_triangle(v(1.0, 1.0, 0.0), a, b, v(2.0, 0.0, 0.0));
        assert_eq!(p, v(1.0, 0.0, 0.0));
        assert_eq!(closest_point_triangle(v(1.0, 1.0, 1.0), a, a, a), a);
    }

    #[test]
    fn segment_to_segment() {
        let (x, y) = closest_points_segments(v(0.0, 0.0, 0.0), v(2.0, 0.0, 0.0), v(1.0, -1.0, 1.0), v(1.0, 1.0, 1.0));
        assert_eq!((x, y), (v(1.0, 0.0, 0.0), v(1.0, 0.0, 1.0)));
        // Parallel, and overlapping in x.
        let (x, y) = closest_points_segments(v(0.0, 0.0, 0.0), v(2.0, 0.0, 0.0), v(1.0, 3.0, 0.0), v(5.0, 3.0, 0.0));
        assert!(close(x.dist(y), 3.0));
        // Zero-length segments.
        let (x, y) = closest_points_segments(v(1.0, 1.0, 1.0), v(1.0, 1.0, 1.0), v(0.0, 0.0, 0.0), v(0.0, 0.0, 0.0));
        assert!(close(x.dist(y), 3f64.sqrt()));
    }

    #[test]
    fn segment_to_triangle() {
        // Through the face.
        assert_eq!(segment_triangle_distance(v(1.0, 1.0, -1.0), v(1.0, 1.0, 1.0), T), 0.0);
        // Above the face, parallel to it.
        assert!(close(segment_triangle_distance(v(0.5, 0.5, 2.0), v(1.5, 0.5, 2.0), T), 2.0));
        // Ending above the face.
        assert!(close(segment_triangle_distance(v(1.0, 1.0, 1.5), v(1.0, 1.0, 9.0), T), 1.5));
        // Passing by an edge, outside the face.
        assert!(close(segment_triangle_distance(v(2.0, -1.0, -5.0), v(2.0, -1.0, 5.0), T), 1.0));
        // Passing by a corner, crossing the face's plane beyond it.
        assert!(close(segment_triangle_distance(v(-1.0, -1.0, -1.0), v(-1.0, -1.0, 1.0), T), 2f64.sqrt()));
        // A point segment and a degenerate triangle stay finite.
        assert!(close(segment_triangle_distance(v(1.0, 1.0, 1.0), v(1.0, 1.0, 1.0), T), 1.0));
        let flat = [v(0.0, 0.0, 0.0), v(1.0, 0.0, 0.0), v(2.0, 0.0, 0.0)];
        assert!(close(segment_triangle_distance(v(1.0, 2.0, -1.0), v(1.0, 2.0, 1.0), flat), 2.0));
    }
}
