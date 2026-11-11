//! A support's path from its tip to the build plate. The support touches a model only at its
//! tip: the body and the base keep `body.clearance` from every model, and no part of the body
//! leans more than `body.max_angle` from vertical. Supports may cross each other.
//!
//! The idea is the support cone of Vanek, Galicia and Benes, "Clever Support" (SGP 2014): the
//! places a straight strut from a point may go lie in a cone opening downwards from it, with the
//! steepest printable lean as its half-angle. Here each support stands alone. It goes straight
//! down when it can. Otherwise it leans along one direction of its cone, then drops straight
//! down beside whatever was below, choosing the shortest such path.
//!
//! A route is derived from the tip, the models' placement and the settings; it is never stored
//! in the scene. Once the user has placed a support's corners ([`Support::corners`]) its route
//! is those corners as they are, never searched again, only checked.
//!
//! A branch ([`Support::parent`]) is a tip alone: straight from its contact to its parent's head,
//! sharing the parent's body, with no body or joint of its own. A head is known without a route
//! ([`support_head`]), so a branch never waits on its parent's.

use resin_geom::{Aabb3, Affine3, Vec3};

use crate::{Model, Scene, Support, SupportPreset};

/// The polar steps of the cone search: no coarser than this, in degrees.
const POLAR_STEP: f64 = 5.0;
/// The azimuth steps of the cone search.
const AZIMUTHS: u32 = 24;
/// The march along a leaning direction never steps finer than this, in millimetres.
const MIN_STEP: f64 = 0.25;
/// How much a lean costs on top of its length, per radian: of two paths about as long, the one
/// nearer vertical wins.
const LEAN_COST: f64 = 0.1;
/// Where along the tip its pierce test starts, as a fraction of its length: the end that meets
/// the model is in the surface on purpose.
const TIP_FREE: f64 = 0.25;
/// A segment this close to a triangle crosses it.
const TOUCH: f64 = 1e-6;

/// Where a support goes.
#[derive(Clone, Debug, PartialEq)]
pub struct Route {
    /// Where the tip touches the model, in the scene.
    pub contact: Vec3,
    /// The tip's unit axis, from the contact away from the model.
    pub axis: Vec3,
    /// The tip's upper end, where the body starts.
    pub head: Vec3,
    /// The body's corners: `head`, the knee where it bends if it does, and its foot on the plate
    /// (Z = 0). Empty when no path reaches the plate. A branch's is its head alone: it has no
    /// body of its own.
    pub body: Vec<Vec3>,
    /// The body ends on the plate, on a base: not a branch's.
    pub foot: bool,
    /// How many clearance tests the search ran: what it cost.
    pub tests: u32,
    /// The user placed the corners, and they break a rule: a lean past `body.max_angle`, a body
    /// that goes up or comes within `body.clearance` of a model, a knee in the base.
    pub fault: bool,
}

impl Route {
    pub fn reachable(&self) -> bool {
        !self.body.is_empty()
    }
}

/// The models a support must keep clear of, each with the inverse of its placement.
struct Obstacles<'a> {
    models: Vec<(&'a Model, Affine3)>,
    tests: u32,
}

impl Obstacles<'_> {
    /// Whether segment `p q` keeps at least `r` from every model.
    fn clear(&mut self, p: Vec3, q: Vec3, r: f64) -> bool {
        self.tests = self.tests.saturating_add(1);
        let g = Vec3::new(r, r, r);
        let reach = Aabb3 { min: p.min(q) - g, max: p.max(q) + g };
        !self.models.iter().any(|(m, inv)| m.bounds.overlaps(&reach) && m.index.near_segment(&m.mesh, &m.transform, inv, p, q, r))
    }
}

/// The tip's axis: the surface normal, turned towards straight down until it leans no more than
/// `max_degrees` from it. `None` when no such axis leaves the surface (a face looking straight
/// up, a normal that is not a direction).
fn tip_axis(normal: Vec3, max_degrees: f64) -> Option<Vec3> {
    let n = normal.normalized()?;
    let max = max_degrees.to_radians();
    if n.dot(-Vec3::Z) >= max.cos() - 1e-12 {
        return Some(n);
    }
    let side = Vec3::new(n.x, n.y, 0.0).normalized()?;
    let axis = -Vec3::Z * max.cos() + side * max.sin();
    (axis.dot(n) > 0.0).then_some(axis)
}

