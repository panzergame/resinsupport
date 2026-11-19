// The spatial threshold is interleaved gradient noise, from its published formula (Jimenez, "Next
// Generation Post Processing in Call of Duty: Advanced Warfare", SIGGRAPH 2014); no code taken.

//! Dithering: greys under a floor, which the resin does not cure reliably, become the floor or
//! dark, a grey `g` lit with probability `g / floor`, so each pixel gives on average the light it
//! had. A pixel compares its grey with a threshold of its own, from where it is and which layer:
//! nothing passes from one layer to the next, so layers still slice in any order, on any thread.
//!
//! Along Z the threshold steps by the golden ratio's inverse each layer: over any run of layers a
//! pixel is lit in close to its grey's share of them, the lit layers as evenly spread as can be
//! (three gaps at most, by the three-distance theorem). A wall leaning by a fraction of a pixel a
//! layer then turns its edge pixel on more and more often, rather than all at once in one layer
//! that shows as a band. Across X and Y each pixel starts from the noise, so neighbours along an
//! edge switch on at different layers rather than together.
//!
//! Only surfaces leaning less than a limit from vertical gain from it: on flatter ones the greys
//! come from the layer's Z cuts and set how deep the pixel cures, and dithering them scatters
//! lit pixels over the surface. A pixel's lean shows in how much its grey changes between the
//! layer's lowest and highest cut: an edge leaning θ moves `tan θ` times their distance apart.
//! Pixels that change more than the limit allows ([`steep`]), grown by the blur's reach
//! ([`dilate`]) so the greys the blur spreads from them go with them, keep their greys.

use std::collections::BTreeMap;

use super::push;
use crate::{Cancel, Run};

/// The golden ratio's inverse.
const STEP: f64 = 0.618_033_988_749_894_9;

/// Dithered pixels between checks of the cancel flag.
const CANCEL_EVERY: u64 = 1 << 20;

/// What to dither and against what.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Dither {
    /// The threshold moves from layer to layer.
    pub(crate) z: bool,
    /// The threshold differs from pixel to pixel.
    pub(crate) xy: bool,
    /// Greys from it up are kept; those under it become it or 0.
    pub(crate) floor: u8,
}

impl Dither {
    /// Pixel `(x, y)`'s threshold on layer `layer`, in [0, 1).
    fn threshold(&self, x: u64, y: u64, layer: u32) -> f64 {
        let mut t = 0.5;
        if self.z {
            t += (STEP * f64::from(layer)).fract();
        }
        if self.xy {
            t += interleaved_gradient_noise(x, y);
        }
        t.fract()
    }
}

fn interleaved_gradient_noise(x: u64, y: u64) -> f64 {
    (52.982_918_9 * (0.067_110_56 * x as f64 + 0.005_837_15 * y as f64).fract()).fract()
}

/// Layer `layer`'s `runs`, rows of `width` pixels, with every grey under `d.floor` dithered but
/// those lit in `steep`.
pub(crate) fn dither(runs: &[Run], width: u32, layer: u32, d: Dither, steep: Option<&[Run]>, cancel: &Cancel) -> Vec<Run> {
    let width = u64::from(width.max(1));
    let floor = d.floor.max(1);
    let mut steep = steep.map(Reader::new);
    let mut out = Vec::with_capacity(runs.len());
    let mut at = 0u64;
    let mut done = 0u64;
    for r in runs {
        let end = at.saturating_add(r.len);
        if r.grey == 0 || r.grey >= floor {
            push(&mut out, r.len, r.grey);
        } else {
            let share = f64::from(r.grey) / f64::from(floor);
            for p in at..end {
                done += 1;
                if done.is_multiple_of(CANCEL_EVERY) && cancel.is_cancelled() {
                    return out;
                }
                if steep.as_mut().is_some_and(|s| s.at(p).0 > 0) {
                    push(&mut out, 1, r.grey);
                    continue;
                }
                let lit = share > d.threshold(p % width, p / width, layer);
                push(&mut out, 1, if lit { floor } else { 0 });
            }
        }
        at = end;
    }
    out
}

/// How much a pixel's grey may change from the layer's lowest cut to its highest when its surface
/// leans `max_angle`° from vertical, `planes` cuts evenly through a layer `layer_height` mm high,
/// on pixels `pitch` mm wide. `None` when no pixel can be steeper: one cut, a limit of 90° or
/// more, or one so wide no change of grey reaches it.
pub(crate) fn steep_grey(max_angle: f64, layer_height: f64, planes: u8, pitch: f64) -> Option<f64> {
    if planes < 2 || !(max_angle < 90.0) || !(pitch > 0.0) {
        return None;
    }
    let apart = layer_height * f64::from(planes - 1) / f64::from(planes);
    let t = 255.0 * max_angle.max(0.0).to_radians().tan() * apart / pitch;
    (t < 255.0).then_some(t)
}

