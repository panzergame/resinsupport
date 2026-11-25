//! Grey blur: a box blur on a layer's pixels, along X then along Y, repeated. Each pass spreads a
//! lit edge `radius` pixels further and keeps the light it spreads, but for what goes past the
//! screen's edge, which counts dark.
//!
//! Only the rows the light can reach are decoded: those within `reach = radius · passes` of a lit
//! pixel. Within them only the **active spans** are worked on: the pixels within `reach` of a
//! change of grey, across a row or between two rows. Any other pixel sees one grey all round, out
//! to `reach`, so every pass leaves it as it is, and its neighbours read it unchanged; a flat
//! area, lit, dark or grey, costs nothing past finding its edges, which come from the runs.

use super::{CANCEL_EVERY, push};
use crate::{Cancel, Run};

/// `runs`, a layer of `res` pixels, blurred `passes` times by a box `2 · radius + 1` pixels wide.
/// Unchanged when there is nothing to blur, when cancelled, or when the rows to blur do not fit
/// in memory.
pub(crate) fn blur(runs: &[Run], res: (u32, u32), radius: u32, passes: u8, cancel: &Cancel) -> Vec<Run> {
    blur_with(runs, res, radius, passes, cancel, box_columns)
}

/// The step down the columns: `box_columns`, or in the benchmark the way it was done before.
type Columns = fn(&[u8], &mut [u8], usize, &[Vec<(usize, usize)>], usize, &mut Vec<u32>, &Cancel) -> bool;

fn blur_with(runs: &[Run], res: (u32, u32), radius: u32, passes: u8, cancel: &Cancel, columns: Columns) -> Vec<Run> {
    let (w, h) = (u64::from(res.0), u64::from(res.1));
    let reach = u64::from(radius) * u64::from(passes);
    if reach == 0 || w == 0 || h == 0 {
        return runs.to_vec();
    }
    let Some((first, last)) = lit_span(runs) else { return runs.to_vec() };
    let top = (first / w).saturating_sub(reach);
    let bottom = (last / w).saturating_add(reach).min(h - 1);
    let (Ok(width), Ok(rows), Ok(r), Ok(reach)) =
        (usize::try_from(w), usize::try_from(bottom - top + 1), usize::try_from(radius), usize::try_from(reach))
    else {
        return runs.to_vec();
    };
    let Some(len) = width.checked_mul(rows) else { return runs.to_vec() };
    let (mut image, mut other) = (Vec::new(), Vec::new());
    if image.try_reserve_exact(len).is_err() || other.try_reserve_exact(len).is_err() {
        return runs.to_vec();
    }
    let segments = band_rows(runs, top * w, width, rows);
    let spans = active_spans(&segments, width, reach);
    decode(runs, top * w, len, &mut image);
    // Off the spans both buffers keep the layer as it was: what the passes read there.
    other.extend_from_slice(&image);
    let mut sums = Vec::new();
    for _ in 0..passes {
        for ((src, dst), row) in image.chunks_exact(width).zip(other.chunks_exact_mut(width)).zip(&spans) {
            for &(a, b) in row {
                if let Some(dst) = dst.get_mut(a..b) {
                    box_span(src, dst, a, r);
                }
            }
        }
        if !columns(&other, &mut image, width, &spans, r, &mut sums, cancel) {
            return runs.to_vec();
        }
    }
    let mut out = Vec::with_capacity(runs.len());
    push(&mut out, top * w, 0);
    for ((row, active), pixels) in segments.iter().zip(&spans).zip(image.chunks_exact(width)) {
        encode_row(row, active, pixels, &mut out);
    }
    let covered: u64 = out.iter().map(|r| r.len).sum();
    push(&mut out, (w * h).saturating_sub(covered), 0);
    out
}

/// The first and last lit pixels' indices.
fn lit_span(runs: &[Run]) -> Option<(u64, u64)> {
    let (mut at, mut span) = (0u64, None);
    for r in runs {
        if r.grey > 0 && r.len > 0 {
            let first = span.map_or(at, |(f, _)| f);
            span = Some((first, at.saturating_add(r.len) - 1));
        }
        at = at.saturating_add(r.len);
    }
    span
}

