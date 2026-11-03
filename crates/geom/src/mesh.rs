use crate::ray::RayFrame;
use crate::{Aabb3, Affine3, Vec3};

/// A triangle soup, as STL stores it: three corners per triangle, counter-clockwise seen from
/// outside. f32 because that is what STL holds and what the GPU draws; measures widen to f64.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Mesh {
    pub triangles: Vec<[[f32; 3]; 3]>,
}

/// Where a ray meets a mesh.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RayHit {
    /// Distance along the (unit) ray direction.
    pub t: f64,
    pub point: Vec3,
    pub triangle: usize,
}

impl Mesh {
    pub fn new(triangles: Vec<[[f32; 3]; 3]>) -> Mesh {
        Mesh { triangles }
    }
    pub fn triangle_count(&self) -> usize {
        self.triangles.len()
    }
    pub fn is_empty(&self) -> bool {
        self.triangles.is_empty()
    }
    pub fn bounds(&self) -> Aabb3 {
        self.triangles.iter().flatten().map(|&p| Vec3::from(p)).collect()
    }
    /// Every corner moved by `xf`. A mirroring map (negative determinant) swaps two corners of
    /// each triangle so the winding still faces out.
    pub fn transformed(&self, xf: &Affine3) -> Mesh {
        let mirror = xf.determinant() < 0.0;
        let triangles = self
            .triangles
            .iter()
            .map(|t| {
                let [a, b, c] = t.map(|p| xf.point(Vec3::from(p)).to_f32());
                if mirror { [a, c, b] } else { [a, b, c] }
            })
            .collect();
        Mesh { triangles }
    }
    /// Unit normal from the winding (none for a degenerate triangle).
    pub fn face_normal(t: &[[f32; 3]; 3]) -> Option<Vec3> {
        let [a, b, c] = t.map(Vec3::from);
        (b - a).cross(c - a).normalized()
    }
    pub fn area(&self) -> f64 {
        self.triangles
            .iter()
            .map(|t| {
                let [a, b, c] = t.map(Vec3::from);
                (b - a).cross(c - a).len() * 0.5
            })
            .sum()
    }
    /// Signed enclosed volume (divergence theorem). Positive for a closed, outward-wound mesh;
    /// meaningless for an open one.
    pub fn signed_volume(&self) -> f64 {
        self.triangles
            .iter()
            .map(|t| {
                let [a, b, c] = t.map(Vec3::from);
                a.dot(b.cross(c)) / 6.0
            })
            .sum()
    }
    /// The nearest triangle hit by the ray `origin + t·dir` with `t > 0`, either side, by the
    /// watertight test (a ray through a shared edge never slips between its triangles). `dir`
    /// should be unit length so `t` is a distance. A scan of every triangle: [`crate::MeshIndex`]
    /// answers the same faster.
    pub fn ray_hit(&self, origin: Vec3, dir: Vec3) -> Option<RayHit> {
        let frame = RayFrame::new(origin, dir)?;
        let mut best: Option<(f64, usize)> = None;
        for (i, tri) in self.triangles.iter().enumerate() {
            if let Some(t) = frame.hit(tri.map(Vec3::from))
                && best.is_none_or(|(bt, _)| t < bt)
            {
                best = Some((t, i));
            }
        }
        best.map(|(t, triangle)| RayHit { t, point: origin + dir * t, triangle })
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// An axis-aligned cube of side `s` at the origin, outward-wound.
    pub(crate) fn cube(s: f32) -> Mesh {
        let v = |x: f32, y: f32, z: f32| [x * s, y * s, z * s];
        let quads = [
            [v(0., 0., 0.), v(0., 1., 0.), v(1., 1., 0.), v(1., 0., 0.)],
            [v(0., 0., 1.), v(1., 0., 1.), v(1., 1., 1.), v(0., 1., 1.)],
            [v(0., 0., 0.), v(1., 0., 0.), v(1., 0., 1.), v(0., 0., 1.)],
            [v(0., 1., 0.), v(0., 1., 1.), v(1., 1., 1.), v(1., 1., 0.)],
            [v(0., 0., 0.), v(0., 0., 1.), v(0., 1., 1.), v(0., 1., 0.)],
            [v(1., 0., 0.), v(1., 1., 0.), v(1., 1., 1.), v(1., 0., 1.)],
        ];
        Mesh::new(quads.iter().flat_map(|[a, b, c, d]| [[*a, *b, *c], [*a, *c, *d]]).collect())
    }

    #[test]
    fn cube_measures() {
        let m = cube(10.0);
        assert_eq!(m.triangle_count(), 12);
        assert!((m.signed_volume() - 1000.0).abs() < 1e-9);
        assert!((m.area() - 600.0).abs() < 1e-9);
        let b = m.bounds();
        assert_eq!((b.min, b.max), (Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0)));
    }

    #[test]
    fn transformed_keeps_volume_sign() {
        let m = cube(10.0);
        let big = m.transformed(&Affine3::scale(Vec3::new(2.0, 1.0, 1.0)));
        assert!((big.signed_volume() - 2000.0).abs() < 1e-6);
        let mirrored = m.transformed(&Affine3::scale(Vec3::new(-1.0, 1.0, 1.0)));
        assert!((mirrored.signed_volume() - 1000.0).abs() < 1e-6);
    }

    #[test]
    fn ray_hits_the_near_face() {
        let m = cube(10.0);
        let hit = m.ray_hit(Vec3::new(5.0, 5.0, 50.0), -Vec3::Z).unwrap();
        assert!((hit.t - 40.0).abs() < 1e-9);
        assert!((hit.point.z - 10.0).abs() < 1e-9);
        assert!(m.ray_hit(Vec3::new(50.0, 5.0, 50.0), -Vec3::Z).is_none());
        assert!(m.ray_hit(Vec3::new(f64::NAN, 5.0, 50.0), -Vec3::Z).is_none());
    }
}
