//! A raft's solid, from the field of its outline ([`Field2`]): a plate standing on the build
//! plate, whose edge leans by a set slope, and an optional rim round its top with a hollow inside.
//!
//! The edge at height `z` is the field's level `(thickness - z) * slope`: level 0, the outline,
//! where the bases stand. That level is linear in `z`, so the sloped wall is the zero surface of
//! `field - level(z)` between the bottom and the top, cut by marching tetrahedra in the prisms
//! over the grid's triangles; the flat faces are the same triangles clipped to their levels.
//! Points on a grid edge are interpolated from its lower-numbered node, so the walls and the faces
//! share them exactly and the solid is closed.

use crate::Field2;

/// The raft's heights and edge, in millimetres.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RaftProfile {
    /// The plate, from Z = 0. Its top meets the outline.
    pub thickness: f64,
    /// How far the edge moves out per millimetre down: positive leans in going up (a chamfer),
    /// negative leans out (an undercut), 0 is a straight wall.
    pub slope: f64,
    /// A rim standing on the plate's edge, the hollow inside it open; 0 for none.
    pub rim_height: f64,
    /// The rim's width where it is narrowest.
    pub rim_width: f64,
}

impl RaftProfile {
    fn usable(&self) -> bool {
        self.thickness > 0.0
            && self.thickness.is_finite()
            && self.slope.is_finite()
            && self.rim_height.is_finite()
            && self.rim_width.is_finite()
            && self.top().is_finite()
    }

    fn rim(&self) -> bool {
        self.rim_height > 0.0 && self.rim_width > 0.0
    }

    /// The top of the raft: its rim's when it has one.
    pub fn top(&self) -> f64 {
        if self.rim() { self.thickness + self.rim_height } else { self.thickness }
    }

    /// The field's level of the edge at height `z`.
    fn edge(&self, z: f64) -> f64 {
        (self.thickness - z) * self.slope
    }

    /// How far past the outline the raft reaches, at its widest: how far its field must go.
    pub fn reach(&self) -> f64 {
        self.edge(0.0).max(self.edge(self.top())).max(0.0)
    }

    /// How far inside the outline the raft has an edge: how deep its field must be exact.
    pub fn depth(&self) -> f64 {
        let inner = if self.rim() { -self.inner() } else { 0.0 };
        inner.max(-self.edge(0.0)).max(-self.edge(self.top())).max(0.0)
    }

    /// The field's level of the rim's inner wall: `rim_width` in from the edge where the edge is
    /// innermost.
    fn inner(&self) -> f64 {
        self.edge(self.thickness).min(self.edge(self.top())) - self.rim_width
    }
}

/// A grid node: its number, where it is and the field's value there.
#[derive(Clone, Copy, Debug)]
struct Node {
    key: usize,
    p: [f64; 2],
    phi: f64,
}

/// Where the field crosses `level` between `a` and `b`, which lie on either side of it, measured
/// from the lower-numbered node so every face sharing the edge gets the very same point.
fn cross(a: Node, b: Node, level: f64) -> [f64; 2] {
    let (a, b) = if a.key <= b.key { (a, b) } else { (b, a) };
    let t = ((a.phi - level) / (a.phi - b.phi)).clamp(0.0, 1.0);
    let t = if t.is_finite() { t } else { 0.0 };
    [a.p[0] + (b.p[0] - a.p[0]) * t, a.p[1] + (b.p[1] - a.p[1]) * t]
}

/// The raft's solid over `field`, closed and wound outwards; nothing for an unusable profile.
pub fn raft_solid(field: &Field2, profile: &RaftProfile) -> Vec<[[f32; 3]; 3]> {
    let mut out = Vec::new();
    if !profile.usable() || field.nx < 2 || field.ny < 2 || field.values.len() < field.nx * field.ny {
        return out;
    }
    let node = |i: usize, j: usize| Node { key: j * field.nx + i, p: field.point(i, j), phi: field.at(i, j) };
    let (t, top) = (profile.thickness, profile.top());
    let (e0, e1) = (profile.edge(0.0), profile.edge(top));
    let inner = profile.inner();
    let mut emit = |tri: [[f64; 3]; 3]| {
        let (u, v) = (sub(tri[1], tri[0]), sub(tri[2], tri[0]));
        if dot(cross3(u, v), cross3(u, v)) > 1e-18 {
            out.push(tri.map(|p| p.map(|c| c as f32)));
        }
    };
    let caps: Vec<(f64, f64, f64, bool)> = if profile.rim() {
        vec![(f64::NEG_INFINITY, e0, 0.0, false), (f64::NEG_INFINITY, inner, t, true), (inner, e1, top, true)]
    } else {
        vec![(f64::NEG_INFINITY, e0, 0.0, false), (f64::NEG_INFINITY, e1, top, true)]
    };
    for j in 0..field.ny - 1 {
        for (lo, hi, z, up) in &caps {
            cap_row(field, j, *lo, *hi, *z, *up, &mut emit);
        }
        for i in 0..field.nx - 1 {
            let (n00, n10, n11, n01) = (node(i, j), node(i + 1, j), node(i + 1, j + 1), node(i, j + 1));
            for tri in [[n00, n10, n11], [n00, n11, n01]] {
                outer_wall(tri, [0.0, top], [e0, e1], &mut emit);
                if profile.rim() {
                    inner_wall(tri, inner, t, top, &mut emit);
                }
            }
        }
    }
    out
}

