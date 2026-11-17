// Exact coverage reproduces what PrusaSlicer's SLA rasterizer outputs (AGG's
// `rasterizer_scanline_aa`, used by `src/libslic3r/SLA/AGGRaster.hpp` at e6c85c5, on the layer's
// polygons unioned first, `SLAPrintSteps.cpp`): each pixel's grey is the share of its square inside
// the union of the cut. Written from that behaviour, with no PrusaSlicer or AGG code.

//! Exact anti-aliasing: a pixel's grey is the share of it inside the cut under the nonzero rule,
//! overlapping solids counted once, worked out from the edges crossing it with no sub-samples.
//!
//! Row by row. Only an edge piece where the winding goes between zero and non-zero bounds the
//! union: entering, it lights everything right of it in the row by its height; leaving, it darkens
//! as much. A piece of height `dy` whose X runs evenly over `[xa, xb]` covers `dy · (H(i + 1) −
//! H(i))` of pixel `i`, with `H(u)` the mean of `max(u − X, 0)` over the piece. The row keeps that
//! coverage's differences per pixel, the second differences of `H`, non-zero only where the piece
//! crosses, and a running sum along the row gives each pixel's coverage.
//!
//! Pieces sharing pixels make a group. Whether a piece bounds the union depends on the winding
//! left of it, which can change down the row (a solid's top or bottom ends in it). A group whose
//! pieces never share a height, beside a winding the same over the whole row, has one answer per
//! piece. Otherwise the row is cut into slabs at every piece end, every change of the winding to
//! the group's left and every crossing, and in each slab the pieces are walked left to right.
//!
//! A row that does not close lights nothing past the pixels its last edge crosses, like the
//! sub-sampled fill, so an open mesh never streaks to the screen's edge.

use super::{CANCEL_EVERY, Segment, push};
use crate::{Cancel, Run};

/// A segment piece, its top end first, wholly at X ≥ 0: a piece left of the plate stands on its
/// edge, where it still turns everything right of it inside or out.
struct Edge {
    top: [f64; 2],
    bottom: [f64; 2],
    /// X per unit of Y.
    slope: f64,
    delta: i32,
}

/// One edge within one pixel row.
#[derive(Clone, Copy)]
struct Piece {
    top: [f64; 2],
    slope: f64,
    ya: f64,
    yb: f64,
    /// The first and last pixel it crosses.
    lo: usize,
    hi: usize,
    delta: i32,
}

impl Piece {
    fn x_at(&self, y: f64) -> f64 {
        (self.top[0] + (y - self.top[1]) * self.slope).max(0.0)
    }
}

/// A winding sum left over at the end of a row beyond this is a row that did not close.
const OPEN: f64 = 1e-6;

/// Crossings cut in one slab: past them, hostile input is cut no further.
const MAX_CROSSINGS: usize = 4096;

fn edges(segments: &[Segment]) -> Vec<Edge> {
    let mut out = Vec::with_capacity(segments.len());
    let mut add = |p: [f64; 2], q: [f64; 2], delta: i32| {
        let (top, bottom) = if p[1] < q[1] { (p, q) } else { (q, p) };
        let dy = bottom[1] - top[1];
        if dy > 0.0 {
            let (top, bottom) = ([top[0].max(0.0), top[1]], [bottom[0].max(0.0), bottom[1]]);
            out.push(Edge { top, bottom, slope: (bottom[0] - top[0]) / dy, delta });
        }
    };
    for s in segments {
        let (p, q) = (s.a.map(f64::from), s.b.map(f64::from));
        if !p.iter().chain(&q).all(|c| c.is_finite()) {
            continue;
        }
        let delta = i32::from(s.delta);
        if (p[0] < 0.0) != (q[0] < 0.0) && p[0] != q[0] {
            let t = -p[0] / (q[0] - p[0]);
            let m = [0.0, p[1] + t * (q[1] - p[1])];
            add(p, m, delta);
            add(m, q, delta);
        } else {
            add(p, q, delta);
        }
    }
    out.sort_by(|a, b| a.top[1].total_cmp(&b.top[1]));
    out
}

