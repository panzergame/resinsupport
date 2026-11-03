// Ported from SolidCraft (MIT OR Apache-2.0), crates/geom/src/bvh.rs at 4bdffef0; see
// ATTRIBUTION.md. Reduced to ray queries; `MeshIndex` works on our triangle soup.
//! A bounding volume hierarchy over boxes: the nearest hit along a ray without looking at each
//! item. Built once (median split on the widest axis of the box centres, a few items per leaf)
//! and only read afterwards. [`MeshIndex`] holds one over a mesh's triangles, for picking.

use crate::{Aabb3, Mesh, RayHit, Vec3};

/// Most items in a leaf.
const LEAF: usize = 4;

#[derive(Clone, Copy, Debug)]
struct Node {
    bounds: Aabb3,
    /// A leaf: the first of its `count` items in [`Bvh::order`]. Otherwise its children are
    /// `first` and `first + 1`.
    first: u32,
    count: u32,
}

/// A hierarchy over boxes; items are the boxes' indices.
#[derive(Clone, Debug, Default)]
pub struct Bvh {
    nodes: Vec<Node>,
    order: Vec<u32>,
}

impl Bvh {
    /// The hierarchy over `boxes` (empty boxes are left out).
    pub fn new(boxes: &[Aabb3]) -> Bvh {
        let mut order: Vec<u32> =
            (0..boxes.len().min(u32::MAX as usize)).filter(|&i| boxes.get(i).is_some_and(|b| !b.is_empty() && finite(b))).map(|i| i as u32).collect();
        if order.is_empty() {
            return Bvh::default();
        }
        let item = |i: u32| boxes.get(i as usize).copied().unwrap_or(Aabb3::EMPTY);
        let centre = |i: u32| item(i).center();
        let mut nodes = vec![Node { bounds: Aabb3::EMPTY, first: 0, count: 0 }];
        // (node, start, end) in `order` still to be filled in.
        let mut todo = vec![(0usize, 0usize, order.len())];
        while let Some((n, start, end)) = todo.pop() {
            let Some(span) = order.get_mut(start..end) else { continue };
            let mut bounds = Aabb3::EMPTY;
            let mut centres = Aabb3::EMPTY;
            for &i in span.iter() {
                bounds = bounds.union(&item(i));
                centres.add(centre(i));
            }
            let size = centres.size();
            let axis = if size.x >= size.y && size.x >= size.z {
                0
            } else if size.y >= size.z {
                1
            } else {
                2
            };
            let extent = [size.x, size.y, size.z][axis];
            if span.len() <= LEAF || !(extent > 0.0) {
                if let Some(node) = nodes.get_mut(n) {
                    *node = Node { bounds, first: start as u32, count: span.len() as u32 };
                }
                continue;
            }
            let key = |i: &u32| {
                let c = centre(*i);
                [c.x, c.y, c.z][axis]
            };
            let mid = span.len() / 2;
            span.select_nth_unstable_by(mid, |a, b| key(a).total_cmp(&key(b)));
            let left = nodes.len();
            nodes.push(Node { bounds: Aabb3::EMPTY, first: 0, count: 0 });
            nodes.push(Node { bounds: Aabb3::EMPTY, first: 0, count: 0 });
            if let Some(node) = nodes.get_mut(n) {
                *node = Node { bounds, first: left as u32, count: 0 };
            }
            todo.push((left, start, start + mid));
            todo.push((left + 1, start + mid, end));
        }
        Bvh { nodes, order }
    }

    /// The box around everything (empty for no items).
    pub fn bounds(&self) -> Aabb3 {
        self.nodes.first().map_or(Aabb3::EMPTY, |n| n.bounds)
    }

    /// The nearest item along the ray from `o` along `d`: `hit` gives an item's distance along
    /// `d` (`None`: missed). Equal distances go to the lowest item, as a scan in item order
    /// keeping strictly nearer hits would answer. Boxes are entered nearest first, and one that
    /// starts past the best hit so far is skipped.
    pub fn nearest(&self, o: Vec3, d: Vec3, mut hit: impl FnMut(u32) -> Option<f64>) -> Option<(f64, u32)> {
        if !o.is_finite() || !d.is_finite() {
            return None;
        }
        let inv = Vec3::new(1.0 / d.x, 1.0 / d.y, 1.0 / d.z);
        let mut best: Option<(f64, u32)> = None;
        let root = self.nodes.first().and_then(|n| ray_box(o, d, inv, &n.bounds))?;
        let mut stack = vec![(0usize, root)];
        while let Some((n, t0)) = stack.pop() {
            if best.is_some_and(|(bt, _)| t0 > bt) {
                continue;
            }
            let Some(node) = self.nodes.get(n) else { continue };
            if node.count > 0 {
                let (a, b) = (node.first as usize, node.first as usize + node.count as usize);
                for &i in self.order.get(a..b).unwrap_or(&[]) {
                    if let Some(t) = hit(i)
                        && best.is_none_or(|(bt, bi)| t < bt || (t == bt && i < bi))
                    {
                        best = Some((t, i));
                    }
                }
                continue;
            }
            let (l, r) = (node.first as usize, node.first as usize + 1);
            let tl = self.nodes.get(l).and_then(|c| ray_box(o, d, inv, &c.bounds));
            let tr = self.nodes.get(r).and_then(|c| ray_box(o, d, inv, &c.bounds));
            // The nearer child is popped first.
            match (tl, tr) {
                (Some(a), Some(b)) if a <= b => stack.extend([(r, b), (l, a)]),
                (Some(a), Some(b)) => stack.extend([(l, a), (r, b)]),
                (Some(a), None) => stack.push((l, a)),
                (None, Some(b)) => stack.push((r, b)),
                (None, None) => {}
            }
        }
        best
    }
}

