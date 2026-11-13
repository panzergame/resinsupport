//! Braces: struts between the bodies of two supports, in a zig-zag or crossed in each bay, that
//! stiffen tall supports against the peel. A brace stores its two supports and its own settings;
//! its struts are worked out from the supports' routes each time, never stored, as routes are.
//! The braced span runs from `foot_gap` above the top of the base cones to `head_gap` below the
//! lower head, cut into sections at the bodies' corners (knees). Within a section both bodies are
//! straight, and its struts rise at `angle` from horizontal as measured in the section's plane,
//! the one holding the gap and the bodies' direction; made only steeper, so that a whole number
//! of bays fills each section. A strut that would come within `body.clearance` of a model is
//! left out. Editing a braced support (its tip, its joints, its route) removes its braces: the
//! user makes them again on the new bodies.

use resin_geom::Vec3;

use crate::route::Obstacles;
use crate::{Route, Scene, SupportPreset};

pub use resin_io::BracePattern;

/// How the selected supports are paired into braces.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BracePairing {
    /// From the support nearest the -X, -Y corner, each to the nearest one not yet visited.
    Chain,
    /// The shortest links that join them all: a minimum spanning tree.
    Tree,
    /// Every pair.
    All,
}

impl BracePairing {
    pub const ALL: [BracePairing; 3] = [BracePairing::Chain, BracePairing::Tree, BracePairing::All];
    pub fn id(self) -> &'static str {
        match self {
            BracePairing::Chain => "chain",
            BracePairing::Tree => "tree",
            BracePairing::All => "all",
        }
    }
    pub fn parse(s: &str) -> Option<BracePairing> {
        BracePairing::ALL.into_iter().find(|p| p.id() == s)
    }
}

/// A brace's own settings, kept with it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BraceParams {
    pub pattern: BracePattern,
    /// Degrees from horizontal: the struts' least slope.
    pub angle: f64,
    /// The struts' diameter as a fraction of the body's.
    pub diameter_ratio: f64,
    /// Millimetres left free below the lower head.
    pub head_gap: f64,
    /// Millimetres left free above the top of the base cones.
    pub foot_gap: f64,
}

pub const ANGLE_RANGE: std::ops::RangeInclusive<f64> = 15.0..=80.0;
pub const DIAMETER_RATIO_RANGE: std::ops::RangeInclusive<f64> = 0.1..=1.0;
pub const GAP_RANGE: std::ops::RangeInclusive<f64> = 0.0..=100.0;
/// How far apart, in mm, two supports may stand and still be braced.
pub const MAX_DISTANCE_RANGE: std::ops::RangeInclusive<f64> = 1.0..=100.0;
pub const DEFAULT_MAX_DISTANCE: f64 = 10.0;
/// The most bays a brace is cut into: supports nearly touching would otherwise ask for thousands.
pub const MAX_BAYS: usize = 100;
/// The most braces made at once: every pair of a big box selection is a lot.
pub const MAX_BRACES: usize = 10_000;

impl Default for BraceParams {
    fn default() -> Self {
        BraceParams { pattern: BracePattern::ZigZag, angle: 45.0, diameter_ratio: 0.5, head_gap: 2.0, foot_gap: 1.0 }
    }
}

impl BraceParams {
    pub fn validate(&self) -> std::result::Result<(), String> {
        let check = |name: &str, v: f64, r: &std::ops::RangeInclusive<f64>| {
            if r.contains(&v) { Ok(()) } else { Err(format!("`{name}` must be a number from {} to {}", r.start(), r.end())) }
        };
        check("angle", self.angle, &ANGLE_RANGE)?;
        check("diameter_ratio", self.diameter_ratio, &DIAMETER_RATIO_RANGE)?;
        check("head_gap", self.head_gap, &GAP_RANGE)?;
        check("foot_gap", self.foot_gap, &GAP_RANGE)
    }
}

