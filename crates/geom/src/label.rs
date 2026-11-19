//! A model's label, to tell prints apart: its text standing out of the raft, past the base of the
//! model's farthest support on one side, on a plate that widens the raft's outline
//! out to it, so the raft's edge and rim go round it. The text reads from outside that side.

use serde::{Deserialize, Serialize};

use crate::text::{TextOutline, text_outline};
use crate::{Field2, RaftProfile, raft_solid};

/// Capital letters' heights, mm.
pub const LABEL_SIZE_RANGE: std::ops::RangeInclusive<f64> = 1.0..=50.0;
/// How far the letters stand out of the plate, mm.
pub const LABEL_DEPTH_RANGE: std::ops::RangeInclusive<f64> = 0.1..=10.0;
/// The gap from the support's base to the letters, mm.
pub const LABEL_MARGIN_RANGE: std::ops::RangeInclusive<f64> = 0.0..=20.0;
pub const MAX_LABEL_CHARS: usize = 200;
pub const MAX_LABEL_LINES: usize = 8;
pub const DEFAULT_LABEL_SIZE: f64 = 2.0;
pub const DEFAULT_LABEL_DEPTH: f64 = 0.5;
pub const DEFAULT_LABEL_MARGIN: f64 = 1.0;
/// The plate's margin past the letters on its three outer sides, inside the rim's inset: they
/// would otherwise touch the raft's edge.
const EDGE_PAD: f64 = 1.0;
/// The letters' grid reaches this many cells past them: the field must be outside all round its
/// edge for their solid to close.
const GRID_PAD_CELLS: f64 = 4.0;
/// The letters' grid: this share of their height, within the bounds below (mm). A printer's
/// pixel is some 0.02 to 0.05 mm, and a stroke a tenth of the height.
const CELL_SHARE: f64 = 1.0 / 60.0;
const CELL_RANGE: (f64, f64) = (0.01, 0.05);

/// The side of the model's supports the label goes on, seen from the front of the plate.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LabelSide {
    /// Towards -Y.
    #[default]
    Front,
    /// Towards +Y.
    Back,
    /// Towards -X.
    Left,
    /// Towards +X.
    Right,
}

impl LabelSide {
    pub const ALL: [LabelSide; 4] = [LabelSide::Front, LabelSide::Back, LabelSide::Left, LabelSide::Right];
    pub fn id(self) -> &'static str {
        match self {
            LabelSide::Front => "front",
            LabelSide::Back => "back",
            LabelSide::Left => "left",
            LabelSide::Right => "right",
        }
    }
    pub fn parse(s: &str) -> Option<LabelSide> {
        LabelSide::ALL.into_iter().find(|v| v.id() == s)
    }
    /// The unit direction away from the model.
    pub fn outwards(self) -> [f64; 2] {
        match self {
            LabelSide::Front => [0.0, -1.0],
            LabelSide::Back => [0.0, 1.0],
            LabelSide::Left => [-1.0, 0.0],
            LabelSide::Right => [1.0, 0.0],
        }
    }
}

/// What a label says and how it is set.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Label {
    /// Lines break at `\n`. Empty: nothing is printed.
    pub text: String,
    pub side: LabelSide,
    /// The capital letters' height, mm.
    pub size: f64,
    /// How far the letters stand out of the plate, mm.
    pub depth: f64,
    /// The gap from the support's base to the letters, mm. Files from before it was set take
    /// the default.
    #[serde(default = "default_margin")]
    pub margin: f64,
}

fn default_margin() -> f64 {
    DEFAULT_LABEL_MARGIN
}

impl Default for Label {
    fn default() -> Label {
        Label { text: String::new(), side: LabelSide::Front, size: DEFAULT_LABEL_SIZE, depth: DEFAULT_LABEL_DEPTH, margin: DEFAULT_LABEL_MARGIN }
    }
}