/// Row `j` of a flat face at `z` where the field lies in `lo..hi`, facing up or down: whole cells
/// inside merged into one quad per run, the others' triangles clipped.
fn cap_row(field: &Field2, j: usize, lo: f64, hi: f64, z: f64, up: bool, emit: &mut impl FnMut([[f64; 3]; 3])) {
    let class = |phi: f64| -> u8 {
        if phi < lo {
            0
        } else if phi < hi {
            1
        } else {
            2
        }
    };
    let node = |i: usize, j: usize| Node { key: j * field.nx + i, p: field.point(i, j), phi: field.at(i, j) };
    let mut face = |poly: &[[f64; 2]]| {
        let Some((first, rest)) = poly.split_first() else { return };
        for w in rest.windows(2) {
            if let [b, c] = w {
                let (a, b, c) = ([first[0], first[1], z], [b[0], b[1], z], [c[0], c[1], z]);
                emit(if up { [a, b, c] } else { [a, c, b] });
            }
        }
    };
    let mut run: Option<usize> = None;
    for i in 0..field.nx {
        let full = i + 1 < field.nx && [(i, j), (i + 1, j), (i + 1, j + 1), (i, j + 1)].iter().all(|&(a, b)| class(field.at(a, b)) == 1);
        if full {
            run.get_or_insert(i);
            continue;
        }
        if let Some(start) = run.take() {
            let (a, b) = (field.point(start, j), field.point(i, j + 1));
            face(&[a, [b[0], a[1]], b, [a[0], b[1]]]);
        }
        if i + 1 >= field.nx {
            break;
        }
        let (n00, n10, n11, n01) = (node(i, j), node(i + 1, j), node(i + 1, j + 1), node(i, j + 1));
        for tri in [[n00, n10, n11], [n00, n11, n01]] {
            let cls = tri.map(|n| class(n.phi));
            if cls.iter().all(|c| *c == 0) || cls.iter().all(|c| *c == 2) {
                continue;
            }
            // The triangle clipped to the band, walked round its edges: convex, at most six corners.
            let mut poly: Vec<[f64; 2]> = Vec::with_capacity(6);
            for k in 0..3 {
                let (a, b) = (tri[k], tri[(k + 1) % 3]);
                let (ca, cb) = (cls[k], cls[(k + 1) % 3]);
                if ca == 1 {
                    poly.push(a.p);
                }
                match (ca, cb) {
                    (0, 1) | (1, 0) => poly.push(cross(a, b, lo)),
                    (1, 2) | (2, 1) => poly.push(cross(a, b, hi)),
                    (0, 2) => {
                        poly.push(cross(a, b, lo));
                        poly.push(cross(a, b, hi));
                    }
                    (2, 0) => {
                        poly.push(cross(a, b, hi));
                        poly.push(cross(a, b, lo));
                    }
                    _ => {}
                }
            }
            face(&poly);
        }
    }
}

