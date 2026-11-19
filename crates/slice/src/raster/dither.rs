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

/// Layer `layer`'s `runs`, rows of `width` pixels, with every grey under `d.floor` dithered.
pub(crate) fn dither(runs: &[Run], width: u32, layer: u32, d: Dither, cancel: &Cancel) -> Vec<Run> {
    let width = u64::from(width.max(1));
    let floor = d.floor.max(1);
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
                let lit = share > d.threshold(p % width, p / width, layer);
                push(&mut out, 1, if lit { floor } else { 0 });
            }
        }
        at = end;
    }
    out
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
        (0..layers).map(|i| bitmap(&dither(&[Run { len: 1, grey: grey(i) }], 1, i, d, &Cancel::default()))[0]).collect()
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
            let lit = |d| bitmap(&dither(&row, 1000, layer, d, &cancel)).iter().filter(|&&g| g > 0).count();
            let spread = lit(BOTH);
            assert!((spread as f64 - 251.0).abs() < 30.0, "layer {layer}: {spread}");
            // Without XY the whole row is lit or dark together: the hairlines XY is for.
            assert!([0, 1000].contains(&lit(Dither { xy: false, ..BOTH })));
        }
        // XY alone: the same pattern every layer, each pixel switching once as its grey rises.
        let xy = Dither { z: false, ..BOTH };
        assert_eq!(dither(&row, 1000, 0, xy, &cancel), dither(&row, 1000, 7, xy, &cancel));
    }

    #[test]
    fn greys_from_the_floor_up_are_kept_and_the_rest_become_it_or_dark() {
        let runs =
            [Run { len: 5, grey: 0 }, Run { len: 40, grey: 50 }, Run { len: 3, grey: 120 }, Run { len: 4, grey: 255 }, Run { len: 8, grey: 100 }];
        let out = dither(&runs, 7, 3, Dither { floor: 100, ..BOTH }, &Cancel::default());
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