/// The `len` pixels from `start` on into `out`, dark where the runs stop short.
fn decode(runs: &[Run], start: u64, len: usize, out: &mut Vec<u8>) {
    let end = start + len as u64;
    let mut at = 0u64;
    for r in runs {
        let (a, b) = (at.max(start), at.saturating_add(r.len).min(end));
        if b > a {
            let dark = ((a - start) as usize).saturating_sub(out.len());
            out.resize(out.len() + dark, 0);
            out.resize(out.len() + (b - a) as usize, r.grey);
        }
        at = at.saturating_add(r.len);
        if at >= end {
            break;
        }
    }
    out.resize(len, 0);
}

/// Where a grey starts in a row; it lasts to the next segment's start, or the row's end.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Segment {
    x: usize,
    grey: u8,
}

/// A row with no light.
const DARK: [Segment; 1] = [Segment { x: 0, grey: 0 }];

/// `rows` rows of `width` pixels from pixel `start` on, each as its segments, the first at 0 and
/// neighbours of one grey merged; dark past the runs.
fn band_rows(runs: &[Run], start: u64, width: usize, rows: usize) -> Vec<Vec<Segment>> {
    let w = width as u64;
    let end = start + w * rows as u64;
    let mut out: Vec<Vec<Segment>> = vec![Vec::new(); rows];
    let mut add = |p: u64, grey: u8| {
        let rel = p - start;
        let Some(row) = out.get_mut((rel / w) as usize) else { return };
        let x = (rel % w) as usize;
        match row.last() {
            Some(s) if s.grey == grey => {}
            _ => row.push(Segment { x, grey }),
        }
    };
    let mut at = 0u64;
    for r in runs {
        let (a, b) = (at.max(start), at.saturating_add(r.len).min(end));
        // Once in each row the run covers.
        let mut p = a;
        while p < b {
            add(p, r.grey);
            p = (start + ((p - start) / w + 1) * w).min(b);
        }
        at = at.saturating_add(r.len);
        if at >= end {
            break;
        }
    }
    if at < end {
        // The runs stop short: dark from there on, every later row starting dark below.
        add(at.max(start), 0);
    }
    for row in &mut out {
        if row.first().is_none_or(|s| s.x != 0) {
            row.insert(0, Segment { x: 0, grey: 0 });
        }
    }
    out
}

/// Each row's active spans, `[a, b)` sorted and apart: the pixels within `reach` of two
/// neighbours of different greys, the screen's edges and the rows past the band counting dark.
/// A few pixels more do no harm: the blur is worked out in full on them.
fn active_spans(rows: &[Vec<Segment>], width: usize, reach: usize) -> Vec<Vec<(usize, usize)>> {
    let widen = |a: usize, b: usize| (a.saturating_sub(reach), b.saturating_add(reach).min(width));
    let mut seeds: Vec<Vec<(usize, usize)>> = vec![Vec::new(); rows.len()];
    for (y, row) in rows.iter().enumerate() {
        let Some(seed) = seeds.get_mut(y) else { continue };
        // Across the row: a grey starting where the one before (dark left of the screen) differs.
        let mut before = 0;
        for s in row {
            if s.grey != before {
                seed.push(widen(s.x, s.x));
            }
            before = s.grey;
        }
        if before != 0 {
            seed.push(widen(width, width));
        }
        // From the row above, dark above the band. Seeding the lower row reaches one row higher
        // than the change needs.
        let above = y.checked_sub(1).and_then(|y| rows.get(y)).map_or(&DARK[..], Vec::as_slice);
        differences(above, row, width, &mut |a, b| seed.push(widen(a, b)));
        if y + 1 == rows.len() {
            differences(row, &DARK, width, &mut |a, b| seed.push(widen(a, b)));
        }
    }
    for seed in &mut seeds {
        *seed = merge(std::mem::take(seed));
    }
    dilate(seeds, reach)
}