/// Where support `s`'s tip meets its body under preset `p`, as [`route`] puts it: a branch's is
/// the head of the support at the root of its parents. `None` for a branch whose parent is gone.
pub fn support_head(scene: &Scene, s: &Support, p: &SupportPreset) -> Option<Vec3> {
    let mut s = s;
    // A parent comes before its branches, so the walk ends; the bound guards a broken scene.
    for _ in 0..=scene.supports().len() {
        let Some(parent) = s.parent else {
            return Some(match s.corners.as_ref().and_then(|c| c.first()) {
                Some(&h) => h,
                None => tip_axis(s.normal, p.tip.max_angle).map_or(s.tip, |axis| s.tip + axis * p.tip.length),
            });
        };
        s = scene.supports().iter().find(|t| t.id == parent)?;
    }
    None
}

/// The path of support `s` in `scene`, sized by preset `p`.
pub fn route(scene: &Scene, s: &Support, p: &SupportPreset) -> Route {
    let models = scene.models();
    if s.parent.is_some() {
        return branch(scene, s, p);
    }
    if let Some(corners) = &s.corners {
        return placed(models, s, corners, p);
    }
    let mut out = Route { contact: s.tip, axis: s.normal, head: s.tip, body: Vec::new(), tests: 0, fault: false, foot: true };
    let Some(axis) = tip_axis(s.normal, p.tip.max_angle) else { return out };
    let head = s.tip + axis * p.tip.length;
    out.axis = axis;
    out.head = head;
    let base_top = p.base.height;
    if !(head.is_finite() && head.z > base_top) {
        return out;
    }
    let mut obs = Obstacles { models: models.iter().filter_map(|m| Some((m, m.transform.inverse()?))).collect(), tests: 0 };
    let body_r = p.body.diameter / 2.0 + p.body.clearance;
    let base_r = p.base.diameter / 2.0 + p.body.clearance;
    let tip_clear = obs.clear(s.tip + axis * (p.tip.length * TIP_FREE), head, TOUCH);
    // Straight down from `k` to the plate, with the base around the foot. The base is a short,
    // wide cone; a capsule of its radius ending at its top holds it.
    let drop = |obs: &mut Obstacles, k: Vec3| {
        let foot = Vec3::new(k.x, k.y, 0.0);
        let clear = obs.clear(k, foot, body_r) && obs.clear(foot, foot + Vec3::Z * (base_top - base_r).max(0.0), base_r);
        clear.then_some(foot)
    };
    if !tip_clear {
        out.tests = obs.tests;
        return out;
    }
    if let Some(foot) = drop(&mut obs, head) {
        out.body = vec![head, foot];
        out.tests = obs.tests;
        return out;
    }

    let max = p.body.max_angle.to_radians();
    let polar_steps = (p.body.max_angle / POLAR_STEP).ceil().max(0.0) as u32;
    let step = p.body.diameter.max(MIN_STEP);
    // (cost, knee, foot)
    let mut best: Option<(f64, Vec3, Vec3)> = None;
    for i in 1..=polar_steps {
        let theta = max * f64::from(i) / f64::from(polar_steps);
        let (sin, cos) = theta.sin_cos();
        // The knee stays above the base.
        let end = (head.z - base_top) / cos;
        for j in 0..AZIMUTHS {
            let phi = std::f64::consts::TAU * f64::from(j) / f64::from(AZIMUTHS);
            let d = Vec3::new(sin * phi.cos(), sin * phi.sin(), -cos);
            let (mut prev, mut along) = (head, 0.0);
            while along < end {
                along = (along + step).min(end);
                let knee = head + d * along;
                // The length grows along `d`: past the best so far, nothing further can win.
                let length = along + knee.z;
                if best.is_some_and(|(c, ..)| length >= c) {
                    break;
                }
                if !obs.clear(prev, knee, body_r) {
                    break;
                }
                if let Some(foot) = drop(&mut obs, knee) {
                    let cost = length * (1.0 + LEAN_COST * theta);
                    if best.is_none_or(|(c, ..)| cost < c) {
                        best = Some((cost, knee, foot));
                    }
                    break;
                }
                prev = knee;
            }
        }
    }
    if let Some((_, knee, foot)) = best {
        out.body = vec![head, knee, foot];
    }
    out.tests = obs.tests;
    out
}