/// The pixels whose grey spreads more than `threshold` across `cuts`, each `pixels` long: lit,
/// the rest dark.
pub(crate) fn steep(cuts: &[Vec<Run>], pixels: u64, threshold: f64) -> Vec<Run> {
    let mut out = Vec::new();
    if cuts.len() < 2 {
        push(&mut out, pixels, 0);
        return out;
    }
    let mut readers: Vec<Reader> = cuts.iter().map(|c| Reader::new(c)).collect();
    let mut at = 0;
    while at < pixels {
        let (mut lo, mut hi, mut end) = (u8::MAX, 0u8, pixels);
        for r in &mut readers {
            let (g, e) = r.at(at);
            (lo, hi, end) = (lo.min(g), hi.max(g), end.min(e));
        }
        push(&mut out, end - at, if f64::from(hi.saturating_sub(lo)) > threshold { 255 } else { 0 });
        at = end;
    }
    out
}

/// `mask`, a layer of `res` pixels, its lit pixels grown by `reach` every way, a square: as far
/// as a box blur reaching that far spreads them.
pub(crate) fn dilate(mask: &[Run], res: (u32, u32), reach: u64, cancel: &Cancel) -> Vec<Run> {
    let (w, h) = (u64::from(res.0), u64::from(res.1));
    if reach == 0 || w == 0 || h == 0 {
        return mask.to_vec();
    }
    // Each row's lit spans, [start, end) in columns.
    let mut rows: BTreeMap<u64, Vec<(u64, u64)>> = BTreeMap::new();
    let mut at = 0u64;
    for r in mask {
        let end = at.saturating_add(r.len).min(w * h);
        let mut from = at;
        while r.grey > 0 && from < end {
            let y = from / w;
            let stop = end.min((y + 1) * w);
            rows.entry(y).or_default().push((from - y * w, stop - y * w));
            from = stop;
        }
        at = end;
    }
    let mut grown: BTreeMap<u64, Vec<(u64, u64)>> = BTreeMap::new();
    for (&y, spans) in &rows {
        if cancel.is_cancelled() {
            return mask.to_vec();
        }
        let wide: Vec<(u64, u64)> = spans.iter().map(|&(a, b)| (a.saturating_sub(reach), b.saturating_add(reach).min(w))).collect();
        for row in y.saturating_sub(reach)..=y.saturating_add(reach).min(h - 1) {
            grown.entry(row).or_default().extend_from_slice(&wide);
        }
    }
    let mut out = Vec::new();
    let mut last = 0u64;
    for (y, mut spans) in grown {
        spans.sort_unstable();
        let mut merged: Option<(u64, u64)> = None;
        // The last entry, past every span, flushes the one being merged.
        for (a, b) in spans.into_iter().chain([(u64::MAX, u64::MAX)]) {
            match merged {
                Some((ma, mb)) if a <= mb => merged = Some((ma, mb.max(b))),
                Some((ma, mb)) => {
                    push(&mut out, y * w + ma - last, 0);
                    push(&mut out, mb - ma, 255);
                    last = y * w + mb;
                    merged = Some((a, b));
                }
                None => merged = Some((a, b)),
            }
        }
    }
    push(&mut out, (w * h).saturating_sub(last), 0);
    out
}

/// A layer's runs read pixel by pixel, forwards only.
struct Reader<'a> {
    runs: std::slice::Iter<'a, Run>,
    /// Where the current run ends.
    end: u64,
    grey: u8,
}

impl<'a> Reader<'a> {
    fn new(runs: &'a [Run]) -> Self {
        Reader { runs: runs.iter(), end: 0, grey: 0 }
    }

    /// Pixel `p`'s grey and where its run ends, `p` never before the pixel last asked for; dark
    /// past the runs' end.
    fn at(&mut self, p: u64) -> (u8, u64) {
        while p >= self.end {
            let Some(r) = self.runs.next() else { return (0, u64::MAX) };
            (self.end, self.grey) = (self.end.saturating_add(r.len), r.grey);
        }
        (self.grey, self.end)
    }
}