fn finite(b: &Aabb3) -> bool {
    b.min.is_finite() && b.max.is_finite()
}

/// Where the ray from `o` along `d` (`inv` = 1 / `d` per axis) enters box `b`, from 0 on; `None`
/// when it misses or the box lies behind.
fn ray_box(o: Vec3, d: Vec3, inv: Vec3, b: &Aabb3) -> Option<f64> {
    let (mut t0, mut t1) = (0.0f64, f64::INFINITY);
    for (o, d, inv, lo, hi) in [(o.x, d.x, inv.x, b.min.x, b.max.x), (o.y, d.y, inv.y, b.min.y, b.max.y), (o.z, d.z, inv.z, b.min.z, b.max.z)] {
        if d == 0.0 || !inv.is_finite() {
            if o < lo || o > hi {
                return None;
            }
            continue;
        }
        let (a, b) = ((lo - o) * inv, (hi - o) * inv);
        let (a, b) = if a <= b { (a, b) } else { (b, a) };
        t0 = t0.max(a);
        t1 = t1.min(b);
        if t0 > t1 {
            return None;
        }
    }
    (t0 <= t1).then_some(t0)
}

/// A mesh's triangles in a hierarchy, for picking: the nearest triangle along a ray. Built from
/// one mesh and answers only for it.
#[derive(Clone, Debug, Default)]
pub struct MeshIndex {
    triangles: Bvh,
    triangle_count: usize,
}

impl MeshIndex {
    pub fn new(mesh: &Mesh) -> MeshIndex {
        let boxes: Vec<Aabb3> = mesh
            .triangles
            .iter()
            .map(|t| {
                let mut b: Aabb3 = t.iter().map(|&p| Vec3::from(p)).collect();
                if !b.is_empty() {
                    // Flat boxes (a triangle square to an axis) are grown a little, so a ray
                    // through them is never lost to rounding in the box test.
                    let scale = [b.min.x, b.min.y, b.min.z, b.max.x, b.max.y, b.max.z].iter().fold(1.0f64, |m, v| m.max(v.abs()));
                    let e = scale * 1e-9;
                    b.min -= Vec3::new(e, e, e);
                    b.max += Vec3::new(e, e, e);
                }
                b
            })
            .collect();
        MeshIndex { triangles: Bvh::new(&boxes), triangle_count: mesh.triangles.len() }
    }

    /// [`Mesh::ray_hit`] through the index: the same answer, including which triangle wins a
    /// tie. For a mesh other than the one indexed, the plain scan answers.
    pub fn ray_hit(&self, mesh: &Mesh, origin: Vec3, dir: Vec3) -> Option<RayHit> {
        if mesh.triangles.len() != self.triangle_count {
            return mesh.ray_hit(origin, dir);
        }
        let frame = crate::ray::RayFrame::new(origin, dir)?;
        self.triangles.nearest(origin, dir, |i| mesh.triangles.get(i as usize).and_then(|t| frame.hit(t.map(Vec3::from)))).map(|(t, i)| RayHit {
            t,
            point: origin + dir * t,
            triangle: i as usize,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::tests::cube;

    #[test]
    fn index_answers_as_the_scan_does() {
        // A row of cubes, so the hierarchy has several levels.
        let mut tris = Vec::new();
        for k in 0..20 {
            let off = k as f32 * 15.0;
            tris.extend(cube(10.0).triangles.iter().map(|t| t.map(|[x, y, z]| [x + off, y, z])));
        }
        let mesh = Mesh::new(tris);
        let index = MeshIndex::new(&mesh);
        for i in 0..200 {
            let x = f64::from(i) * 1.6 - 5.0;
            for (o, d) in [
                (Vec3::new(x, 5.0, 50.0), -Vec3::Z),
                (Vec3::new(x, -50.0, 5.0), Vec3::Y),
                (Vec3::new(-20.0, 5.0, 5.0), Vec3::new(1.0, 0.0, 0.01 * f64::from(i)).normalized().unwrap()),
            ] {
                assert_eq!(index.ray_hit(&mesh, o, d), mesh.ray_hit(o, d), "{o:?} {d:?}");
            }
        }
        assert!(index.ray_hit(&mesh, Vec3::new(f64::NAN, 0.0, 0.0), Vec3::Z).is_none());
        assert!(index.ray_hit(&mesh, Vec3::ZERO, Vec3::ZERO).is_none());
    }

    #[test]
    fn empty_and_junk_meshes_have_no_hits() {
        let empty = Mesh::default();
        assert!(MeshIndex::new(&empty).ray_hit(&empty, Vec3::ZERO, Vec3::Z).is_none());
        let junk = Mesh::new(vec![[[f32::NAN, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]; 10]);
        assert!(MeshIndex::new(&junk).ray_hit(&junk, Vec3::new(0.2, 0.2, 5.0), -Vec3::Z).is_none());
    }
}