/// Call `out(a, b)` for each stretch `[a, b)` where rows `p` and `q` differ.
fn differences(p: &[Segment], q: &[Segment], width: usize, out: &mut dyn FnMut(usize, usize)) {
    let grey = |row: &[Segment], i: usize| row.get(i).map_or(0, |s| s.grey);
    let next = |row: &[Segment], i: usize| row.get(i + 1).map_or(width, |s| s.x);
    let (mut i, mut j, mut x) = (0, 0, 0);
    let mut open = None;
    while x < width {
        let differ = grey(p, i) != grey(q, j);
        match (differ, open) {
            (true, None) => open = Some(x),
            (false, Some(a)) => {
                out(a, x);
                open = None;
            }
            _ => {}
        }
        let (np, nq) = (next(p, i), next(q, j));
        x = np.min(nq);
        if np == x {
            i += 1;
        }
        if nq == x {
            j += 1;
        }
    }
    if let Some(a) = open {
        out(a, width);
    }
}

/// `spans` sorted, overlapping or touching ones joined.
fn merge(mut spans: Vec<(usize, usize)>) -> Vec<(usize, usize)> {
    spans.sort_unstable();
    let mut out: Vec<(usize, usize)> = Vec::with_capacity(spans.len());
    for (a, b) in spans {
        match out.last_mut() {
            Some(last) if a <= last.1 => last.1 = last.1.max(b),
            _ => out.push((a, b)),
        }
    }
    out
}

/// Each row's spans joined with those of the `reach` rows above and below it. In steps: if every
/// row holds its window of `m` rows each way, joining it with the rows `d ≤ 2m + 1` above and
/// below gives the window of `m + d`, so the window doubles at each step. Past the first or last
/// row the nearest one stands in: its window holds every row the missing one would bring.
fn dilate(mut rows: Vec<Vec<(usize, usize)>>, reach: usize) -> Vec<Vec<(usize, usize)>> {
    let mut m = 0;
    while m < reach {
        let d = (2 * m + 1).min(reach - m);
        let last = rows.len().saturating_sub(1);
        let next = (0..rows.len())
            .map(|y| {
                let mut joined = Vec::new();
                for near in [y.saturating_sub(d), y, y.saturating_add(d).min(last)] {
                    if let Some(spans) = rows.get(near) {
                        joined.extend_from_slice(spans);
                    }
                }
                merge(joined)
            })
            .collect();
        rows = next;
        m += d;
    }
    rows
}

/// `(sum + d / 2) / d` as a grey.
fn mean(sum: u32, d: u32) -> u8 {
    ((sum + d / 2) / d).min(255) as u8
}

/// `dst[k]`, pixel `a + k` of the row: the mean of `src` over `[a + k − r, a + k + r]`, dark past
/// either end.
fn box_span(src: &[u8], dst: &mut [u8], a: usize, r: usize) {
    let d = 2 * r as u32 + 1;
    let at = |i: usize| src.get(i).map_or(0, |&v| u32::from(v));
    // The window before the first pixel's: [a − r, a + r).
    let mut sum: u32 = (a.saturating_sub(r)..a + r).map(at).sum();
    for (x, out) in (a..).zip(dst.iter_mut()) {
        sum += at(x + r);
        *out = mean(sum, d);
        if let Some(left) = x.checked_sub(r) {
            sum -= at(left);
        }
    }
}