/// The sloped outer wall in the prism over `tri` from `z[0]` to `z[1]`, where the edge's level
/// goes from `e[0]` to `e[1]`: the zero surface of `field - level`, by marching tetrahedra.
fn outer_wall(tri: [Node; 3], z: [f64; 2], e: [f64; 2], emit: &mut impl FnMut([[f64; 3]; 3])) {
    let f = |n: Node, layer: usize| n.phi - e[layer];
    let all = tri.iter().flat_map(|n| [f(*n, 0), f(*n, 1)]);
    let inside = all.clone().filter(|v| *v < 0.0).count();
    if inside == 0 || inside == 6 {
        return;
    }
    let mut v = tri;
    v.sort_by_key(|n| n.key);
    // Each shared side of two prisms is cut on the same diagonal, from its higher-numbered
    // bottom node to its lower-numbered top node, so neighbouring tetrahedra meet face to face.
    let tets =
        [[(v[0], 0), (v[1], 0), (v[2], 0), (v[0], 1)], [(v[1], 0), (v[2], 0), (v[0], 1), (v[1], 1)], [(v[2], 0), (v[0], 1), (v[1], 1), (v[2], 1)]];
    let pos = |(n, l): (Node, usize)| [n.p[0], n.p[1], z[l]];
    let point = |a: (Node, usize), b: (Node, usize)| -> [f64; 3] {
        if a.1 == b.1 {
            let p = cross(a.0, b.0, e[a.1]);
            return [p[0], p[1], z[a.1]];
        }
        let (a, b) = if (a.0.key, a.1) <= (b.0.key, b.1) { (a, b) } else { (b, a) };
        let (fa, fb) = (f(a.0, a.1), f(b.0, b.1));
        let t = (fa / (fa - fb)).clamp(0.0, 1.0);
        let t = if t.is_finite() { t } else { 0.0 };
        let (pa, pb) = (pos(a), pos(b));
        [pa[0] + (pb[0] - pa[0]) * t, pa[1] + (pb[1] - pa[1]) * t, pa[2] + (pb[2] - pa[2]) * t]
    };
    for tet in tets {
        let (ins, outs): (Vec<_>, Vec<_>) = tet.iter().partition(|(n, l)| f(*n, *l) < 0.0);
        let outwards = sub(centroid(outs.iter().map(|v| pos(*v))), centroid(ins.iter().map(|v| pos(*v))));
        let mut face = |tri: [[f64; 3]; 3]| {
            let n = cross3(sub(tri[1], tri[0]), sub(tri[2], tri[0]));
            emit(if dot(n, outwards) >= 0.0 { tri } else { [tri[0], tri[2], tri[1]] });
        };
        match (ins.as_slice(), outs.as_slice()) {
            ([lone], [a, b, c]) | ([a, b, c], [lone]) => face([point(*lone, *a), point(*lone, *b), point(*lone, *c)]),
            ([i0, i1], [o0, o1]) => {
                let q = [point(*i0, *o0), point(*i0, *o1), point(*i1, *o1), point(*i1, *o0)];
                face([q[0], q[1], q[2]]);
                face([q[0], q[2], q[3]]);
            }
            _ => {}
        }
    }
}