impl Label {
    /// Why the label cannot be printed as it is, if it cannot.
    pub fn validate(&self) -> Result<(), String> {
        let within = |x: f64, r: &std::ops::RangeInclusive<f64>| x.is_finite() && r.contains(&x);
        if !within(self.size, &LABEL_SIZE_RANGE) {
            return Err(format!("the label's size must be from {} to {} mm", LABEL_SIZE_RANGE.start(), LABEL_SIZE_RANGE.end()));
        }
        if !within(self.depth, &LABEL_DEPTH_RANGE) {
            return Err(format!("the label's depth must be from {} to {} mm", LABEL_DEPTH_RANGE.start(), LABEL_DEPTH_RANGE.end()));
        }
        if !within(self.margin, &LABEL_MARGIN_RANGE) {
            return Err(format!("the label's margin must be from {} to {} mm", LABEL_MARGIN_RANGE.start(), LABEL_MARGIN_RANGE.end()));
        }
        if self.text.chars().count() > MAX_LABEL_CHARS {
            return Err(format!("the label's text must be at most {MAX_LABEL_CHARS} characters"));
        }
        if self.text.split('\n').count() > MAX_LABEL_LINES {
            return Err(format!("the label's text must be at most {MAX_LABEL_LINES} lines"));
        }
        if self.text.chars().any(|c| c.is_control() && c != '\n') {
            return Err("the label's text must have no control characters but line breaks".into());
        }
        Ok(())
    }
}

/// Where a label goes and what it shows: its text, and the plate under it, a box in the text's
/// frame that is part of the raft's outline, so the raft's edge and rim go round it.
pub struct LabelLayout {
    /// The text frame's x and y axes on the plate: y points at the model.
    x: [f64; 2],
    y: [f64; 2],
    /// Where the text frame's origin lands.
    origin: [f64; 2],
    /// The plate, in the text frame.
    lo: [f64; 2],
    hi: [f64; 2],
    text: TextOutline,
    /// Where the letters' grid goes, in the text frame: their box and some cells round it.
    grid_lo: [f64; 2],
    grid_hi: [f64; 2],
    cell: f64,
    depth: f64,
}

impl LabelLayout {
    /// `label` on a raft `profile` under `feet` (the model's supports' feet on the plate), whose
    /// bases are `base_radius` wide: the letters `label.margin` past the base of the foot farthest
    /// towards its side, inset into the raft's disc round it; the plate from that foot's centre
    /// out round them, a fixed margin and the rim's inset past them on its three outer sides so no
    /// letter stands on the rim. `None` for no foot, no raft, a label that does not validate or
    /// one with no letters.
    pub fn new(label: &Label, feet: &[[f64; 2]], base_radius: f64, profile: &RaftProfile) -> Option<LabelLayout> {
        let usable = |x: f64| x.is_finite() && x > 0.0;
        let inset = profile.inset();
        if label.validate().is_err() || !usable(base_radius) || !usable(profile.thickness) || !inset.is_finite() {
            return None;
        }
        let out = label.side.outwards();
        // The text frame is turned so its y points at the model: the text reads from outside.
        let (x, y) = ([-out[1], out[0]], [-out[0], -out[1]]);
        let along = |p: &[f64; 2], a: [f64; 2]| p[0] * a[0] + p[1] * a[1];
        let far = feet.iter().filter(|f| f.iter().all(|c| c.is_finite())).max_by(|a, b| along(a, out).total_cmp(&along(b, out)))?;
        let cell = (label.size * CELL_SHARE).clamp(CELL_RANGE.0, CELL_RANGE.1);
        let text = text_outline(&label.text, label.size, cell / 5.0)?;
        let (lo, hi) = text.bounds()?;
        // Where the plate's back edge, at the foot's centre, leaves the raft's disc, the rim runs
        // along it: a rim wider than the base would reach letters only `margin` past the base.
        let near = label.margin.max(inset - base_radius);
        let side = EDGE_PAD + inset;
        let (plate_lo, text_hi) = ([lo[0] - side, lo[1] - side], [hi[0] + side, hi[1] + near]);
        let g = GRID_PAD_CELLS * cell;
        let (grid_lo, grid_hi) = ([lo[0] - g, lo[1] - g], [hi[0] + g, hi[1] + g]);
        // The letters' top lands `near` past the farthest base; the plate goes on to its foot.
        let edge = along(far, out) + base_radius;
        let centre = along(far, x) - (plate_lo[0] + text_hi[0]) / 2.0;
        let origin = [x[0] * centre + out[0] * (edge + text_hi[1]), x[1] * centre + out[1] * (edge + text_hi[1])];
        let plate_hi = [text_hi[0], text_hi[1] + base_radius];
        Some(LabelLayout { x, y, origin, lo: plate_lo, hi: plate_hi, text, grid_lo, grid_hi, cell, depth: label.depth })
    }

