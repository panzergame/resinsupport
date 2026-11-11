// Ported from mslicer (GPL-3.0), slicer/src/slicer/raster/mod.rs (`layer`) at 355aca2; see
// ATTRIBUTION.md. Changed: one exposure for every solid (no per-model priority or exposure
// table), a segment carries its depth change, rows start on a whole pixel when anti-aliasing,
// the work stops when cancelled.

//! One layer's cut, filled row by row into runs.

mod downsample;
mod edge_table;
mod segments;

pub(crate) use segments::Segments1D;

use crate::{Cancel, Run};
use downsample::{downsample, downsample_adjacent};
use edge_table::{global_edge_table, update_active_edges};

/// Where a plane cuts one triangle: X and Y in (anti-aliased) pixels.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Segment {
    pub(crate) a: [f32; 2],
    pub(crate) b: [f32; 2],
    /// +1 where a scanline along +X enters the solid, −1 where it leaves.
    pub(crate) delta: i8,
}

/// Rows between checks of the cancel flag.
const CANCEL_EVERY: u64 = 1024;

/// Append `len` pixels of `grey`, merged into the last run when it has the same grey.
pub(crate) fn push(out: &mut Vec<Run>, len: u64, grey: u8) {
    if len == 0 {
        return;
    }
    match out.last_mut() {
        Some(r) if r.grey == grey => r.len += len,
        _ => out.push(Run { len, grey }),
    }
}

/// The layer cut along `segments`, at `aa` samples per pixel side on a screen of `res` pixels:
/// its runs, covering every pixel, and how many rows did not close.
pub(crate) fn layer(aa: u32, res: (u32, u32), segments: &[Segment], cancel: &Cancel) -> (Vec<Run>, u64) {
    let n = u64::from(aa.max(1));
    let (real_w, real_h) = (u64::from(res.0), u64::from(res.1));
    let (w, h) = (real_w * n, real_h * n);
    let mut edges = global_edge_table(segments);
    let mut active = Vec::new();
    let first = edges.front().map_or(h, |e| (((e.p_min_y() - 0.5).ceil().max(0.0)) as u64).min(h));
    // Start on a whole pixel's first sub-row, so each group of `n` rows is one pixel row.
    let mut y = first / n * n;
    let mut runs = Vec::new();
    push(&mut runs, y / n * real_w, 0);
    let mut row = Vec::new();
    let mut rows = vec![Vec::new(); n as usize];
    let mut open = 0;
    while y < h && (!edges.is_empty() || !active.is_empty() || !y.is_multiple_of(n)) {
        if y.is_multiple_of(CANCEL_EVERY) && cancel.is_cancelled() {
            break;
        }
        update_active_edges(&mut edges, &mut active, y);
        let out = if n > 1 { &mut row } else { &mut runs };
        let px = |x: f32| (((x - 0.5).ceil().max(0.0)) as u64).min(w);
        let mut depth = 0i32;
        let mut last = 0;
        for pair in active.windows(2) {
            let [a, b] = pair else { continue };
            depth += i32::from(a.delta);
            let (start, end) = (px(a.x).max(last), px(b.x));
            if depth != 0 && end > start {
                push(out, start - last, 0);
                push(out, end - start, 255);
                last = end;
            }
        }
        if let Some(e) = active.last() {
            depth += i32::from(e.delta);
        }
        if depth != 0 {
            open += 1;
        }
        push(out, w - last, 0);
        if n > 1 {
            if let Some(sub) = rows.get_mut((y % n) as usize) {
                downsample_adjacent(n, &row, sub);
            }
            row.clear();
            if (y + 1).is_multiple_of(n) {
                downsample(&rows, real_w, &mut runs);
                rows.iter_mut().for_each(Vec::clear);
            }
        }
        y += 1;
    }
    // The printer does not clear its buffer between layers: a layer stopping short would print
    // the last one's leftovers, so the runs always reach the last pixel.
    let covered: u64 = runs.iter().map(|r| r.len).sum();
    push(&mut runs, (real_w * real_h).saturating_sub(covered), 0);
    (runs, open)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(x0: f32, y0: f32, x1: f32, y1: f32) -> Vec<Segment> {
        // Counter-clockwise seen from above with Y up; in pixels Y points down, so the left edge
        // enters and the right one leaves.
        vec![
            Segment { a: [x0, y0], b: [x0, y1], delta: 1 },
            Segment { a: [x1, y0], b: [x1, y1], delta: -1 },
            Segment { a: [x0, y0], b: [x1, y0], delta: 1 },
            Segment { a: [x0, y1], b: [x1, y1], delta: 1 },
        ]
    }

    fn total(runs: &[Run]) -> u64 {
        runs.iter().map(|r| r.len).sum()
    }

    #[test]
    fn a_square_fills_its_pixels_and_the_layer_reaches_the_end() {
        let cancel = Cancel::default();
        let (runs, open) = layer(1, (10, 8), &square(2.0, 3.0, 5.0, 6.0), &cancel);
        assert_eq!(open, 0);
        assert_eq!(total(&runs), 80);
        let lit: u64 = runs.iter().filter(|r| r.grey == 255).map(|r| r.len).sum();
        assert_eq!(lit, 9);
        // Nothing to cut: one dark run.
        assert_eq!(layer(1, (10, 8), &[], &cancel).0, vec![Run { len: 80, grey: 0 }]);
        // Off the plate: clipped, still covering every pixel.
        let (runs, _) = layer(2, (10, 8), &square(-5.0, -5.0, 40.0, 3.0), &cancel);
        assert_eq!(total(&runs), 80);
    }

    #[test]
    fn a_lone_edge_is_an_open_row() {
        let (runs, open) = layer(1, (10, 8), &[Segment { a: [2.0, 0.0], b: [2.0, 8.0], delta: 1 }], &Cancel::default());
        assert_eq!(open, 8);
        assert_eq!(total(&runs), 80);
    }
}