/// Struts between supports `a` and `b`, by id: always `a < b`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Brace {
    pub id: u64,
    pub a: u64,
    pub b: u64,
    pub params: BraceParams,
}

/// The point of `body` (head first, going down) at height `z`.
fn at_height(body: &[Vec3], z: f64) -> Option<Vec3> {
    body.windows(2).find_map(|w| match w {
        [p, q] if p.z >= z && z >= q.z => {
            let drop = p.z - q.z;
            Some(if drop > 0.0 { *p + (*q - *p) * ((p.z - z) / drop) } else { *p })
        }
        _ => None,
    })
}

fn flat_dist(p: Vec3, q: Vec3) -> f64 {
    (p.x - q.x).hypot(p.y - q.y)
}

/// The plane a section's struts are measured in: it holds the horizontal gap direction `u` and
/// the bodies' mean direction, so `up` is the bodies' slope within it. Exact when both segments lie
/// in one plane (parallel or meeting), the nearest fit when they are skew.
#[derive(Clone, Copy, Debug)]
struct SectionPlane {
    u: Vec3,
    up: Vec3,
}

impl SectionPlane {
    /// The plane of a section whose bodies run from `a0` to `a1` and from `b0` to `b1`, upwards.
    fn of(a0: Vec3, a1: Vec3, b0: Vec3, b1: Vec3) -> Option<SectionPlane> {
        let gap = (b0 + b1 - a0 - a1) * 0.5;
        let u = Vec3::new(gap.x, gap.y, 0.0).normalized()?;
        let along = ((a1 - a0).normalized()? + (b1 - b0).normalized()?).normalized()?;
        // Bodies leaning along the gap leave the plane vertical, with `along` in it.
        let up = match u.cross(along).normalized() {
            Some(n) => n.cross(u),
            None => Vec3::Z,
        };
        let up = if up.z < 0.0 { -up } else { up };
        Some(SectionPlane { u, up })
    }
    /// Whether strut `s e` rises at `slope` or steeper within the plane.
    fn steep_enough(&self, s: Vec3, e: Vec3, slope: f64) -> bool {
        let v = e - s;
        v.dot(self.up) >= v.dot(self.u).abs() * slope - 1e-9
    }
}

/// The struts of `bays` equal bays from height `z0` to `z1`, the first bay's zig-zag strut
/// starting on `a` when `parity` is even, so a zig-zag runs on unbroken from the section below.
fn section_struts(a: &[Vec3], b: &[Vec3], (z0, z1): (f64, f64), bays: usize, parity: usize, pattern: BracePattern) -> Option<Vec<(Vec3, Vec3)>> {
    let level = |k: usize| z0 + (z1 - z0) * k as f64 / bays as f64;
    let mut out = Vec::with_capacity(bays * 2);
    for k in 0..bays {
        let (lo, hi) = (level(k), level(k + 1));
        let (a0, a1, b0, b1) = (at_height(a, lo)?, at_height(a, hi)?, at_height(b, lo)?, at_height(b, hi)?);
        match pattern {
            BracePattern::ZigZag if (parity + k).is_multiple_of(2) => out.push((a0, b1)),
            BracePattern::ZigZag => out.push((b0, a1)),
            BracePattern::Cross => out.extend([(a0, b1), (b0, a1)]),
        }
    }
    Some(out)
}

