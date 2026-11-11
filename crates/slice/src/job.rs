//! A sliced job: every layer as runs of grey, with what the printer needs to expose it.

use serde::{Deserialize, Serialize};

use crate::{PrintSettings, PrinterProfile};

/// `len` pixels of one grey level, row-major, X fastest. 0 is dark, 255 fully lit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Run {
    pub len: u64,
    pub grey: u8,
}

/// One layer. Its runs cover exactly `resolution_x · resolution_y` pixels: a full bitmap would be
/// 33 MB per layer on an 8K screen.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Layer {
    pub runs: Vec<Run>,
    /// The layer's top above the plate, mm: `(i + 1) · layer height`.
    pub z: f32,
    /// Seconds, from [`PrintSettings::exposure_for`].
    pub exposure: f32,
    /// Seconds the light stays off before this layer's exposure.
    pub light_off: f32,
    /// Lit pixels, weighted by grey: a pixel at grey 128 counts about half.
    pub lit: u64,
}

/// What the slicer measured.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct JobStats {
    pub volume_ml: f64,
    pub print_time_s: f64,
    /// Solids that cross the plate's edge, go under it or rise past the build height: what is
    /// outside is not printed.
    pub clipped: u32,
    /// Scanlines whose crossings do not add up to zero: an open or inside-out mesh.
    pub open_rows: u64,
}

/// What a job was sliced from: the scene revision and hashes of the print and support settings.
/// A job whose stamp no longer matches them is stale.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Stamp {
    pub scene: u64,
    pub print: u64,
    pub supports: u64,
}

/// A whole job, with the profile and settings it was sliced for.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SlicedJob {
    pub layers: Vec<Layer>,
    pub profile: PrinterProfile,
    pub settings: PrintSettings,
    pub stats: JobStats,
    #[serde(default)]
    pub stamp: Stamp,
}

impl SlicedJob {
    /// The print's height, mm: the last layer's top.
    pub fn height(&self) -> f64 {
        self.layers.last().map_or(0.0, |l| f64::from(l.z))
    }
}

impl Layer {
    /// The layer as a `width · height` bitmap of grey levels, row-major, into `out` (cleared
    /// first). Runs that add up to more are cut; fewer leave the rest dark.
    pub fn decode_into(&self, width: u32, height: u32, out: &mut Vec<u8>) {
        let total = usize::try_from(u64::from(width) * u64::from(height)).unwrap_or(0);
        out.clear();
        out.reserve_exact(total);
        for r in &self.runs {
            let room = total - out.len();
            if room == 0 {
                break;
            }
            let n = usize::try_from(r.len).unwrap_or(usize::MAX).min(room);
            out.resize(out.len() + n, r.grey);
        }
        out.resize(total, 0);
    }

    /// How many separate lit regions the layer has, a lit pixel joining its eight neighbours.
    /// Read from the runs: no bitmap is built.
    pub fn regions(&self, width: u32) -> u32 {
        let width = u64::from(width);
        if width == 0 {
            return 0;
        }
        let mut sets = UnionFind::default();
        // The lit spans of the row before and of this one: (start, end, set).
        let (mut above, mut here): (Vec<(u64, u64, usize)>, Vec<(u64, u64, usize)>) = (Vec::new(), Vec::new());
        let mut row = 0u64;
        let mut at = 0u64;
        let join = |above: &[(u64, u64, usize)], start: u64, end: u64, sets: &mut UnionFind| {
            let id = sets.add();
            for &(a, b, s) in above {
                // Diagonal neighbours touch too: a span ending at `b` reaches column `b`.
                if a <= end && start <= b {
                    sets.union(id, s);
                }
            }
            id
        };
        for r in &self.runs {
            let (mut from, to) = (at, at.saturating_add(r.len));
            at = to;
            if r.grey == 0 {
                continue;
            }
            while from < to {
                let (y, x) = (from / width, from % width);
                if y != row {
                    above = if y == row + 1 { std::mem::take(&mut here) } else { Vec::new() };
                    here.clear();
                    row = y;
                }
                let end = (to - y * width).min(width);
                let id = join(&above, x, end, &mut sets);
                here.push((x, end, id));
                from = y * width + end;
            }
        }
        sets.roots()
    }
}

/// Disjoint sets of lit spans, for [`Layer::regions`].
#[derive(Default)]
struct UnionFind {
    parent: Vec<usize>,
}

impl UnionFind {
    fn add(&mut self) -> usize {
        self.parent.push(self.parent.len());
        self.parent.len() - 1
    }

    fn find(&mut self, mut i: usize) -> usize {
        while let Some(&p) = self.parent.get(i)
            && p != i
        {
            let grand = self.parent.get(p).copied().unwrap_or(p);
            if let Some(slot) = self.parent.get_mut(i) {
                *slot = grand;
            }
            i = p;
        }
        i
    }

    fn union(&mut self, a: usize, b: usize) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra != rb
            && let Some(slot) = self.parent.get_mut(ra)
        {
            *slot = rb;
        }
    }

    fn roots(&self) -> u32 {
        self.parent.iter().enumerate().filter(|&(i, &p)| i == p).count() as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A layer from a bitmap of `width` columns.
    fn layer(bits: &[u8]) -> Layer {
        let mut runs: Vec<Run> = Vec::new();
        for &g in bits {
            match runs.last_mut() {
                Some(r) if r.grey == g => r.len += 1,
                _ => runs.push(Run { len: 1, grey: g }),
            }
        }
        Layer { runs, ..Default::default() }
    }

    #[test]
    fn decoding_fills_the_bitmap_exactly() {
        let l = Layer { runs: vec![Run { len: 3, grey: 0 }, Run { len: 2, grey: 255 }, Run { len: 7, grey: 128 }], ..Default::default() };
        let mut out = vec![9; 100];
        l.decode_into(4, 3, &mut out);
        assert_eq!(out, [0, 0, 0, 255, 255, 128, 128, 128, 128, 128, 128, 128]);
        l.decode_into(2, 2, &mut out);
        assert_eq!(out, [0, 0, 0, 255], "runs past the end are cut");
        l.decode_into(5, 3, &mut out);
        assert_eq!(out.len(), 15);
        assert_eq!(&out[12..], [0, 0, 0], "missing pixels stay dark");
    }

    #[test]
    fn regions_join_across_rows_and_corners() {
        #[rustfmt::skip]
        let two_squares = layer(&[
            1, 1, 0, 0, 0,
            1, 1, 0, 0, 0,
            0, 0, 0, 1, 1,
            0, 0, 0, 1, 1,
        ]);
        assert_eq!(two_squares.regions(5), 2);
        #[rustfmt::skip]
        let diagonal = layer(&[
            1, 0, 0, 0, 0,
            0, 1, 0, 0, 0,
            0, 0, 1, 0, 0,
            0, 0, 0, 0, 1,
        ]);
        assert_eq!(diagonal.regions(5), 2, "corners touch; a gap row apart does not");
        #[rustfmt::skip]
        let u = layer(&[
            1, 0, 0, 0, 1,
            1, 0, 0, 0, 1,
            1, 1, 1, 1, 1,
        ]);
        assert_eq!(u.regions(5), 1, "joined below");
        #[rustfmt::skip]
        let wrap = layer(&[
            0, 0, 0, 0, 1,
            1, 0, 0, 0, 0,
        ]);
        assert_eq!(wrap.regions(5), 2, "a run over a row's end is two spans, not neighbours");
        let full = layer(&[255; 20]);
        assert_eq!(full.regions(5), 1);
        assert_eq!(layer(&[0; 20]).regions(5), 0);
    }
}