/// The grey before `table` that comes out at `floor` or above: the floor to dither to, so the
/// floor is set in the greys the printer gets.
pub(crate) fn coverage_floor(table: Option<&[u8; 256]>, floor: u8) -> u8 {
    let floor = floor.max(1);
    match table {
        None => floor,
        Some(t) => t.iter().position(|&g| g >= floor).and_then(|i| u8::try_from(i).ok()).unwrap_or(u8::MAX).max(1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::raster::gamma_table;

    const BOTH: Dither = Dither { z: true, xy: true, floor: 255 };

    fn bitmap(runs: &[Run]) -> Vec<u8> {
        runs.iter().flat_map(|r| std::iter::repeat_n(r.grey, r.len as usize)).collect()
    }

    /// Pixel 0's grey on each of `layers` layers, `grey` going in.
    fn pixel_over_layers(d: Dither, grey: impl Fn(u32) -> u8, layers: u32) -> Vec<u8> {
        (0..layers).map(|i| bitmap(&dither(&[Run { len: 1, grey: grey(i) }], 1, i, d, None, &Cancel::default()))[0]).collect()
    }

    #[test]
    fn a_pixel_is_lit_in_its_greys_share_of_the_layers_evenly_spread() {
        for d in [BOTH, Dither { xy: false, ..BOTH }] {
            for grey in [1u8, 3, 64, 128, 200, 254] {
                let out = pixel_over_layers(d, |_| grey, 2000);
                assert!(out.iter().all(|&g| g == 0 || g == 255));
                let lit = out.iter().filter(|&&g| g > 0).count() as f64;
                let want = 2000.0 * f64::from(grey) / 255.0;
                assert!((lit - want).abs() <= 2.0, "{d:?} grey {grey}: {lit} lit, {want} wanted");
                // Evenly: no gap between lit layers much longer than the average one.
                let at: Vec<usize> = out.iter().enumerate().filter(|(_, g)| **g > 0).map(|(i, _)| i).collect();
                let longest = at.windows(2).map(|w| w[1] - w[0]).max().unwrap_or(0) as f64;
                assert!(longest <= 255.0 / f64::from(grey) * 2.0 + 1.0, "{d:?} grey {grey}: gap {longest}");
            }
        }
    }

    #[test]
    fn a_slowly_leaning_edge_keeps_its_light_layer_by_layer() {
        // An edge crossing one pixel in 80 layers: the lit layers so far never stray far from the
        // grey summed so far, where a threshold at half would light all of them from layer 40 on.
        let ramp = |i: u32| ((i % 80) as f64 / 80.0 * 255.0).round() as u8;
        let out = pixel_over_layers(BOTH, ramp, 800);
        let (mut want, mut got) = (0.0, 0.0);
        for (i, g) in out.iter().enumerate() {
            want += f64::from(ramp(i as u32)) / 255.0;
            got += f64::from(*g) / 255.0;
            assert!((want - got).abs() < 3.0, "layer {i}: {got} lit, {want} wanted");
        }
    }

    #[test]
    fn across_the_plate_neighbours_switch_on_apart_unless_xy_is_off() {
        // A row of 1000 pixels at a quarter grey, as an edge leaning along Y leaves them.
        let row = [Run { len: 1000, grey: 64 }];
        let cancel = Cancel::default();
        for layer in 0..20 {
            let lit = |d| bitmap(&dither(&row, 1000, layer, d, None, &cancel)).iter().filter(|&&g| g > 0).count();
            let spread = lit(BOTH);
            assert!((spread as f64 - 251.0).abs() < 30.0, "layer {layer}: {spread}");
            // Without XY the whole row is lit or dark together: the hairlines XY is for.
            assert!([0, 1000].contains(&lit(Dither { xy: false, ..BOTH })));
        }
        // XY alone: the same pattern every layer, each pixel switching once as its grey rises.
        let xy = Dither { z: false, ..BOTH };
        assert_eq!(dither(&row, 1000, 0, xy, None, &cancel), dither(&row, 1000, 7, xy, None, &cancel));
    }

    #[test]
    fn greys_from_the_floor_up_are_kept_and_the_rest_become_it_or_dark() {
        let runs =
            [Run { len: 5, grey: 0 }, Run { len: 40, grey: 50 }, Run { len: 3, grey: 120 }, Run { len: 4, grey: 255 }, Run { len: 8, grey: 100 }];
        let out = dither(&runs, 7, 3, Dither { floor: 100, ..BOTH }, None, &Cancel::default());
        let (a, b) = (bitmap(&runs), bitmap(&out));
        assert_eq!(a.len(), b.len());
        for (x, y) in a.iter().zip(&b) {
            match x {
                0 | 100.. => assert_eq!(x, y),
                _ => assert!([0, 100].contains(y), "{x} became {y}"),
            }
        }
        assert!(b[5..45].contains(&0) && b[5..45].contains(&100));
    }

    #[test]
    fn steep_pixels_keep_their_greys() {
        let runs = [Run { len: 10, grey: 60 }];
        let steep = [Run { len: 3, grey: 0 }, Run { len: 4, grey: 255 }, Run { len: 3, grey: 0 }];
        let out = bitmap(&dither(&runs, 10, 0, BOTH, Some(&steep), &Cancel::default()));
        assert_eq!(out[3..7], [60; 4]);
        assert!(out[..3].iter().chain(&out[7..]).all(|&g| g == 0 || g == 255));
    }

    #[test]
    fn steepness_is_the_spread_of_a_pixels_grey_across_the_cuts() {
        // Three cuts of 6 pixels: spreads 0, 10, 100, 255, 30 and 0.
        let cuts = vec![
            vec![Run { len: 1, grey: 7 }, Run { len: 1, grey: 50 }, Run { len: 1, grey: 0 }, Run { len: 1, grey: 255 }, Run { len: 2, grey: 0 }],
            vec![Run { len: 1, grey: 7 }, Run { len: 1, grey: 55 }, Run { len: 1, grey: 100 }, Run { len: 1, grey: 0 }, Run { len: 2, grey: 0 }],
            vec![
                Run { len: 1, grey: 7 },
                Run { len: 1, grey: 60 },
                Run { len: 1, grey: 50 },
                Run { len: 1, grey: 9 },
                Run { len: 1, grey: 30 },
                Run { len: 1, grey: 0 },
            ],
        ];
        assert_eq!(bitmap(&steep(&cuts, 6, 20.0)), [0, 0, 255, 255, 255, 0]);
        assert_eq!(bitmap(&steep(&cuts, 6, 100.0)), [0, 0, 0, 255, 0, 0]);
        assert_eq!(steep(&cuts[..1], 6, 0.0), [Run { len: 6, grey: 0 }], "one cut: nothing is steep");
    }

    #[test]
    fn dilation_grows_a_square_clipped_at_the_edges() {
        let (w, h) = (9u32, 7u32);
        let mut mask = [0u8; 63];
        mask[2 * 9 + 4] = 255;
        mask[6 * 9 + 8] = 255;
        let runs: Vec<Run> = mask.iter().map(|&g| Run { len: 1, grey: g }).collect();
        let out = bitmap(&dilate(&runs, (w, h), 1, &Cancel::default()));
        assert_eq!(out.len(), 63);
        for y in 0..7 {
            for x in 0..9 {
                let near = |cx: i32, cy: i32| (x - cx).abs() <= 1 && (y - cy).abs() <= 1;
                assert_eq!(out[(y * 9 + x) as usize] > 0, near(4, 2) || near(8, 6), "({x}, {y})");
            }
        }
        assert_eq!(dilate(&runs, (w, h), 0, &Cancel::default()), runs);
        assert_eq!(bitmap(&dilate(&[Run { len: 63, grey: 0 }], (w, h), 3, &Cancel::default())), [0; 63]);
    }

    #[test]
    fn the_angle_limit_becomes_a_change_of_grey() {
        // 8 cuts through 0.05 mm on 0.0285 mm pixels: at 20° an edge moves 0.0159 mm between the
        // lowest and the highest, 0.559 of a pixel.
        let t = steep_grey(20.0, 0.05, 8, 0.0285).unwrap();
        assert!((t - 255.0 * 20f64.to_radians().tan() * 0.04375 / 0.0285).abs() < 1e-9 && (t - 142.5).abs() < 0.5, "{t}");
        assert_eq!(steep_grey(0.0, 0.05, 8, 0.0285), Some(0.0));
        assert_eq!(steep_grey(90.0, 0.05, 8, 0.0285), None);
        assert_eq!(steep_grey(20.0, 0.05, 1, 0.0285), None, "one cut measures nothing");
        assert_eq!(steep_grey(60.0, 0.05, 8, 0.0285), None, "no change of grey reaches it");
        assert_eq!(steep_grey(f64::NAN, 0.05, 8, 0.0285), None);
    }

    #[test]
    fn the_floor_is_read_through_the_gamma() {
        assert_eq!(coverage_floor(None, 200), 200);
        assert_eq!(coverage_floor(None, 0), 1);
        let bright = gamma_table(0.5);
        let f = coverage_floor(Some(&bright), 128);
        assert!(bright[usize::from(f)] >= 128 && bright[usize::from(f) - 1] < 128);
        assert_eq!(coverage_floor(Some(&gamma_table(0.0)), 255), 128);
        assert_eq!(coverage_floor(Some(&gamma_table(1.0)), 255), 255);
    }
}