/// The winding just left of the pixels still to do, down the row: each step from its Y to the
/// next one's, the last to the row's bottom. Neighbours differ.
struct Profile {
    steps: Vec<(f64, i32)>,
    bottom: f64,
}

impl Profile {
    fn reset(&mut self, top: f64, bottom: f64) {
        self.steps.clear();
        self.steps.push((top, 0));
        self.bottom = bottom;
    }

    fn split(&mut self, y: f64) {
        let k = self.steps.partition_point(|s| s.0 <= y);
        if let Some(&(start, w)) = k.checked_sub(1).and_then(|i| self.steps.get(i))
            && start < y
            && y < self.bottom
        {
            self.steps.insert(k, (y, w));
        }
    }

    fn add(&mut self, ya: f64, yb: f64, delta: i32) {
        self.split(ya);
        self.split(yb);
        for s in self.steps.iter_mut().filter(|s| s.0 >= ya && s.0 < yb) {
            s.1 = s.1.saturating_add(delta);
        }
        self.steps.dedup_by(|later, earlier| later.1 == earlier.1);
    }

    fn at(&self, y: f64) -> i32 {
        let k = self.steps.partition_point(|s| s.0 <= y);
        k.checked_sub(1).and_then(|i| self.steps.get(i)).map_or(0, |s| s.1)
    }

    /// The winding when it is the same all down the row.
    fn flat(&self) -> Option<i32> {
        match self.steps.as_slice() {
            [(_, w)] => Some(*w),
            _ => None,
        }
    }

    /// The share of the row's height inside.
    fn covered(&self) -> f64 {
        let ends = self.steps.iter().skip(1).map(|s| s.0).chain(std::iter::once(self.bottom));
        self.steps.iter().zip(ends).filter(|(s, _)| s.1 != 0).map(|(s, end)| end - s.0).sum()
    }

    /// The share of the row's height inside that `closing`, the winding of the pieces right of
    /// the screen, brings back to zero.
    fn covered_closed_by(&self, closing: &Profile, ys: &mut Vec<f64>) -> f64 {
        if closing.flat() == Some(0) {
            return 0.0;
        }
        ys.clear();
        ys.extend(self.steps.iter().chain(&closing.steps).map(|s| s.0));
        ys.push(self.bottom);
        ys.sort_unstable_by(f64::total_cmp);
        ys.dedup();
        let inside = |y: f64| {
            let w = self.at(y);
            w != 0 && w.saturating_add(closing.at(y)) == 0
        };
        ys.windows(2)
            .filter_map(|p| match *p {
                [a, b] if inside(0.5 * (a + b)) => Some(b - a),
                _ => None,
            })
            .sum()
    }
}

/// 1 inside, 0 outside.
fn lit(winding: i32) -> i32 {
    i32::from(winding != 0)
}

/// Add an edge piece of signed height `d` whose X runs over `[xa, xb]` (`xa ≤ xb`) to the coverage
/// differences `acc` of the pixels from `origin` on.
fn spread(acc: &mut [f64], origin: usize, xa: f64, xb: f64, d: f64) {
    let len = xb - xa;
    let h = |u: f64| {
        if u <= xa {
            0.0
        } else if u >= xb {
            u - 0.5 * (xa + xb)
        } else {
            (u - xa) * (u - xa) / (2.0 * len)
        }
    };
    let first = xa.floor().max(origin as f64);
    let last = (xb.floor() + 1.0).min((origin + acc.len()) as f64 - 1.0);
    if !(first <= last) {
        return;
    }
    let (first, last) = (first as usize, last as usize);
    let (mut before, mut at) = (h(first as f64 - 1.0), h(first as f64));
    for i in first..=last {
        let after = h(i as f64 + 1.0);
        if let Some(cell) = acc.get_mut(i - origin) {
            *cell += d * (after - 2.0 * at + before);
        }
        (before, at) = (at, after);
    }
}

/// Buffers reused from row to row.
#[derive(Default)]
struct Scratch {
    ys: Vec<f64>,
    /// A slab's pieces: X at its top and bottom.
    slab: Vec<(f64, f64, Piece)>,
    /// Where a slab is cut.
    cuts: Vec<f64>,
}

