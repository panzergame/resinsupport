//! Spheres and capsules: the ray tests for picking them, and a unit sphere to draw them with.

use crate::Vec3;

/// The distance along unit `dir` from `origin` to where the ray enters the sphere, or `0` when
/// the origin is inside it; `None` when it misses or the sphere is behind.
pub fn ray_sphere(origin: Vec3, dir: Vec3, center: Vec3, radius: f64) -> Option<f64> {
    if !(radius > 0.0) || !radius.is_finite() || !origin.is_finite() || !dir.is_finite() || !center.is_finite() {
        return None;
    }
    let oc = origin - center;
    let b = oc.dot(dir);
    let c = oc.len2() - radius * radius;
    if c <= 0.0 {
        return Some(0.0);
    }
    let disc = b * b - c;
    if b > 0.0 || disc < 0.0 {
        return None;
    }
    let t = -b - disc.sqrt();
    t.is_finite().then_some(t.max(0.0))
}

/// The distance along unit `dir` from `origin` to where the ray enters the capsule of radius
/// `radius` around segment `a b` (a sphere when `a == b`), or `0` when the origin is inside it;
/// `None` when it misses or the capsule is behind.
pub fn ray_capsule(origin: Vec3, dir: Vec3, a: Vec3, b: Vec3, radius: f64) -> Option<f64> {
    if !(radius > 0.0) || !radius.is_finite() || !origin.is_finite() || !dir.is_finite() || !a.is_finite() || !b.is_finite() {
        return None;
    }
    let caps = [ray_sphere(origin, dir, a, radius), ray_sphere(origin, dir, b, radius)].into_iter().flatten().min_by(f64::total_cmp);
    // The side: an infinite cylinder around the axis, kept between the ends (Ericson, Real-Time
    // Collision Detection, 5.3.7).
    let (d, m) = (b - a, origin - a);
    let (dd, md, nd) = (d.len2(), m.dot(d), dir.dot(d));
    let k = m.len2() - radius * radius;
    let c = dd * k - md * md;
    if dd > crate::EPS && c <= 0.0 && (0.0..=dd).contains(&md) {
        return Some(0.0);
    }
    let qa = dd * dir.len2() - nd * nd;
    let side = if dd > crate::EPS && qa > crate::EPS * dd {
        let qb = dd * m.dot(dir) - nd * md;
        let disc = qb * qb - qa * c;
        let t = (-qb - disc.max(0.0).sqrt()) / qa;
        let along = md + t * nd;
        (disc >= 0.0 && t >= 0.0 && (0.0..=dd).contains(&along)).then_some(t)
    } else {
        None
    };
    [caps, side].into_iter().flatten().filter(|t| t.is_finite()).min_by(f64::total_cmp)
}

