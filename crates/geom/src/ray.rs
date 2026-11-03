// Ported from SolidCraft (MIT OR Apache-2.0), crates/geom/src/mesh.rs (`RayFrame`) at 4bdffef0;
// see ATTRIBUTION.md.
//! Watertight ray/triangle intersection (Woop, Benthin & Wald, "Watertight Ray/Triangle
//! Intersection", JCGT 2(1), 2013): the vertices are moved into a frame where the ray runs along
//! +z through the origin, and each edge's side test is computed from those per-vertex values
//! alone, so the two triangles that share an edge get exactly opposite values there. A ray
//! through a shared edge or vertex hits at least one of them; it never slips between them.

use crate::Vec3;

/// A ray in the watertight test's frame: its dominant axis becomes z; x and y follow it
/// cyclically, swapped to keep the frame right-handed when the ray runs towards −z.
pub(crate) struct RayFrame {
    o: Vec3,
    k: [usize; 3],
    s: [f64; 3],
}

fn comp(v: Vec3, k: usize) -> f64 {
    match k {
        0 => v.x,
        1 => v.y,
        _ => v.z,
    }
}

impl RayFrame {
    /// `None` for an origin or direction that is not finite, or a zero direction.
    pub(crate) fn new(o: Vec3, dir: Vec3) -> Option<RayFrame> {
        if !o.is_finite() {
            return None;
        }
        let a = [dir.x.abs(), dir.y.abs(), dir.z.abs()];
        let kz = if a[0] >= a[1] && a[0] >= a[2] {
            0
        } else if a[1] >= a[2] {
            1
        } else {
            2
        };
        let dz = comp(dir, kz);
        if !(dz.is_finite() && dz != 0.0 && dir.is_finite()) {
            return None;
        }
        let (mut kx, mut ky) = ((kz + 1) % 3, (kz + 2) % 3);
        if dz < 0.0 {
            std::mem::swap(&mut kx, &mut ky);
        }
        Some(RayFrame { o, k: [kx, ky, kz], s: [comp(dir, kx) / dz, comp(dir, ky) / dz, 1.0 / dz] })
    }

    /// A vertex in the ray's frame: (x, y) across the ray, z scaled along it.
    fn frame(&self, p: Vec3) -> (f64, f64, f64) {
        let q = p - self.o;
        let z = comp(q, self.k[2]);
        (comp(q, self.k[0]) - self.s[0] * z, comp(q, self.k[1]) - self.s[1] * z, self.s[2] * z)
    }

    /// The distance along the ray to triangle `[pa, pb, pc]` (`None`: missed, or not in front).
    pub(crate) fn hit(&self, [pa, pb, pc]: [Vec3; 3]) -> Option<f64> {
        let ((ax, ay, az), (bx, by, bz), (cx, cy, cz)) = (self.frame(pa), self.frame(pb), self.frame(pc));
        let u = cx * by - cy * bx;
        let v = ax * cy - ay * cx;
        let w = bx * ay - by * ax;
        if (u < 0.0 || v < 0.0 || w < 0.0) && (u > 0.0 || v > 0.0 || w > 0.0) {
            return None;
        }
        let det = u + v + w;
        if det == 0.0 {
            return None;
        }
        let t = (u * az + v * bz + w * cz) / det;
        (t.is_finite() && t > 1e-9).then_some(t)
    }
}