/// The corners support `s`'s body has, to keep as placed corners: those placed, or its route's
/// but the foot; its head alone when no path reaches the plate.
pub(crate) fn placed_corners(scene: &Scene, s: &Support, p: &SupportPreset) -> Vec<Vec3> {
    if let Some(c) = &s.corners {
        return c.clone();
    }
    let r = route(scene, s, p);
    match r.body.split_last() {
        Some((_, c)) if !c.is_empty() => c.to_vec(),
        _ => vec![r.head],
    }
}

/// A branch: its tip straight from its contact to the head it shares, flagged when it does not
/// leave the surface or goes through a model. Placed corners are ignored: it has no joint to place.
fn branch(scene: &Scene, s: &Support, p: &SupportPreset) -> Route {
    let mut out = Route { contact: s.tip, axis: s.normal, head: s.tip, body: Vec::new(), tests: 0, fault: false, foot: false };
    let Some(head) = support_head(scene, s, p) else { return out };
    let Some(axis) = (head - s.tip).normalized() else { return out };
    let mut obs = Obstacles { models: scene.models().iter().filter_map(|m| Some((m, m.transform.inverse()?))).collect(), tests: 0 };
    let leaves = s.normal.normalized().is_some_and(|n| axis.dot(n) > 0.0);
    out.fault = !(leaves && obs.clear(s.tip + axis * (s.tip.dist(head) * TIP_FREE), head, TOUCH));
    out.axis = axis;
    out.head = head;
    out.body = vec![head];
    out.tests = obs.tests;
    out
}

/// The route through the corners the user placed, checked against the rules the search keeps.
fn placed(models: &[Model], s: &Support, corners: &[Vec3], p: &SupportPreset) -> Route {
    let mut out = Route { contact: s.tip, axis: s.normal, head: s.tip, body: Vec::new(), tests: 0, fault: true, foot: true };
    let (Some(&head), Some(&last)) = (corners.first(), corners.last()) else { return out };
    out.axis = (head - s.tip).normalized().unwrap_or(s.normal);
    out.head = head;
    out.body = corners.iter().copied().chain(std::iter::once(Vec3::new(last.x, last.y, 0.0))).collect();
    let mut obs = Obstacles { models: models.iter().filter_map(|m| Some((m, m.transform.inverse()?))).collect(), tests: 0 };
    let body_r = p.body.diameter / 2.0 + p.body.clearance;
    let base_r = p.base.diameter / 2.0 + p.body.clearance;
    let base_top = p.base.height;
    let steep = p.body.max_angle.to_radians().cos() - 1e-9;
    let leans_ok = out.body.windows(2).all(|w| match w {
        [a, b] => a.dist(*b) <= TOUCH || (a.z - b.z) >= steep * a.dist(*b),
        _ => true,
    });
    let foot = Vec3::new(last.x, last.y, 0.0);
    let clear = obs.clear(s.tip + out.axis * (s.tip.dist(head) * TIP_FREE), head, TOUCH)
        && out.body.windows(2).all(|w| match w {
            [a, b] => obs.clear(*a, *b, body_r),
            _ => true,
        })
        && obs.clear(foot, foot + Vec3::Z * (base_top - base_r).max(0.0), base_r);
    out.fault = !(leans_ok && clear && last.z > base_top);
    out.tests = obs.tests;
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tip_leans_no_more_than_allowed() {
        let down = tip_axis(-Vec3::Z, 40.0).unwrap();
        assert_eq!(down, -Vec3::Z);
        let wall = tip_axis(Vec3::X, 40.0).unwrap();
        assert!((wall.dot(-Vec3::Z) - 40f64.to_radians().cos()).abs() < 1e-12);
        assert!(wall.x > 0.0 && wall.y.abs() < 1e-12);
        // Within the lean: the normal as it is.
        let n = Vec3::new(0.3, 0.0, -1.0).normalized().unwrap();
        assert!(tip_axis(n, 40.0).unwrap().dist(n) < 1e-12);
        assert_eq!(tip_axis(Vec3::Z, 40.0), None);
        assert_eq!(tip_axis(Vec3::ZERO, 40.0), None);
        assert_eq!(tip_axis(Vec3::new(f64::NAN, 0.0, 0.0), 40.0), None);
        // A face looking up and out gets an axis only while it still leaves the face.
        assert!(tip_axis(Vec3::new(1.0, 0.0, 0.2).normalized().unwrap(), 40.0).is_some());
        assert_eq!(tip_axis(Vec3::new(1.0, 0.0, 3.0).normalized().unwrap(), 40.0), None);
    }
}