/// Add to `acc`, the coverage differences of the pixels from `origin` on, the pieces of `group`
/// that bound the union, `profile` being the winding left of them.
fn resolve(group: &mut [Piece], profile: &Profile, origin: usize, acc: &mut [f64], scratch: &mut Scratch) {
    group.sort_unstable_by(|a, b| a.ya.total_cmp(&b.ya));
    let one_per_height = group.windows(2).all(|w| matches!(w, [a, b] if b.ya >= a.yb));
    if let Some(w) = profile.flat()
        && one_per_height
    {
        let sign = |delta: i32| lit(w.saturating_add(delta)) - lit(w);
        for p in group.iter() {
            let s = sign(p.delta);
            if s != 0 {
                let (xa, xb) = (p.x_at(p.ya), p.x_at(p.yb));
                spread(acc, origin, xa.min(xb), xa.max(xb), f64::from(s) * (p.yb - p.ya));
            }
        }
        return;
    }
    let ys = &mut scratch.ys;
    ys.clear();
    ys.extend(profile.steps.iter().map(|s| s.0));
    ys.push(profile.bottom);
    ys.extend(group.iter().flat_map(|p| [p.ya, p.yb]));
    ys.sort_unstable_by(f64::total_cmp);
    ys.dedup();
    for pair in ys.windows(2) {
        let [s0, e0] = *pair else { continue };
        let slab = &mut scratch.slab;
        slab.clear();
        slab.extend(group.iter().filter(|p| p.ya <= s0 && p.yb >= e0).map(|p| (p.x_at(s0), p.x_at(e0), *p)));
        // Cut where pieces cross, so each part keeps one order. Two pieces cross when their order
        // at the slab's top and bottom differ: sorted by the top, an insertion sort by the bottom
        // swaps each such pair once.
        let cuts = &mut scratch.cuts;
        cuts.clear();
        cuts.extend([s0, e0]);
        slab.sort_unstable_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
        'sort: for i in 1..slab.len() {
            let mut j = i;
            while let (Some(&a), Some(&b)) = (slab.get(j.wrapping_sub(1)), slab.get(j)) {
                if b.1 >= a.1 {
                    break;
                }
                if cuts.len() >= MAX_CROSSINGS {
                    break 'sort;
                }
                let (top, bottom) = (b.0 - a.0, b.1 - a.1);
                if top > 0.0 {
                    cuts.push(s0 + (e0 - s0) * top / (top - bottom));
                }
                slab.swap(j - 1, j);
                j -= 1;
            }
        }
        cuts.sort_unstable_by(f64::total_cmp);
        cuts.dedup();
        let wind_left = profile.at(s0);
        for part in cuts.windows(2) {
            let [s, e] = *part else { continue };
            if !(e > s) {
                continue;
            }
            // Ordered half way down, clear of the crossings at its ends.
            let mid = 0.5 * (s + e);
            slab.sort_unstable_by(|a, b| a.2.x_at(mid).total_cmp(&b.2.x_at(mid)));
            let mut wind = wind_left;
            for &(_, _, p) in slab.iter() {
                let next = wind.saturating_add(p.delta);
                let sign = lit(next) - lit(wind);
                if sign != 0 {
                    let (xs, xe) = (p.x_at(s), p.x_at(e));
                    spread(acc, origin, xs.min(xe), xs.max(xe), f64::from(sign) * (e - s));
                }
                wind = next;
            }
        }
    }
}