/// The most bays from height `z0` to `z1`, at most `budget`, whose struts all rise at `slope` or
/// steeper in the section's plane, and those struts; `None` when not even one bay does.
fn fit_section(
    a: &[Vec3],
    b: &[Vec3],
    (z0, z1): (f64, f64),
    slope: f64,
    budget: usize,
    parity: usize,
    pattern: BracePattern,
) -> Option<(usize, Vec<(Vec3, Vec3)>)> {
    let (a0, a1, b0, b1) = (at_height(a, z0)?, at_height(a, z1)?, at_height(b, z0)?, at_height(b, z1)?);
    let plane = SectionPlane::of(a0, a1, b0, b1)?;
    // Within a section both bodies are straight, so the gap along `u` is widest at an end.
    let widest = (b0 - a0).dot(plane.u).abs().max((b1 - a1).dot(plane.u).abs());
    let height = ((a1 - a0).dot(plane.up) + (b1 - b0).dot(plane.up)) / 2.0;
    let estimate = (height / (widest * slope)).floor();
    // Rounding down makes each bay taller: the struts only ever get steeper. A strut is longer
    // along `u` than the gap at either of its heights where the bodies lean, hence the check.
    let mut bays = if estimate.is_finite() && estimate > 0.0 { (estimate as usize).min(budget) } else { 0 };
    while bays > 0 {
        let struts = section_struts(a, b, (z0, z1), bays, parity, pattern)?;
        if struts.iter().all(|(s, e)| plane.steep_enough(*s, *e, slope)) {
            return Some((bays, struts));
        }
        bays -= 1;
    }
    None
}

/// The struts between routes `a` and `b` under `params`, sized by preset `p`, before any is
/// left out for a model. The span is cut into sections at the bodies' corners, so both bodies
/// are straight within each; a section's struts are measured in its own plane
/// ([`SectionPlane`]), and a section too short for one bay joins the one above it (the last,
/// the one below). None when either route has no body to the plate, the span is too short, or
/// the bodies come within a body's width of each other.
pub fn brace_struts(a: &Route, b: &Route, params: &BraceParams, p: &SupportPreset) -> Vec<(Vec3, Vec3)> {
    let (Some(ha), Some(hb)) = (a.body.first(), b.body.first()) else { return Vec::new() };
    if !(a.foot && b.foot && a.body.len() >= 2 && b.body.len() >= 2) || params.validate().is_err() {
        return Vec::new();
    }
    let lo = p.base.height + params.foot_gap;
    let hi = ha.z.min(hb.z) - params.head_gap;
    if !(hi - lo > 0.0) {
        return Vec::new();
    }
    let mut cuts: Vec<f64> = a.body.iter().chain(&b.body).map(|c| c.z).filter(|z| *z > lo && *z < hi).chain([lo, hi]).collect();
    cuts.sort_by(f64::total_cmp);
    cuts.dedup_by(|x, y| (*x - *y).abs() < 1e-9);
    // Between corners both bodies are straight, so they come nearest at a corner or an end.
    let near = cuts.iter().any(|z| match (at_height(&a.body, *z), at_height(&b.body, *z)) {
        (Some(pa), Some(pb)) => flat_dist(pa, pb) < p.body.diameter,
        _ => true,
    });
    if near {
        return Vec::new();
    }
    let slope = params.angle.to_radians().tan();
    let (a, b, pattern) = (&a.body, &b.body, params.pattern);
    // Each made section: its heights, the bays below it (the zig-zag's parity) and its struts.
    let mut sections: Vec<((f64, f64), usize, Vec<(Vec3, Vec3)>)> = Vec::new();
    let mut bays = 0;
    let mut from = lo;
    for &to in cuts.iter().skip(1) {
        if let Some((n, struts)) = fit_section(a, b, (from, to), slope, MAX_BAYS.saturating_sub(bays), bays, pattern) {
            sections.push(((from, to), bays, struts));
            bays += n;
            from = to;
        }
    }
    // A top too short for a bay joins the section below it.
    if from < hi
        && let Some(((start, _), parity, _)) = sections.last()
        && let Some((_, struts)) = fit_section(a, b, (*start, hi), slope, MAX_BAYS.saturating_sub(*parity), *parity, pattern)
        && let Some(last) = sections.last_mut()
    {
        *last = ((last.0.0, hi), last.1, struts);
    }
    sections.into_iter().flat_map(|(.., struts)| struts).collect()
}