    fn map(&self, p: [f64; 2]) -> [f64; 2] {
        [self.origin[0] + p[0] * self.x[0] + p[1] * self.y[0], self.origin[1] + p[0] * self.x[1] + p[1] * self.y[1]]
    }

    /// The plate's corners on the build plate.
    pub fn corners(&self) -> [[f64; 2]; 4] {
        [[self.lo[0], self.lo[1]], [self.hi[0], self.lo[1]], [self.hi[0], self.hi[1]], [self.lo[0], self.hi[1]]].map(|p| self.map(p))
    }

    /// The signed distance from `p` on the build plate to the plate's outline, negative inside.
    pub fn plate_distance(&self, p: [f64; 2]) -> f64 {
        let d = [p[0] - self.origin[0], p[1] - self.origin[1]];
        let q = [d[0] * self.x[0] + d[1] * self.x[1], d[0] * self.y[0] + d[1] * self.y[1]];
        let e = [0, 1].map(|k| (q[k] - (self.lo[k] + self.hi[k]) / 2.0).abs() - (self.hi[k] - self.lo[k]) / 2.0);
        e[0].max(0.0).hypot(e[1].max(0.0)) + e[0].max(e[1]).min(0.0)
    }

    /// The letters on a raft `thickness` mm thick, closed and wound outwards: standing `depth`
    /// out of its top, from halfway down into it, so their bottom faces hide inside the raft
    /// rather than lying on its top, where they would flicker against it.
    pub fn letters(&self, thickness: f64) -> Vec<[[f32; 3]; 3]> {
        let Some(field) = Field2::polygons(&self.text.contours, self.grid_lo, self.grid_hi, self.cell) else { return Vec::new() };
        let profile = RaftProfile { thickness: self.depth + thickness / 2.0, slope: 0.0, rim_height: 0.0, rim_width: 0.0 };
        // The frame is a turn, not a mirror: the winding stays outwards.
        raft_solid(&field, &profile)
            .into_iter()
            .map(|t| {
                t.map(|p| {
                    let q = self.map([f64::from(p[0]), f64::from(p[1])]);
                    [q[0] as f32, q[1] as f32, (f64::from(p[2]) + thickness / 2.0) as f32]
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Mesh;

    fn label(text: &str, side: LabelSide) -> Label {
        Label { text: text.into(), side, size: 5.0, depth: 0.5, margin: 1.5 }
    }

    fn flat(thickness: f64) -> RaftProfile {
        RaftProfile { thickness, slope: 0.0, rim_height: 0.0, rim_width: 0.0 }
    }

    #[test]
    fn the_plate_runs_from_the_farthest_foot_out_round_the_text_standing_past_its_base() {
        let feet = [[0.0, 0.0], [10.0, -3.0], [20.0, 5.0]];
        let (r, t) = (4.1, 1.0);
        for side in LabelSide::ALL {
            let l = LabelLayout::new(&label("12", side), &feet, r, &flat(t)).unwrap();
            let out = side.outwards();
            let along = |p: [f64; 2]| p[0] * out[0] + p[1] * out[1];
            let across = |p: [f64; 2]| p[0] * -out[1] + p[1] * out[0];
            // The farthest foot that way: (10, -3) to the front, (0, 0) left, (20, 5) back and right.
            let foot = match side {
                LabelSide::Front => [10.0, -3.0],
                LabelSide::Back | LabelSide::Right => [20.0, 5.0],
                LabelSide::Left => [0.0, 0.0],
            };
            let c = l.corners();
            let (near, far) = c.iter().fold((f64::MAX, f64::MIN), |(a, b), p| (a.min(along(*p)), b.max(along(*p))));
            assert!((near - along(foot)).abs() < 1e-9, "{side:?}: from the foot's centre");
            assert!(far - near > r + 1.5 + 5.0 + EDGE_PAD - 0.1, "{side:?}: {near} {far}");
            let mid = c.iter().map(|p| across(*p)).sum::<f64>() / 4.0;
            assert!((mid - across(foot)).abs() < 1e-9, "{side:?}: centred on it");
            assert!(l.plate_distance(foot).abs() < 1e-9 && l.plate_distance(c[0]).abs() < 1e-9);
            assert!((l.plate_distance([foot[0] + out[0], foot[1] + out[1]]) + 1.0).abs() < 1e-9);

            let tris = l.letters(t);
            let mesh = Mesh::new(tris.clone());
            let b = mesh.bounds();
            assert!((b.min.z - t / 2.0).abs() < 1e-6 && (b.max.z - (t + 0.5)).abs() < 1e-5, "{side:?}: {b:?}");
            // The margin from the base, inside the plate.
            let gap = tris.iter().flatten().map(|p| along([f64::from(p[0]), f64::from(p[1])]) - along(foot)).fold(f64::MAX, f64::min);
            assert!((gap - (r + 1.5)).abs() < 0.05, "{side:?}: {gap}");
            for p in tris.iter().flatten() {
                assert!(l.plate_distance([f64::from(p[0]), f64::from(p[1])]) < -EDGE_PAD + 0.05, "{side:?}: {p:?}");
            }
            // Closed: its volume does not depend on where it is measured from.
            let moved = Mesh::new(tris.iter().map(|t| t.map(|p| [p[0] + 100.0, p[1] - 50.0, p[2] + 30.0])).collect()).signed_volume();
            let v = mesh.signed_volume();
            assert!(v > 1.0 && (moved - v).abs() < 1e-3 * v, "{side:?}: {moved} {v}");
        }
    }

    #[test]
    fn a_rim_widens_the_outer_sides_so_no_letter_stands_on_it_but_not_the_margin() {
        let rim = RaftProfile { thickness: 1.0, slope: 0.0, rim_height: 1.0, rim_width: 2.0 };
        let near = |l: &LabelLayout| l.letters(1.0).iter().flatten().map(|p| -f64::from(p[1])).fold(f64::MAX, f64::min);
        let plain = LabelLayout::new(&label("1", LabelSide::Front), &[[0.0, 0.0]], 3.0, &flat(1.0)).unwrap();
        let ringed = LabelLayout::new(&label("1", LabelSide::Front), &[[0.0, 0.0]], 3.0, &rim).unwrap();
        let (width, length) = (|l: &LabelLayout| l.hi[0] - l.lo[0], |l: &LabelLayout| l.hi[1] - l.lo[1]);
        assert!((width(&ringed) - width(&plain) - 4.0).abs() < 1e-9);
        assert!((length(&ringed) - length(&plain) - 2.0).abs() < 1e-9, "the far side only");
        assert!((near(&ringed) - near(&plain)).abs() < 1e-6 && (near(&plain) - 4.5).abs() < 0.05, "{}", near(&ringed));
        for p in ringed.letters(1.0).iter().flatten() {
            assert!(ringed.plate_distance([f64::from(p[0]), f64::from(p[1])]) < -2.95, "{p:?}");
        }
        // A rim wider than the base past a smaller margin: its run along the plate's back edge
        // would reach the letters, which keep that far from the foot's centre.
        let thin = LabelLayout::new(&Label { margin: 0.0, ..label("1", LabelSide::Front) }, &[[0.0, 0.0]], 0.5, &rim).unwrap();
        assert!((near(&thin) - 2.0).abs() < 0.05, "{}", near(&thin));
    }

    #[test]
    fn the_margin_sets_the_gap_from_the_base_and_none_still_closes_the_letters() {
        let at = |margin: f64| {
            let l = LabelLayout::new(&Label { margin, ..label("8", LabelSide::Front) }, &[[0.0, 0.0]], 2.0, &flat(1.0)).unwrap();
            let tris = l.letters(1.0);
            // The letters' highest y on the plate: the gap from them to the base is the margin.
            let near = tris.iter().flatten().map(|p| -f64::from(p[1])).fold(f64::MAX, f64::min);
            // The plate's three outer sides keep their own margin.
            let edge = tris.iter().flatten().map(|p| l.plate_distance([f64::from(p[0]), f64::from(p[1])])).fold(f64::MIN, f64::max);
            (l, tris, near, edge)
        };
        for margin in [0.0, 0.5, 4.0] {
            let (_, tris, near, edge) = at(margin);
            assert!((near - (2.0 + margin)).abs() < 0.05, "{margin}: {near}");
            assert!((edge + EDGE_PAD).abs() < 0.05, "{margin}: {edge}");
            let mesh = Mesh::new(tris.clone());
            let moved = Mesh::new(tris.iter().map(|t| t.map(|p| [p[0] + 100.0, p[1] - 50.0, p[2] + 30.0])).collect()).signed_volume();
            assert!((moved - mesh.signed_volume()).abs() < 1e-3 * mesh.signed_volume(), "{margin}: closed");
        }
        let size = |m: f64| {
            let l = at(m).0;
            (l.hi[0] - l.lo[0], l.hi[1] - l.lo[1])
        };
        let (a, b) = (size(0.5), size(4.0));
        assert!((b.0 - a.0).abs() < 1e-9 && (b.1 - a.1 - 3.5).abs() < 1e-9, "{a:?} {b:?}");
    }

    #[test]
    fn the_text_reads_from_outside_its_side() {
        // A lone "L": its foot runs right from its stem, in the text frame. Seen from the right of
        // the plate (looking towards -X) the text's right is +Y: the stem lies at the lower Y.
        let solid = LabelLayout::new(&label("L", LabelSide::Right), &[[0.0, 0.0]], 2.0, &flat(1.0)).unwrap().letters(1.0);
        let tops: Vec<[f32; 3]> = solid.iter().flatten().copied().filter(|p| p[2] > 1.4).collect();
        let (y0, y1) = tops.iter().fold((f32::MAX, f32::MIN), |(a, b), p| (a.min(p[1]), b.max(p[1])));
        let near_x = tops.iter().filter(|p| p[1] < y0 + 0.3).map(|p| p[0]).fold(f32::MAX, f32::min);
        let far_x = tops.iter().filter(|p| p[1] < y0 + 0.3).map(|p| p[0]).fold(f32::MIN, f32::max);
        // The stem spans the letter's height, along X; the foot does not.
        assert!(far_x - near_x > 4.5, "{near_x} {far_x} over {y0}..{y1}");
    }

    #[test]
    fn nothing_without_feet_raft_or_letters_and_hostile_labels_are_refused() {
        let feet = [[0.0, 0.0]];
        let none = |l: &Label, feet: &[[f64; 2]], r: f64, t: f64| LabelLayout::new(l, feet, r, &flat(t)).is_none();
        assert!(none(&label("1", LabelSide::Front), &[], 2.0, 1.0));
        assert!(none(&label("1", LabelSide::Front), &[[f64::NAN, 0.0]], 2.0, 1.0));
        assert!(none(&label("1", LabelSide::Front), &feet, 2.0, 0.0));
        assert!(none(&label("1", LabelSide::Front), &feet, f64::NAN, 1.0));
        assert!(none(&label("", LabelSide::Front), &feet, 2.0, 1.0));
        assert!(none(&label(" \n ", LabelSide::Front), &feet, 2.0, 1.0));
        for bad in [
            Label { size: 0.5, ..label("1", LabelSide::Front) },
            Label { size: f64::NAN, ..label("1", LabelSide::Front) },
            Label { depth: 0.0, ..label("1", LabelSide::Front) },
            Label { depth: 1e308, ..label("1", LabelSide::Front) },
            Label { margin: -0.1, ..label("1", LabelSide::Front) },
            Label { margin: 21.0, ..label("1", LabelSide::Front) },
            Label { margin: f64::INFINITY, ..label("1", LabelSide::Front) },
            label(&"1".repeat(MAX_LABEL_CHARS + 1), LabelSide::Front),
            label(&"1\n".repeat(MAX_LABEL_LINES), LabelSide::Front),
            label("1\r2", LabelSide::Front),
        ] {
            assert!(bad.validate().is_err(), "{bad:?}");
            assert!(none(&bad, &feet, 2.0, 1.0));
        }
        assert!(label(&"1".repeat(MAX_LABEL_CHARS), LabelSide::Back).validate().is_ok());
        assert_eq!(LabelSide::parse("left"), Some(LabelSide::Left));
        assert_eq!(LabelSide::parse("up"), None);
    }
}