/// Down the columns of `src`, rows of `width`, over each row's active spans: the mean of the
/// `2r + 1` rows around, dark past the band, into `dst`. False when cancelled.
///
/// `sums` holds each column's sum over the window, carried down from the row above where the
/// column was active there too: one row comes in, one goes out. A column entering a span sums
/// its window afresh; the spans are dilated `reach` rows each way, so that is once per stretch
/// of at least `2 · reach + 1` rows.
fn box_columns(src: &[u8], dst: &mut [u8], width: usize, spans: &[Vec<(usize, usize)>], r: usize, sums: &mut Vec<u32>, cancel: &Cancel) -> bool {
    let d = 2 * r as u32 + 1;
    let rows = spans.len();
    sums.clear();
    sums.resize(width, 0);
    let line = |y: usize, a: usize, b: usize| src.get(y * width + a..y * width + b);
    let mut above: &[(usize, usize)] = &[];
    for (y, (out, row)) in dst.chunks_exact_mut(width).zip(spans).enumerate() {
        if (y as u64).is_multiple_of(CANCEL_EVERY) && cancel.is_cancelled() {
            return false;
        }
        let (incoming, outgoing) = ((y + r < rows).then_some(y + r), y.checked_sub(r + 1));
        let mut k = 0;
        for &(a, b) in row {
            let mut x = a;
            while x < b {
                while above.get(k).is_some_and(|&(_, e)| e <= x) {
                    k += 1;
                }
                let (end, carried) = match above.get(k) {
                    Some(&(c, e)) if c <= x => (e.min(b), true),
                    next => (next.map_or(b, |&(c, _)| c.clamp(x, b)), false),
                };
                let Some(s) = sums.get_mut(x..end) else { break };
                if carried {
                    if let Some(v) = incoming.and_then(|y| line(y, x, end)) {
                        s.iter_mut().zip(v).for_each(|(s, &v)| *s += u32::from(v));
                    }
                    if let Some(v) = outgoing.and_then(|y| line(y, x, end)) {
                        s.iter_mut().zip(v).for_each(|(s, &v)| *s = s.saturating_sub(u32::from(v)));
                    }
                } else {
                    s.fill(0);
                    for near in y.saturating_sub(r)..(y + r + 1).min(rows) {
                        if let Some(v) = line(near, x, end) {
                            s.iter_mut().zip(v).for_each(|(s, &v)| *s += u32::from(v));
                        }
                    }
                }
                x = end;
            }
            if let (Some(out), Some(s)) = (out.get_mut(a..b), sums.get(a..b)) {
                out.iter_mut().zip(s).for_each(|(o, &s)| *o = mean(s, d));
            }
        }
        above = row;
    }
    true
}