fn grey(coverage: f64) -> u8 {
    (coverage.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// The layer cut along `segments` on a screen of `res` pixels, each pixel's grey its covered
/// share: its runs, covering every pixel, and how many rows did not close.
pub(crate) fn layer(res: (u32, u32), segments: &[Segment], cancel: &Cancel) -> (Vec<Run>, u64) {
    let (w, h) = (res.0 as usize, res.1 as usize);
    let edges = edges(segments);
    let mut runs = Vec::new();
    let first = edges.first().map_or(h, |e| (e.top[1].floor().max(0.0) as usize).min(h));
    push(&mut runs, first as u64 * w as u64, 0);
    let mut active: Vec<usize> = Vec::new();
    let mut pieces: Vec<Piece> = Vec::new();
    let mut acc: Vec<f64> = Vec::new();
    let mut profile = Profile { steps: Vec::new(), bottom: 0.0 };
    let mut closing = Profile { steps: Vec::new(), bottom: 0.0 };
    let mut scratch = Scratch::default();
    let mut next = 0;
    let mut open = 0;
    for y in first..h {
        if next == edges.len() && active.is_empty() {
            break;
        }
        if (y as u64).is_multiple_of(CANCEL_EVERY) && cancel.is_cancelled() {
            break;
        }
        let (y0, y1) = (y as f64, y as f64 + 1.0);
        while edges.get(next).is_some_and(|e| e.top[1] < y1) {
            active.push(next);
            next += 1;
        }
        active.retain(|&i| edges.get(i).is_some_and(|e| e.bottom[1] > y0));
        pieces.clear();
        closing.reset(y0, y1);
        let mut winding = 0.0;
        for e in active.iter().filter_map(|&i| edges.get(i)) {
            let (ya, yb) = (e.top[1].max(y0), e.bottom[1].min(y1));
            if yb <= ya {
                continue;
            }
            winding += f64::from(e.delta) * (yb - ya);
            let piece = Piece { top: e.top, slope: e.slope, ya, yb, lo: 0, hi: 0, delta: e.delta };
            let (xa, xb) = (piece.x_at(ya), piece.x_at(yb));
            if xa.min(xb) < w as f64 {
                let hi = xa.max(xb).floor().min(w as f64 - 1.0) as usize;
                pieces.push(Piece { lo: xa.min(xb).floor() as usize, hi, ..piece });
            } else {
                // Right of the screen it bounds no pixel, but may close what runs off it.
                closing.add(ya, yb, e.delta);
            }
        }
        if winding.abs() > OPEN {
            open += 1;
        }
        pieces.sort_unstable_by_key(|p| p.lo);
        profile.reset(y0, y1);
        let mut done = 0;
        let mut start = 0;
        while let Some(head) = pieces.get(start) {
            let (a, mut b) = (head.lo, head.hi);
            let mut end = start + 1;
            while let Some(p) = pieces.get(end).filter(|p| p.lo <= b) {
                b = b.max(p.hi);
                end += 1;
            }
            let Some(group) = pieces.get_mut(start..end) else { break };
            let base = profile.covered();
            push(&mut runs, a.saturating_sub(done) as u64, grey(base));
            acc.clear();
            acc.resize(b - a + 2, 0.0);
            resolve(group, &profile, a, &mut acc, &mut scratch);
            let mut sum = base;
            for cell in acc.iter().take(b - a + 1) {
                sum += cell;
                push(&mut runs, 1, grey(sum));
            }
            for p in group.iter() {
                profile.add(p.ya, p.yb, p.delta);
            }
            done = b + 1;
            start = end;
        }
        // Past the last pixel an edge crosses, only a solid running off the right side is inside.
        let rest = profile.covered_closed_by(&closing, &mut scratch.ys);
        push(&mut runs, w.saturating_sub(done) as u64, grey(rest));
    }
    // The printer does not clear its buffer between layers: the runs always reach the last pixel.
    let covered: u64 = runs.iter().map(|r| r.len).sum();
    push(&mut runs, (w as u64 * h as u64).saturating_sub(covered), 0);
    (runs, open)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A rectangle in pixels, as the cut gives it: the left side enters, the right one leaves.
    fn rect(x0: f32, y0: f32, x1: f32, y1: f32) -> Vec<Segment> {
        vec![
            Segment { a: [x0, y0], b: [x0, y1], delta: 1 },
            Segment { a: [x1, y0], b: [x1, y1], delta: -1 },
            Segment { a: [x0, y0], b: [x1, y0], delta: 1 },
            Segment { a: [x0, y1], b: [x1, y1], delta: 1 },
        ]
    }

    fn bitmap(runs: &[Run]) -> Vec<u8> {
        runs.iter().flat_map(|r| std::iter::repeat_n(r.grey, r.len as usize)).collect()
    }

    fn lit(runs: &[Run]) -> f64 {
        runs.iter().map(|r| r.len as f64 * f64::from(r.grey) / 255.0).sum()
    }

    #[test]
    fn a_pixel_half_covered_is_half_grey() {
        let cancel = Cancel::default();
        let (runs, open) = layer((6, 4), &rect(1.5, 1.0, 4.0, 3.0), &cancel);
        assert_eq!(open, 0);
        #[rustfmt::skip]
        assert_eq!(bitmap(&runs), [
            0, 0, 0, 0, 0, 0,
            0, 128, 255, 255, 0, 0,
            0, 128, 255, 255, 0, 0,
            0, 0, 0, 0, 0, 0,
        ]);
        // A quarter of a pixel, both ways.
        let (runs, _) = layer((3, 3), &rect(1.5, 1.5, 2.0, 2.0), &cancel);
        assert_eq!(bitmap(&runs)[4], 64);
    }

    #[test]
    fn a_slant_greys_its_pixels_by_their_covered_share_and_keeps_the_area() {
        // A right triangle with legs of 7 pixels: its hypotenuse halves the pixels it crosses.
        let tri = vec![
            Segment { a: [1.0, 1.0], b: [1.0, 8.0], delta: 1 },
            Segment { a: [1.0, 1.0], b: [8.0, 8.0], delta: -1 },
            Segment { a: [1.0, 8.0], b: [8.0, 8.0], delta: 1 },
        ];
        let (runs, open) = layer((10, 10), &tri, &Cancel::default());
        assert_eq!(open, 0);
        let px = bitmap(&runs);
        for k in 1..8 {
            assert_eq!(px[k * 10 + k], 128, "diagonal pixel {k}");
            assert_eq!(px[k * 10 + k - 1], if k > 1 { 255 } else { 0 });
            assert_eq!(px[k * 10 + k + 1], 0);
        }
        assert!((lit(&runs) - 24.5).abs() < 0.05, "{}", lit(&runs));
        // At any slope and offset, the lit area is the triangle's.
        for (x0, x1) in [(0.3f32, 7.9f32), (2.71, 3.05), (0.0, 9.99)] {
            let tri = vec![
                Segment { a: [x0, 0.4], b: [x0, 9.6], delta: 1 },
                Segment { a: [x0, 0.4], b: [x1, 9.6], delta: -1 },
                Segment { a: [x0, 9.6], b: [x1, 9.6], delta: 1 },
            ];
            let (runs, _) = layer((10, 10), &tri, &Cancel::default());
            let area = 0.5 * f64::from(x1 - x0) * 9.2;
            assert!((lit(&runs) - area).abs() < 0.2, "{x0} {x1}: {} {area}", lit(&runs));
        }
    }

    #[test]
    fn overlaps_light_once_holes_stay_dark_and_lone_edges_are_open() {
        let cancel = Cancel::default();
        let mut both = rect(1.0, 1.0, 5.0, 3.0);
        both.extend(rect(3.0, 1.0, 7.0, 3.0));
        let (runs, open) = layer((8, 4), &both, &cancel);
        assert_eq!((lit(&runs), open), (12.0, 0));
        // A hole: the inner rectangle wound the other way.
        let mut ring = rect(0.0, 0.0, 6.0, 6.0);
        ring.extend(rect(2.5, 2.5, 3.5, 3.5).into_iter().map(|s| Segment { delta: -s.delta, ..s }));
        let (runs, _) = layer((6, 6), &ring, &cancel);
        assert_eq!(bitmap(&runs)[2 * 6 + 2], 191);
        assert!((lit(&runs) - 35.0).abs() < 0.01);
        // A lone edge lights the pixels it crosses only, not up to the screen's edge.
        let (runs, open) = layer((10, 8), &[Segment { a: [2.5, 0.0], b: [2.5, 8.0], delta: 1 }], &cancel);
        assert_eq!(open, 8);
        assert!(bitmap(&runs).chunks(10).all(|row| row == [0, 0, 128, 0, 0, 0, 0, 0, 0, 0]));
        // An open edge left of a closed solid: lit from it through the solid's last pixel, as the
        // winding never returns to zero, and nothing after.
        let mut both = vec![Segment { a: [0.5, 0.0], b: [0.5, 1.0], delta: 1 }];
        both.extend(rect(3.0, 0.0, 5.0, 1.0));
        assert_eq!(bitmap(&layer((8, 1), &both, &cancel).0), [128, 255, 255, 255, 255, 255, 0, 0]);
    }

    /// A closed polygon's sides, each with its depth change.
    fn polygon(points: &[[f32; 2]]) -> Vec<Segment> {
        let n = points.len();
        let area: f32 = (0..n).map(|i| points[i][0] * points[(i + 1) % n][1] - points[(i + 1) % n][0] * points[i][1]).sum();
        (0..n)
            .map(|i| {
                let (a, b) = (points[i], points[(i + 1) % n]);
                // Inside lies left of a side when the area is positive: towards +X where it runs up.
                let up = b[1] < a[1];
                Segment { a, b, delta: if up == (area > 0.0) { 1 } else { -1 } }
            })
            .collect()
    }

    /// Each pixel's covered share, from 64 × 64 samples of the nonzero rule.
    fn sampled(res: (u32, u32), segments: &[Segment]) -> Vec<f64> {
        const N: u32 = 64;
        let inside = |x: f32, y: f32| {
            let mut w = 0;
            for s in segments {
                let (p, q) = if s.a[1] < s.b[1] { (s.a, s.b) } else { (s.b, s.a) };
                if p[1] <= y && y < q[1] && p[0] + (y - p[1]) / (q[1] - p[1]) * (q[0] - p[0]) < x {
                    w += i32::from(s.delta);
                }
            }
            w != 0
        };
        let mut out = Vec::new();
        for py in 0..res.1 {
            for px in 0..res.0 {
                let mut n = 0;
                for j in 0..N {
                    for i in 0..N {
                        let (x, y) = (px as f32 + (i as f32 + 0.5) / N as f32, py as f32 + (j as f32 + 0.5) / N as f32);
                        n += u32::from(inside(x, y));
                    }
                }
                out.push(f64::from(n) / f64::from(N * N));
            }
        }
        out
    }

    #[test]
    fn overlapping_solids_count_once_down_to_each_pixel() {
        let cancel = Cancel::default();
        // Both right sides in pixel 5: half of it, not both halves added.
        let mut both = rect(1.0, 1.0, 5.5, 3.0);
        both.extend(rect(3.0, 1.0, 5.5, 3.0));
        let (runs, open) = layer((8, 4), &both, &cancel);
        assert_eq!(open, 0);
        assert_eq!(&bitmap(&runs)[8..16], [0, 255, 255, 255, 255, 128, 0, 0]);
        // Tops ending part way down a row: the shared row is lit once.
        let mut both = rect(1.0, 1.4, 5.0, 3.0);
        both.extend(rect(3.0, 1.4, 7.0, 3.0));
        let (runs, _) = layer((8, 4), &both, &cancel);
        assert_eq!(&bitmap(&runs)[8..16], [0, 153, 153, 153, 153, 153, 153, 0]);

        // Crossing, coincident, nested and touching solids, against samples of the nonzero rule.
        let res = (12, 10);
        let shapes: [Vec<Vec<[f32; 2]>>; 4] = [
            // Two slanted squares whose sides cross inside pixels.
            vec![vec![[2.3, 1.2], [9.6, 2.7], [8.1, 8.9], [1.1, 7.4]], vec![[5.7, 0.4], [11.2, 5.1], [6.3, 9.7], [0.6, 4.9]]],
            // A triangle overlapping a rectangle, one side shared.
            vec![vec![[1.5, 1.5], [7.5, 1.5], [7.5, 8.5], [1.5, 8.5]], vec![[1.5, 1.5], [10.7, 4.2], [1.5, 8.5]]],
            // A small diamond inside a larger one, and one wound the other way.
            vec![vec![[6.0, 0.5], [11.5, 5.0], [6.0, 9.5], [0.5, 5.0]], vec![[6.0, 3.2], [7.9, 5.0], [6.0, 6.8], [4.1, 5.0]]],
            vec![vec![[0.3, 0.3], [6.2, 0.3], [6.2, 6.2], [0.3, 6.2]], vec![[5.9, 5.9], [11.7, 6.6], [6.6, 9.8]].into_iter().rev().collect()],
        ];
        for (k, shape) in shapes.iter().enumerate() {
            let segments: Vec<Segment> = shape.iter().flat_map(|p| polygon(p)).collect();
            let (runs, open) = layer(res, &segments, &cancel);
            assert_eq!(open, 0, "shape {k}");
            for (i, (&g, want)) in bitmap(&runs).iter().zip(sampled(res, &segments)).enumerate() {
                let got = f64::from(g) / 255.0;
                assert!((got - want).abs() < 0.02, "shape {k}, pixel {i}: {got} against {want}");
            }
        }
    }

    #[test]
    fn random_overlapping_polygons_match_samples_of_the_nonzero_rule() {
        // Xorshift, so every run checks the same polygons.
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed % 10_000) as f32 / 10_000.0
        };
        let res = (10, 8);
        for case in 0..60 {
            let mut segments = Vec::new();
            for _ in 0..1 + (next() * 4.0) as usize {
                let n = 3 + (next() * 5.0) as usize;
                // Past the screen's sides too, some jagged enough to cross themselves.
                let (cx, cy, r) = (next() * 12.0 - 1.0, next() * 10.0 - 1.0, 0.5 + next() * 5.0);
                let points: Vec<[f32; 2]> = (0..n)
                    .map(|k| {
                        let a = k as f32 / n as f32 * std::f32::consts::TAU + next() * 0.5;
                        let rr = r * (0.5 + next());
                        [cx + rr * a.cos(), cy + rr * a.sin()]
                    })
                    .collect();
                segments.extend(polygon(&points));
            }
            let (runs, _) = layer(res, &segments, &Cancel::default());
            for (i, (&g, want)) in bitmap(&runs).iter().zip(sampled(res, &segments)).enumerate() {
                let got = f64::from(g) / 255.0;
                assert!((got - want).abs() < 0.02, "case {case}, pixel {i}: {got} against {want}");
            }
        }
    }

    #[test]
    fn off_the_screen_is_clipped_and_every_pixel_is_covered() {
        let cancel = Cancel::default();
        // Past every side: the whole screen lit.
        let (runs, open) = layer((10, 8), &rect(-5.0, -5.0, 40.0, 30.0), &cancel);
        assert_eq!((runs, open), (vec![Run { len: 80, grey: 255 }], 0));
        // A slant leaving through the left side still closes the rows it crosses.
        let tri = vec![
            Segment { a: [-4.0, 0.0], b: [-4.0, 8.0], delta: 1 },
            Segment { a: [-4.0, 0.0], b: [4.0, 8.0], delta: -1 },
            Segment { a: [-4.0, 8.0], b: [4.0, 8.0], delta: 1 },
        ];
        let (runs, open) = layer((10, 8), &tri, &cancel);
        assert_eq!(open, 0);
        assert!((lit(&runs) - 8.0).abs() < 0.05, "{}", lit(&runs));
        assert_eq!(layer((10, 8), &[], &cancel).0, vec![Run { len: 80, grey: 0 }]);
        let wild = [
            Segment { a: [f32::NAN, 1.0], b: [2.0, 3.0], delta: 1 },
            Segment { a: [f32::MAX, -f32::MAX], b: [-f32::MAX, f32::MAX], delta: 1 },
            Segment { a: [1e30, 2.0], b: [1e30, 2.0], delta: -1 },
        ];
        let (runs, _) = layer((10, 8), &wild, &cancel);
        assert_eq!(runs.iter().map(|r| r.len).sum::<u64>(), 80);
        assert_eq!(layer((0, 0), &rect(1.0, 1.0, 2.0, 2.0), &cancel).0, vec![]);
    }
}