/// A unit sphere at the origin, outward-wound: an icosahedron with each face split
/// `4^subdivisions` times (clamped to 0..=5) and pushed onto the sphere.
pub fn unit_sphere(subdivisions: u32) -> Vec<[[f32; 3]; 3]> {
    let p = (1.0 + 5f64.sqrt()) / 2.0;
    let v = [
        Vec3::new(-1.0, p, 0.0),
        Vec3::new(1.0, p, 0.0),
        Vec3::new(-1.0, -p, 0.0),
        Vec3::new(1.0, -p, 0.0),
        Vec3::new(0.0, -1.0, p),
        Vec3::new(0.0, 1.0, p),
        Vec3::new(0.0, -1.0, -p),
        Vec3::new(0.0, 1.0, -p),
        Vec3::new(p, 0.0, -1.0),
        Vec3::new(p, 0.0, 1.0),
        Vec3::new(-p, 0.0, -1.0),
        Vec3::new(-p, 0.0, 1.0),
    ];
    const FACES: [[usize; 3]; 20] = [
        [0, 11, 5],
        [0, 5, 1],
        [0, 1, 7],
        [0, 7, 10],
        [0, 10, 11],
        [1, 5, 9],
        [5, 11, 4],
        [11, 10, 2],
        [10, 7, 6],
        [7, 1, 8],
        [3, 9, 4],
        [3, 4, 2],
        [3, 2, 6],
        [3, 6, 8],
        [3, 8, 9],
        [4, 9, 5],
        [2, 4, 11],
        [6, 2, 10],
        [8, 6, 7],
        [9, 8, 1],
    ];
    let unit = |a: Vec3| a.normalized().unwrap_or(Vec3::Z);
    let mut tris: Vec<[Vec3; 3]> = FACES.iter().map(|f| f.map(|i| unit(v[i]))).collect();
    for _ in 0..subdivisions.min(5) {
        tris = tris
            .iter()
            .flat_map(|&[a, b, c]| {
                let (ab, bc, ca) = (unit((a + b) * 0.5), unit((b + c) * 0.5), unit((c + a) * 0.5));
                [[a, ab, ca], [ab, b, bc], [ca, bc, c], [ab, bc, ca]]
            })
            .collect();
    }
    tris.iter().map(|t| t.map(Vec3::to_f32)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Mesh;

    #[test]
    fn ray_enters_the_near_side() {
        let c = Vec3::new(0.0, 0.0, 10.0);
        assert_eq!(ray_sphere(Vec3::ZERO, Vec3::Z, c, 1.0), Some(9.0));
        assert_eq!(ray_sphere(Vec3::ZERO, -Vec3::Z, c, 1.0), None);
        assert_eq!(ray_sphere(Vec3::new(2.0, 0.0, 0.0), Vec3::Z, c, 1.0), None);
        assert_eq!(ray_sphere(c, Vec3::Z, c, 1.0), Some(0.0));
        for bad in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert_eq!(ray_sphere(Vec3::ZERO, Vec3::Z, c, bad), None);
        }
        assert_eq!(ray_sphere(Vec3::new(f64::NAN, 0.0, 0.0), Vec3::Z, c, 1.0), None);
    }

    #[test]
    fn ray_enters_the_capsule() {
        let (a, b) = (Vec3::new(-5.0, 0.0, 10.0), Vec3::new(5.0, 0.0, 10.0));
        // The side, from below.
        assert!((ray_capsule(Vec3::ZERO, Vec3::Z, a, b, 1.0).unwrap() - 9.0).abs() < 1e-12);
        // An end cap, along the axis.
        assert!((ray_capsule(Vec3::new(-20.0, 0.0, 10.0), Vec3::X, a, b, 1.0).unwrap() - 14.0).abs() < 1e-12);
        // Past an end, beside it.
        assert_eq!(ray_capsule(Vec3::new(7.0, 0.0, 0.0), Vec3::Z, a, b, 1.0), None);
        assert_eq!(ray_capsule(Vec3::ZERO, -Vec3::Z, a, b, 1.0), None);
        assert_eq!(ray_capsule(Vec3::new(0.0, 0.0, 10.5), Vec3::Z, a, b, 1.0), Some(0.0));
        // A zero-length segment is a sphere.
        assert_eq!(ray_capsule(Vec3::ZERO, Vec3::Z, b, b, 1.0), None);
        assert_eq!(ray_capsule(Vec3::new(5.0, 0.0, 0.0), Vec3::Z, b, b, 1.0), Some(9.0));
        for bad in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert_eq!(ray_capsule(Vec3::ZERO, Vec3::Z, a, b, bad), None);
        }
        assert_eq!(ray_capsule(Vec3::ZERO, Vec3::Z, Vec3::new(f64::NAN, 0.0, 0.0), b, 1.0), None);
    }

    #[test]
    fn unit_sphere_is_closed_and_outward() {
        let m = Mesh::new(unit_sphere(2));
        assert_eq!(m.triangle_count(), 20 * 16);
        let volume = 4.0 / 3.0 * std::f64::consts::PI;
        assert!(m.signed_volume() > 0.9 * volume && m.signed_volume() < volume);
        assert!(m.triangles.iter().flatten().all(|&p| (Vec3::from(p).len() - 1.0).abs() < 1e-6));
        assert_eq!(unit_sphere(99).len(), 20 * 4usize.pow(5));
    }
}