/// One row back to runs: its segments as they were off the active spans, the blurred pixels on
/// them.
fn encode_row(segments: &[Segment], active: &[(usize, usize)], pixels: &[u8], out: &mut Vec<Run>) {
    let width = pixels.len();
    let mut i = 0;
    // The segments' pixels over [from, to).
    let mut copy = |from: usize, to: usize, out: &mut Vec<Run>| {
        while let Some(s) = segments.get(i) {
            let end = segments.get(i + 1).map_or(width, |n| n.x);
            let (a, b) = (s.x.max(from), end.min(to));
            if b > a {
                push(out, (b - a) as u64, s.grey);
            }
            if end > to {
                break;
            }
            i += 1;
        }
    };
    let mut x = 0;
    for &(a, b) in active {
        copy(x, a, out);
        for same in pixels.get(a..b).unwrap_or_default().chunk_by(|p, q| p == q) {
            if let Some(&grey) = same.first() {
                push(out, same.len() as u64, grey);
            }
        }
        x = b;
    }
    copy(x, width, out);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(len: u64, grey: u8) -> Run {
        Run { len, grey }
    }

    fn pixels(runs: &[Run], len: usize) -> Vec<u8> {
        let mut out = Vec::new();
        decode(runs, 0, len, &mut out);
        out
    }

    fn to_runs(image: &[u8]) -> Vec<Run> {
        let mut runs = Vec::new();
        for same in image.chunk_by(|a, b| a == b) {
            push(&mut runs, same.len() as u64, same[0]);
        }
        runs
    }

    /// The blur on every pixel of the whole screen, the plain way: what the spans must match.
    fn reference(runs: &[Run], res: (u32, u32), radius: u32, passes: u8) -> Vec<Run> {
        let (w, h) = (res.0 as usize, res.1 as usize);
        let r = radius as usize;
        let d = 2 * radius + 1;
        let mut image = pixels(runs, w * h);
        let mut other = vec![0u8; w * h];
        let at = |img: &[u8], x: isize, y: isize| {
            if x < 0 || y < 0 || x >= w as isize || y >= h as isize { 0 } else { u32::from(img[y as usize * w + x as usize]) }
        };
        for _ in 0..passes {
            for y in 0..h as isize {
                for x in 0..w as isize {
                    let sum: u32 = (-(r as isize)..=r as isize).map(|k| at(&image, x + k, y)).sum();
                    other[y as usize * w + x as usize] = mean(sum, d);
                }
            }
            for y in 0..h as isize {
                for x in 0..w as isize {
                    let sum: u32 = (-(r as isize)..=r as isize).map(|k| at(&other, x, y + k)).sum();
                    image[y as usize * w + x as usize] = mean(sum, d);
                }
            }
        }
        to_runs(&image)
    }

    /// xorshift64*: test data without a dependency.
    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 >> 12;
            self.0 ^= self.0 << 25;
            self.0 ^= self.0 >> 27;
            self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
        }

        fn below(&mut self, n: u64) -> u64 {
            self.next() % n
        }
    }

    #[test]
    fn a_lit_pixel_spreads_over_its_box_and_keeps_its_light() {
        // 10 × 8, pixel (4, 3) lit.
        let runs = [run(34, 0), run(1, 255), run(45, 0)];
        let out = blur(&runs, (10, 8), 1, 1, &Cancel::default());
        assert_eq!(out.iter().map(|r| r.len).sum::<u64>(), 80);
        let img = pixels(&out, 80);
        for y in 0..8 {
            for x in 0..10 {
                let inside = (3..=5).contains(&x) && (2..=4).contains(&y);
                assert_eq!(img[y * 10 + x], if inside { 28 } else { 0 }, "({x}, {y})");
            }
        }
        // Two passes reach twice as far, a peak in the middle.
        let img = pixels(&blur(&runs, (10, 8), 1, 2, &Cancel::default()), 80);
        assert!(img[3 * 10 + 4] > img[3 * 10 + 6] && img[3 * 10 + 6] > 0 && img[3 * 10 + 7] == 0);
    }

    #[test]
    fn a_wide_area_keeps_its_light_and_only_its_edges_turn_grey() {
        // 40 × 40, a 20 × 20 block.
        let mut img = vec![0u8; 1600];
        for y in 10..30 {
            img[y * 40 + 10..y * 40 + 30].fill(255);
        }
        let out = pixels(&blur(&to_runs(&img), (40, 40), 2, 3, &Cancel::default()), 1600);
        let light = |v: &[u8]| v.iter().map(|&g| u64::from(g)).sum::<u64>();
        let (a, b) = (light(&img) as f64, light(&out) as f64);
        assert!((a - b).abs() / a < 0.01, "{a} → {b}");
        assert_eq!(out[20 * 40 + 20], 255);
        assert!(out.iter().any(|&g| g > 0 && g < 255));
    }

    #[test]
    fn the_spans_match_the_blur_of_every_pixel() {
        let (w, h) = (37usize, 23usize);
        let mut shapes: Vec<Vec<u8>> = Vec::new();
        // A flat screen, lit and grey; one pixel; each corner; halves split along X and along Y.
        shapes.push(vec![255; w * h]);
        shapes.push(vec![90; w * h]);
        for p in [0, w - 1, w * (h - 1), w * h - 1, 11 * w + 17] {
            let mut img = vec![0; w * h];
            img[p] = 255;
            shapes.push(img);
        }
        shapes.push((0..w * h).map(|p| if p % w < w / 2 { 255 } else { 0 }).collect());
        shapes.push((0..w * h).map(|p| if p / w < h / 2 { 0 } else { 200 }).collect());
        // Random blocks of random greys over a random background, flat greys among them.
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
        for _ in 0..60 {
            let mut img = vec![[0, 0, 255, 128][rng.below(4) as usize]; w * h];
            for _ in 0..rng.below(8) {
                let (x0, y0) = (rng.below(w as u64) as usize, rng.below(h as u64) as usize);
                let (x1, y1) = ((x0 + 1 + rng.below(20) as usize).min(w), (y0 + 1 + rng.below(12) as usize).min(h));
                let grey = [255, 0, 64, rng.below(256) as u8][rng.below(4) as usize];
                for y in y0..y1 {
                    img[y * w + x0..y * w + x1].fill(grey);
                }
            }
            shapes.push(img);
        }
        for (k, img) in shapes.iter().enumerate() {
            let runs = to_runs(img);
            // Radius × passes past the screen too.
            for (radius, passes) in [(1, 1), (2, 1), (1, 3), (3, 2), (5, 4), (16, 4)] {
                let fast = blur(&runs, (w as u32, h as u32), radius, passes, &Cancel::default());
                assert_eq!(
                    pixels(&fast, w * h),
                    pixels(&reference(&runs, (w as u32, h as u32), radius, passes), w * h),
                    "shape {k}, {radius} × {passes}"
                );
                assert_eq!(fast.iter().map(|r| r.len).sum::<u64>(), (w * h) as u64);
            }
        }
    }

    #[test]
    fn the_band_rows_and_differences_follow_the_runs() {
        // 5 wide: a lit run across a row end, then the runs stop short.
        let runs = [run(3, 0), run(4, 255), run(2, 0)];
        let rows = band_rows(&runs, 0, 5, 3);
        let s = |x, grey| Segment { x, grey };
        assert_eq!(rows, [vec![s(0, 0), s(3, 255)], vec![s(0, 255), s(2, 0)], vec![s(0, 0)]]);
        let mut found = Vec::new();
        differences(&rows[0], &rows[1], 5, &mut |a, b| found.push((a, b)));
        assert_eq!(found, [(0, 2), (3, 5)]);
        // A band past the top rows starts dark.
        assert_eq!(band_rows(&runs, 5, 5, 2), [vec![s(0, 255), s(2, 0)], vec![s(0, 0)]]);
        // A reach past the band's ends still reaches every row.
        assert_eq!(dilate(vec![vec![(0, 1)], vec![], vec![], vec![]], 9), vec![vec![(0, 1)]; 4]);
        assert_eq!(dilate(vec![vec![], vec![], vec![], vec![(0, 1)]], 9), vec![vec![(0, 1)]; 4]);
        assert_eq!(
            dilate(vec![vec![], vec![], vec![(1, 2)], vec![], vec![], vec![]], 2),
            [vec![(1, 2)], vec![(1, 2)], vec![(1, 2)], vec![(1, 2)], vec![(1, 2)], vec![]]
        );
    }

    #[test]
    fn nothing_to_blur_and_screen_edges_never_panic() {
        let dark = [run(80, 0)];
        assert_eq!(blur(&dark, (10, 8), 3, 2, &Cancel::default()), dark);
        let lit = [run(80, 255)];
        assert_eq!(blur(&lit, (10, 8), 0, 2, &Cancel::default()), lit);
        // Corners, a radius past the screen, runs short or long.
        for runs in [vec![run(1, 255), run(79, 0)], vec![run(79, 0), run(1, 255)], vec![run(5, 200)], vec![run(500, 90)]] {
            let out = blur(&runs, (10, 8), 16, 4, &Cancel::default());
            assert_eq!(out.iter().map(|r| r.len).sum::<u64>(), 80, "{runs:?}");
            assert_eq!(pixels(&out, 80), pixels(&reference(&runs, (10, 8), 16, 4), 80), "{runs:?}");
        }
        assert_eq!(blur(&lit, (0, 8), 1, 1, &Cancel::default()), lit);
    }

    #[test]
    fn a_cancelled_blur_gives_the_runs_back() {
        let cancel = Cancel::default();
        cancel.cancel();
        let runs = [run(34, 0), run(1, 255), run(45, 0)];
        assert_eq!(blur(&runs, (10, 8), 1, 1, &cancel), runs);
    }
}