/// The `struts` that keep the clearance from every model in `scene`, and how many clearance tests
/// that took.
pub fn clear_struts(scene: &Scene, struts: Vec<(Vec3, Vec3)>, params: &BraceParams, p: &SupportPreset) -> (Vec<(Vec3, Vec3)>, u32) {
    let mut obs = Obstacles::of(scene.models());
    let r = p.body.diameter * params.diameter_ratio / 2.0 + p.body.clearance;
    let kept = struts.into_iter().filter(|(s, e)| obs.clear(*s, *e, r)).collect();
    (kept, obs.tests)
}

/// The struts a brace with `params` has in `scene` between routes `ra` and `rb`, and the tests run.
pub fn brace_solids(scene: &Scene, ra: &Route, rb: &Route, params: &BraceParams, p: &SupportPreset) -> (Vec<(Vec3, Vec3)>, u32) {
    clear_struts(scene, brace_struts(ra, rb, params, p), params, p)
}

/// The pairs to brace among `points` (a support id and where its body stands on the plate), at
/// most `max_distance` apart, each pair `(lower id, higher id)`, in a stable order.
pub fn brace_pairs(points: &[(u64, Vec3)], pairing: BracePairing, max_distance: f64) -> Vec<(u64, u64)> {
    let mut pts: Vec<(u64, Vec3)> = points.to_vec();
    pts.sort_by_key(|(id, _)| *id);
    pts.dedup_by_key(|(id, _)| *id);
    let near = |i: usize, j: usize| match (pts.get(i), pts.get(j)) {
        (Some(a), Some(b)) => Some(flat_dist(a.1, b.1)).filter(|d| *d <= max_distance),
        _ => None,
    };
    let pair = |i: usize, j: usize| match (pts.get(i), pts.get(j)) {
        (Some(a), Some(b)) => Some((a.0.min(b.0), a.0.max(b.0))),
        _ => None,
    };
    let n = pts.len();
    let mut out = Vec::new();
    match pairing {
        BracePairing::All => {
            for i in 0..n {
                for j in i + 1..n {
                    if near(i, j).is_some() {
                        out.extend(pair(i, j));
                    }
                }
            }
        }
        BracePairing::Chain => {
            let corner = |v: &Vec3| v.x + v.y;
            let Some(start) = (0..n).min_by(|&i, &j| match (pts.get(i), pts.get(j)) {
                (Some(a), Some(b)) => corner(&a.1).total_cmp(&corner(&b.1)),
                _ => std::cmp::Ordering::Equal,
            }) else {
                return out;
            };
            let mut seen = vec![false; n];
            let mut at = start;
            for _ in 1..n {
                if let Some(s) = seen.get_mut(at) {
                    *s = true;
                }
                let d = |j: usize| match (pts.get(at), pts.get(j)) {
                    (Some(a), Some(b)) => flat_dist(a.1, b.1),
                    _ => f64::INFINITY,
                };
                let Some(next) = (0..n).filter(|j| !seen.get(*j).copied().unwrap_or(true)).min_by(|&i, &j| d(i).total_cmp(&d(j))) else { break };
                // Too far a step makes no brace, but the chain goes on from there.
                if near(at, next).is_some() {
                    out.extend(pair(at, next));
                }
                at = next;
            }
        }
        BracePairing::Tree => {
            // Prim's, over the links no longer than `max_distance`: a forest when they don't reach.
            let mut joined = vec![false; n];
            let mut best: Vec<Option<(f64, usize)>> = vec![None; n];
            for root in 0..n {
                if joined.get(root).copied().unwrap_or(true) {
                    continue;
                }
                let mut at = Some(root);
                while let Some(i) = at {
                    if let Some(j) = joined.get_mut(i) {
                        *j = true;
                    }
                    if let Some(Some((_, from))) = best.get(i) {
                        out.extend(pair(*from, i));
                    }
                    for j in 0..n {
                        if joined.get(j).copied().unwrap_or(true) {
                            continue;
                        }
                        if let (Some(d), Some(slot)) = (near(i, j), best.get_mut(j))
                            && slot.is_none_or(|(old, _)| d < old)
                        {
                            *slot = Some((d, i));
                        }
                    }
                    at = (0..n)
                        .filter(|j| !joined.get(*j).copied().unwrap_or(true))
                        .filter_map(|j| best.get(j).copied().flatten().map(|(d, _)| (d, j)))
                        .min_by(|a, b| a.0.total_cmp(&b.0))
                        .map(|(_, j)| j);
                }
            }
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PresetId;

    fn column(x: f64, y: f64, head: f64) -> Route {
        let h = Vec3::new(x, y, head);
        Route { contact: h + Vec3::Z, axis: -Vec3::Z, head: h, body: vec![h, Vec3::new(x, y, 0.0)], foot: true, tests: 0, fault: false }
    }

    fn slope(s: &(Vec3, Vec3)) -> f64 {
        (s.1.z - s.0.z).atan2(flat_dist(s.0, s.1)).to_degrees()
    }

    #[test]
    fn a_zig_zag_fills_the_span_and_only_ever_gets_steeper() {
        let p = PresetId::Medium.defaults();
        let params = BraceParams::default();
        // 5 mm apart, a 21.5 mm span from 1 mm above the base to 2 mm under the lower head: four
        // bays at 45° would rise 20 mm, so four taller ones fill it.
        let lo = p.base.height + 1.0;
        let (a, b) = (column(0.0, 0.0, lo + 21.5 + 2.0), column(5.0, 0.0, lo + 30.0));
        let s = brace_struts(&a, &b, &params, &p);
        assert_eq!(s.len(), 4);
        assert!((s[0].0.z - lo).abs() < 1e-9 && (s[3].1.z - (lo + 21.5)).abs() < 1e-9, "{s:?}");
        assert!(s.iter().all(|st| slope(st) >= 45.0 - 1e-9 && slope(st) < 50.0), "{s:?}");
        // Zig, then zag: each strut starts where the last ended.
        assert!(s.windows(2).all(|w| w[0].1 == w[1].0));
        assert_eq!((s[0].0.x, s[0].1.x, s[1].1.x), (0.0, 5.0, 0.0));

        let cross = brace_struts(&a, &b, &BraceParams { pattern: BracePattern::Cross, ..params }, &p);
        assert_eq!(cross.len(), 8);

        // A steeper angle, fewer bays.
        assert_eq!(brace_struts(&a, &b, &BraceParams { angle: 70.0, ..params }, &p).len(), 1);
        // Too short for one bay; too far apart; touching; a branch with no body.
        assert!(brace_struts(&column(0.0, 0.0, lo + 4.0), &column(5.0, 0.0, lo + 4.0), &params, &p).is_empty());
        assert!(brace_struts(&a, &column(50.0, 0.0, 30.0), &params, &p).is_empty());
        assert!(brace_struts(&a, &column(p.body.diameter / 2.0, 0.0, 30.0), &params, &p).is_empty());
        let mut branch = b.clone();
        (branch.body, branch.foot) = (vec![b.head], false);
        assert!(brace_struts(&a, &branch, &params, &p).is_empty());
        // Hostile settings make no struts rather than thousands.
        assert!(brace_struts(&a, &b, &BraceParams { angle: f64::NAN, ..params }, &p).is_empty());
        assert!(brace_struts(&a, &column(0.0, p.body.diameter, 1e5), &BraceParams { angle: 80.0, ..params }, &p).len() <= MAX_BAYS);
    }

    #[test]
    fn struts_follow_a_bent_body() {
        let p = PresetId::Medium.defaults();
        let params = BraceParams::default();
        // `b` leans away from `a` above a knee at 10 mm.
        let a = column(0.0, 0.0, 30.0);
        let knee = Vec3::new(6.0, 0.0, 10.0);
        let b = Route { body: vec![Vec3::new(12.0, 0.0, 30.0), knee, Vec3::new(6.0, 0.0, 0.0)], ..column(6.0, 0.0, 30.0) };
        let s = brace_struts(&a, &b, &params, &p);
        assert!(!s.is_empty());
        for st in &s {
            assert!(slope(st) >= 45.0 - 1e-9, "{st:?}");
            for end in [st.0, st.1] {
                let on_a = end.x.abs() < 1e-9;
                let on_b = at_height(&b.body, end.z).is_some_and(|q| q.dist(end) < 1e-9);
                assert!(on_a || on_b, "{end:?} on a body");
            }
        }
        assert!(s.iter().any(|st| (st.0.z - 10.0).abs() < 1e-9 || (st.1.z - 10.0).abs() < 1e-9), "a strut ends at the knee: {s:?}");
    }

    /// Bodies leaning side by side, across the gap: above their knees the struts are measured in
    /// the tilted plane of the two leaning segments, below them in the vertical one.
    #[test]
    fn each_section_is_measured_in_its_own_plane() {
        let p = PresetId::Medium.defaults();
        let params = BraceParams::default();
        let leaning = |x: f64| Route { body: vec![Vec3::new(x, 6.0, 30.0), Vec3::new(x, 0.0, 12.0), Vec3::new(x, 0.0, 0.0)], ..column(x, 0.0, 30.0) };
        let s = brace_struts(&leaning(0.0), &leaning(8.0), &params, &p);
        // Below the knees 10.2 mm for an 8 mm gap: one bay. Above, 16.9 mm up the tilted plane: two.
        assert_eq!(s.len(), 3, "{s:?}");
        assert!(s.windows(2).all(|w| w[0].1 == w[1].0), "one unbroken zig-zag: {s:?}");
        assert!((s[0].1.z - 12.0).abs() < 1e-9, "the first section ends at the knees: {s:?}");
        let up = Vec3::new(0.0, 6.0, 18.0).normalized().unwrap();
        for st in &s[1..] {
            let v = st.1 - st.0;
            let in_plane = v.dot(up).atan2(v.x.abs()).to_degrees();
            assert!((45.0 - 1e-9..50.0).contains(&in_plane), "{st:?}: {in_plane}");
            assert!(slope(st) < 45.0, "steeper in the plane than from horizontal: {st:?}");
        }
    }

    #[test]
    fn pairs_by_chain_tree_or_all_within_reach() {
        // A 2 x 3 grid, 5 mm apart, and one 40 mm off.
        let mut pts: Vec<(u64, Vec3)> = (0..6).map(|i| (i + 1, Vec3::new(5.0 * (i % 3) as f64, 5.0 * (i / 3) as f64, 0.0))).collect();
        pts.push((7, Vec3::new(40.0, 0.0, 0.0)));
        let all = brace_pairs(&pts, BracePairing::All, 10.0);
        // Each grid pair within 10 mm: 7 at 5 mm, 4 diagonals, 2 at 10 mm.
        assert_eq!(all.len(), 13);
        assert!(all.iter().all(|(a, b)| a < b && *b != 7));
        let tree = brace_pairs(&pts, BracePairing::Tree, 10.0);
        assert_eq!(tree.len(), 5, "{tree:?}");
        assert!(tree.iter().all(|p| all.contains(p)));
        let chain = brace_pairs(&pts, BracePairing::Chain, 10.0);
        assert_eq!(chain.len(), 5, "{chain:?}");
        assert!(chain.iter().all(|p| all.contains(p)));
        assert!(brace_pairs(&pts[..1], BracePairing::Tree, 10.0).is_empty());
        assert!(brace_pairs(&[], BracePairing::Chain, 10.0).is_empty());
    }
}
