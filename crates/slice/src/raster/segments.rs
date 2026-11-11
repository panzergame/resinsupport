// Ported from mslicer (GPL-3.0), slicer/src/geometry/partitioning/segments_1d.rs (`Segments1D`)
// and slicer/src/geometry/triangle.rs (`plane_triangle_intersection`) at 355aca2; see
// ATTRIBUTION.md. Changed: triangles come placed (X and Y in pixels, Z in mm) with their
// depth change, a bucket spans a triangle's whole Z range, nothing indexes past a slice.

//! A solid's triangles, bucketed along Z so a layer cuts only those that may span its plane.

use super::Segment;

/// Buckets per solid (mslicer's `SEGMENT_LAYERS`).
const BUCKETS: usize = 100;

pub(crate) struct Segments1D {
    start: f32,
    end: f32,
    step: f32,
    buckets: Vec<Vec<u32>>,
    triangles: Vec<[[f32; 3]; 3]>,
    /// Per triangle: +1 where a scanline along +X enters the solid, −1 where it leaves.
    delta: Vec<i8>,
}

impl Segments1D {
    /// `triangles` placed, all finite, each with its depth change in `delta`.
    pub(crate) fn new(triangles: Vec<[[f32; 3]; 3]>, delta: Vec<i8>) -> Segments1D {
        let (start, end) = triangles.iter().flatten().fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), v| (lo.min(v[2]), hi.max(v[2])));
        let step = (end - start) / BUCKETS as f32;
        let mut s = Segments1D { start, end, step, buckets: vec![Vec::new(); BUCKETS], triangles: Vec::new(), delta };
        if !(step > 0.0) || !step.is_finite() {
            // Flat or empty: no plane crosses it.
            return s;
        }
        for (i, t) in triangles.iter().enumerate() {
            let lo = t[0][2].min(t[1][2]).min(t[2][2]);
            let hi = t[0][2].max(t[1][2]).max(t[2][2]);
            // A flat triangle adds nothing to a cut.
            if lo == hi {
                continue;
            }
            let (b0, b1) = (s.bucket(lo), s.bucket(hi));
            for b in s.buckets.iter_mut().take(b1 + 1).skip(b0) {
                b.push(i as u32);
            }
        }
        s.triangles = triangles;
        s
    }

    fn bucket(&self, z: f32) -> usize {
        (((z - self.start) / self.step).max(0.0) as usize).min(BUCKETS - 1)
    }

    /// The segments where the plane at height `z` cuts the solid, appended to `out`.
    pub(crate) fn intersect_plane(&self, z: f32, out: &mut Vec<Segment>) {
        if !(z >= self.start && z <= self.end) || self.triangles.is_empty() {
            return;
        }
        let Some(bucket) = self.buckets.get(self.bucket(z)) else { return };
        for &i in bucket {
            let (Some(t), Some(&delta)) = (self.triangles.get(i as usize), self.delta.get(i as usize)) else { continue };
            if let Some([a, b]) = cut(t, z) {
                out.push(Segment { a, b, delta });
            }
        }
    }
}

/// Where the plane at `z` crosses triangle `t`: two points in X and Y, or none. Each crossing is
/// worked out from the edge's upper end, so two triangles sharing the edge get the same point to
/// the bit and the contour stays closed.
fn cut(t: &[[f32; 3]; 3], z: f32) -> Option<[[f32; 2]; 2]> {
    let mut out = [[0.0; 2]; 2];
    let mut n = 0;
    let mut cross = |p: &[f32; 3], q: &[f32; 3]| {
        let (dp, dq) = (p[2] - z, q[2] - z);
        let s = dp / (dp - dq);
        if let Some(o) = out.get_mut(n) {
            *o = [p[0] + s * (q[0] - p[0]), p[1] + s * (q[1] - p[1])];
        }
        n += 1;
    };
    for (p, q) in [(&t[0], &t[1]), (&t[1], &t[2]), (&t[2], &t[0])] {
        match (p[2] > z, q[2] > z) {
            (true, false) => cross(p, q),
            (false, true) => cross(q, p),
            _ => {}
        }
    }
    (n == 2).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plane_cuts_only_the_triangles_spanning_it() {
        let up = [[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [0.0, 0.0, 10.0]];
        let high = [[0.0, 5.0, 20.0], [10.0, 5.0, 20.0], [0.0, 5.0, 30.0]];
        let s = Segments1D::new(vec![up, high], vec![1, -1]);
        let mut out = Vec::new();
        s.intersect_plane(5.0, &mut out);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].delta, 1);
        out.clear();
        s.intersect_plane(25.0, &mut out);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].delta, -1);
        out.clear();
        for z in [-1.0, 15.0, 31.0, f32::NAN] {
            s.intersect_plane(z, &mut out);
        }
        assert!(out.is_empty());
        // Flat and empty solids cut nothing.
        Segments1D::new(vec![[[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]], vec![1]).intersect_plane(0.0, &mut out);
        Segments1D::new(Vec::new(), Vec::new()).intersect_plane(0.0, &mut out);
        assert!(out.is_empty());
    }
}
