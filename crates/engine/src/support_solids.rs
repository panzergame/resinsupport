//! The solids a support is made of, sized by its preset: one description for the viewport, which
//! draws them, and for the slicer, which prints them, so what is printed is what is drawn.

use resin_geom::{ShadedVertex, Vec3, frustum, unit_sphere};

use crate::{BodyShape, ContactShape, Route, SupportPart, SupportPreset};

/// One solid of a support.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Primitive {
    /// A cone, cylinder or prism from `a` (radius `ra`) to `b` (radius `rb`) with `sides` sides;
    /// `smooth` shades it round. `caps` are the ends drawn closed: inner ends are left open on
    /// screen, where a joint or another part covers them.
    Frustum {
        a: Vec3,
        b: Vec3,
        ra: f64,
        rb: f64,
        sides: u32,
        smooth: bool,
        caps: (bool, bool),
    },
    Sphere {
        center: Vec3,
        radius: f64,
    },
}

/// What a solid of a support belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Owner {
    /// The tip and its round contact: what selecting the support lights up.
    Tip,
    Base,
    /// A body column or the joint at a bend.
    Part(SupportPart),
}

/// Each solid of route `r` sized by `p`, with what it belongs to.
pub fn support_primitives(r: &Route, p: &SupportPreset, mut f: impl FnMut(Owner, Primitive)) {
    let sides = p.body.segments;
    let tip_start = r.contact - r.axis * p.tip.penetration;
    f(
        Owner::Tip,
        Primitive::Frustum {
            a: tip_start,
            b: r.head,
            ra: p.tip.contact_diameter / 2.0,
            rb: p.tip.upper_diameter / 2.0,
            sides,
            smooth: true,
            caps: (true, true),
        },
    );
    if p.tip.contact_shape == ContactShape::Sphere {
        f(Owner::Tip, Primitive::Sphere { center: r.contact, radius: p.tip.contact_diameter / 2.0 });
    }
    let radius = p.body.diameter / 2.0;
    let smooth = p.body.shape == BodyShape::Cylinder;
    for (w, i) in r.body.windows(2).zip(0u32..) {
        if let [a, b] = w {
            f(Owner::Part(SupportPart::Segment(i)), Primitive::Frustum { a: *a, b: *b, ra: radius, rb: radius, sides, smooth, caps: (false, false) });
        }
    }
    // Every corner but the foot.
    let corners = r.body.split_last().map(|(_, c)| c).unwrap_or_default();
    for (c, i) in corners.iter().zip(0u32..) {
        f(Owner::Part(SupportPart::Joint(i)), Primitive::Sphere { center: *c, radius });
    }
    if let Some(foot) = r.body.last().filter(|_| r.foot) {
        let top = *foot + Vec3::Z * p.base.height;
        f(Owner::Base, Primitive::Frustum { a: *foot, b: top, ra: p.base.diameter / 2.0, rb: radius, sides, smooth, caps: (true, false) });
    }
}

/// Turns support solids into closed triangle soups for the slicer.
pub struct SupportTessellator {
    sphere: Vec<[[f32; 3]; 3]>,
    scratch: Vec<ShadedVertex>,
}

/// The sphere's subdivisions: as drawn.
const SPHERE_DETAIL: u32 = 2;

impl Default for SupportTessellator {
    fn default() -> Self {
        SupportTessellator { sphere: unit_sphere(SPHERE_DETAIL), scratch: Vec::new() }
    }
}

impl SupportTessellator {
    /// Route `r`'s solids sized by `p`, each closed (both caps on), appended to `out`.
    pub fn triangles(&mut self, r: &Route, p: &SupportPreset, out: &mut Vec<[[f32; 3]; 3]>) {
        let sphere = &self.sphere;
        let scratch = &mut self.scratch;
        support_primitives(r, p, |_, prim| match prim {
            Primitive::Frustum { a, b, ra, rb, sides, smooth, .. } => {
                scratch.clear();
                frustum(a, b, ra, rb, sides, smooth, (true, true), scratch);
                out.extend(scratch.chunks_exact(3).map(|t| [t[0].pos, t[1].pos, t[2].pos]));
            }
            Primitive::Sphere { center, radius } => {
                if !(radius > 0.0) || !center.is_finite() {
                    return;
                }
                let at = |v: [f32; 3]| (center + Vec3::from(v) * radius).to_f32();
                out.extend(sphere.iter().map(|t| t.map(at)));
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PresetId;
    use resin_geom::Mesh;

    #[test]
    fn a_support_prints_as_closed_solids_that_reach_the_plate() {
        let r = Route {
            contact: Vec3::new(0.0, 0.0, 10.0),
            axis: -Vec3::Z,
            head: Vec3::new(0.0, 0.0, 8.0),
            body: vec![Vec3::new(0.0, 0.0, 8.0), Vec3::new(2.0, 0.0, 5.0), Vec3::new(2.0, 0.0, 0.0)],
            tests: 0,
            fault: false,
            foot: true,
        };
        let p = PresetId::Medium.defaults();
        let mut parts = Vec::new();
        support_primitives(&r, &p, |part, _| parts.push(part));
        assert!(parts.contains(&Owner::Part(SupportPart::Segment(1))) && parts.contains(&Owner::Part(SupportPart::Joint(1))));
        assert!(!parts.contains(&Owner::Part(SupportPart::Joint(2))), "the foot has no joint");
        assert!(parts.contains(&Owner::Tip) && parts.contains(&Owner::Base));
        let mut t = Vec::new();
        SupportTessellator::default().triangles(&r, &p, &mut t);
        let mesh = Mesh::new(t);
        assert!(mesh.signed_volume() > 0.0, "wound outwards");
        assert!(mesh.bounds().min.z.abs() < 1e-6, "the base stands on the plate");
    }
}