/// The rim's inner wall over `tri`: upright, where the field crosses `level`, from `bottom` to
/// `top`, facing the hollow (the lower values).
fn inner_wall(tri: [Node; 3], level: f64, bottom: f64, top: f64, emit: &mut impl FnMut([[f64; 3]; 3])) {
    let mut ends: Vec<[f64; 2]> = Vec::with_capacity(2);
    for k in 0..3 {
        let (a, b) = (tri[k], tri[(k + 1) % 3]);
        if (a.phi < level) != (b.phi < level) {
            ends.push(cross(a, b, level));
        }
    }
    let (Some(&p), Some(&q), Some(hollow)) = (ends.first(), ends.get(1), tri.iter().find(|n| n.phi < level)) else {
        return;
    };
    let mid = [(p[0] + q[0]) / 2.0, (p[1] + q[1]) / 2.0];
    let inwards = [hollow.p[0] - mid[0], hollow.p[1] - mid[1], 0.0];
    let (p0, q0, q1, p1) = ([p[0], p[1], bottom], [q[0], q[1], bottom], [q[0], q[1], top], [p[0], p[1], top]);
    for tri in [[p0, q0, q1], [p0, q1, p1]] {
        let n = cross3(sub(tri[1], tri[0]), sub(tri[2], tri[0]));
        emit(if dot(n, inwards) >= 0.0 { tri } else { [tri[0], tri[2], tri[1]] });
    }
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn centroid(points: impl Iterator<Item = [f64; 3]>) -> [f64; 3] {
    let (mut s, mut n) = ([0.0; 3], 0.0);
    for p in points {
        s = [s[0] + p[0], s[1] + p[1], s[2] + p[2]];
        n += 1.0;
    }
    if n > 0.0 { s.map(|c| c / n) } else { s }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Mesh;

    fn disc(radius: f64) -> Field2 {
        Field2::closed_discs(&[[0.0, 0.0]], radius, 0.0, 2.0, 3.0, 0.1).unwrap()
    }

    /// The raft's radius at height `z`: the farthest of its points within 0.01 mm of it.
    fn radius_at(t: &[[[f32; 3]; 3]], z: f64) -> f64 {
        t.iter().flatten().filter(|p| (p[2] as f64 - z).abs() < 0.01).map(|p| (p[0] as f64).hypot(p[1] as f64)).fold(0.0, f64::max)
    }

    fn volume(t: &[[[f32; 3]; 3]]) -> f64 {
        Mesh::new(t.to_vec()).signed_volume()
    }

    #[test]
    fn a_straight_raft_is_a_closed_disc_its_thickness() {
        let t = raft_solid(&disc(5.0), &RaftProfile { thickness: 1.0, slope: 0.0, rim_height: 0.0, rim_width: 1.0 });
        let want = std::f64::consts::PI * 25.0;
        assert!((volume(&t) - want).abs() < want * 0.01, "{} not {want}", volume(&t));
        let b = Mesh::new(t.clone()).bounds();
        assert!(b.min.z.abs() < 1e-6 && (b.max.z - 1.0).abs() < 1e-6);
        assert!((radius_at(&t, 0.0) - 5.0).abs() < 0.02 && (radius_at(&t, 1.0) - 5.0).abs() < 0.02);
    }

    #[test]
    fn a_chamfer_narrows_going_up_and_an_undercut_widens() {
        let chamfer = raft_solid(&disc(5.0), &RaftProfile { thickness: 1.0, slope: 1.0, rim_height: 0.0, rim_width: 1.0 });
        assert!((radius_at(&chamfer, 0.0) - 6.0).abs() < 0.02 && (radius_at(&chamfer, 1.0) - 5.0).abs() < 0.02);
        let undercut = raft_solid(&disc(5.0), &RaftProfile { thickness: 1.0, slope: -1.0, rim_height: 0.0, rim_width: 1.0 });
        assert!((radius_at(&undercut, 0.0) - 4.0).abs() < 0.02 && (radius_at(&undercut, 1.0) - 5.0).abs() < 0.02);
        // A cone frustum's volume: h π (R² + R r + r²) / 3.
        let want = std::f64::consts::PI * (36.0 + 30.0 + 25.0) / 3.0;
        assert!((volume(&chamfer) - want).abs() < want * 0.01, "{}", volume(&chamfer));
        assert!(volume(&undercut) > 0.0);
    }

    #[test]
    fn a_rim_rings_the_top_and_leaves_its_hollow_open() {
        let t = raft_solid(&disc(5.0), &RaftProfile { thickness: 1.0, slope: 0.0, rim_height: 2.0, rim_width: 1.0 });
        let pi = std::f64::consts::PI;
        let want = pi * 25.0 + 2.0 * pi * (25.0 - 16.0);
        assert!((volume(&t) - want).abs() < want * 0.01, "{} not {want}", volume(&t));
        assert!((Mesh::new(t.clone()).bounds().max.z - 3.0).abs() < 1e-6);
        let ring = t.iter().flatten().filter(|p| (p[2] - 3.0).abs() < 1e-6).map(|p| (p[0] as f64).hypot(p[1] as f64));
        assert!(ring.clone().all(|r| r > 3.98), "nothing on top inside the rim");
    }

    #[test]
    fn it_is_closed_wherever_it_merges_and_whatever_its_edge() {
        // A closed solid's volume does not depend on where it is measured from.
        let f = Field2::closed_discs(&[[0.0, 0.0], [3.0, 0.5], [8.0, 0.0], [4.0, 6.0]], 1.0, 3.0, 1.0, 3.0, 0.1).unwrap();
        for profile in [
            RaftProfile { thickness: 1.0, slope: 0.7, rim_height: 0.0, rim_width: 1.0 },
            RaftProfile { thickness: 1.0, slope: -0.5, rim_height: 1.5, rim_width: 0.6 },
            RaftProfile { thickness: 0.5, slope: 0.0, rim_height: 1.0, rim_width: 3.0 },
        ] {
            let t = raft_solid(&f, &profile);
            let moved: Vec<_> = t.iter().map(|tri| tri.map(|p| [p[0] + 100.0, p[1] - 50.0, p[2] + 30.0])).collect();
            let (v, w) = (volume(&t), volume(&moved));
            assert!(v > 0.0 && (v - w).abs() < v * 1e-4, "{profile:?}: {v} moved {w}");
        }
    }

    #[test]
    fn hostile_profiles_give_nothing() {
        let f = disc(2.0);
        for p in [
            RaftProfile { thickness: 0.0, slope: 0.0, rim_height: 0.0, rim_width: 0.0 },
            RaftProfile { thickness: f64::NAN, slope: 0.0, rim_height: 0.0, rim_width: 0.0 },
            RaftProfile { thickness: 1.0, slope: f64::INFINITY, rim_height: 0.0, rim_width: 0.0 },
        ] {
            assert!(raft_solid(&f, &p).is_empty());
        }
        let empty = Field2 { origin: [0.0; 2], cell: 0.1, nx: 3, ny: 3, values: vec![] };
        assert!(raft_solid(&empty, &RaftProfile { thickness: 1.0, slope: 0.0, rim_height: 0.0, rim_width: 0.0 }).is_empty());
    }
}
