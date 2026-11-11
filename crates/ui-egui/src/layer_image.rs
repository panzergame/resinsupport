//! One sliced layer as a bitmap ready for the GPU: the grey levels, cut into tiles no bigger than
//! the device's largest texture, each with its mipmaps, and how its pixels sit on the plate. Built
//! on the window thread when the shown layer changes, never per frame.

use resin_engine::slice::{Layer, PrinterProfile, Stamp};

/// Which layer of which job an image shows.
pub type ImageKey = (Stamp, usize);

/// A rectangle of the image, in pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TileRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// One texture's worth of the image: where it is, and its mipmaps after the first level (the first
/// is read from [`LayerImage::pixels`] in place).
pub struct Tile {
    pub rect: TileRect,
    /// Each level's size and grey levels, halving down to 1 × 1.
    pub mips: Vec<(u32, u32, Vec<u8>)>,
}

/// How the image's pixels lie on the plate: the slicer's placement, read back.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlateMap {
    pub width: u32,
    pub height: u32,
    pub display_x: f64,
    pub display_y: f64,
    pub mirror_x: bool,
    pub mirror_y: bool,
}

impl PlateMap {
    pub fn new(p: &PrinterProfile) -> PlateMap {
        PlateMap {
            width: p.resolution_x,
            height: p.resolution_y,
            display_x: p.display_x,
            display_y: p.display_y,
            mirror_x: p.mirror_x,
            mirror_y: p.mirror_y,
        }
    }

    /// A pixel's size, mm.
    pub fn pitch(&self) -> (f64, f64) {
        (self.display_x / f64::from(self.width.max(1)), self.display_y / f64::from(self.height.max(1)))
    }

    /// Where the image point (`px`, `py`), in pixels from its top-left corner, is on the plate, mm:
    /// the plate's centre is the image's, row 0 its +Y edge, then the profile's mirrors.
    pub fn to_plate(&self, px: f64, py: f64) -> (f64, f64) {
        let (kx, ky) = self.pitch();
        let mut x = px * kx - self.display_x / 2.0;
        let mut y = self.display_y / 2.0 - py * ky;
        if self.mirror_x {
            x = -x;
        }
        if self.mirror_y {
            y = -y;
        }
        (x, y)
    }
}

pub struct LayerImage {
    pub key: ImageKey,
    pub plate: PlateMap,
    /// `width · height` grey levels, row-major, row 0 the plate's +Y edge (before mirrors).
    pub pixels: Vec<u8>,
    pub tiles: Vec<Tile>,
    /// Separate lit regions.
    pub regions: u32,
}

impl LayerImage {
    /// Decode `layer` of a job sliced for `profile`, in tiles no wider or taller than `limit`.
    pub fn decode(key: ImageKey, layer: &Layer, profile: &PrinterProfile, limit: u32) -> LayerImage {
        let plate = PlateMap::new(profile);
        let mut pixels = Vec::new();
        layer.decode_into(plate.width, plate.height, &mut pixels);
        let tiles = tiles(plate.width, plate.height, limit).into_iter().map(|rect| Tile { rect, mips: mips(&pixels, plate.width, rect) }).collect();
        LayerImage { key, plate, pixels, tiles, regions: layer.regions(plate.width) }
    }

    /// The grey level at pixel (`x`, `y`).
    pub fn grey(&self, x: u32, y: u32) -> Option<u8> {
        if x >= self.plate.width || y >= self.plate.height {
            return None;
        }
        self.pixels.get(y as usize * self.plate.width as usize + x as usize).copied()
    }
}

/// The tiles covering a `width × height` image, none past `limit` on a side: columns first, then
/// rows if the image is taller than the limit too. The tile size is a power of two, so tiles line
/// up with their mipmaps' halvings.
pub fn tiles(width: u32, height: u32, limit: u32) -> Vec<TileRect> {
    let side = 1u32 << (31 - limit.max(1).leading_zeros());
    let mut out = Vec::new();
    let mut y = 0;
    while y < height {
        let h = side.min(height - y);
        let mut x = 0;
        while x < width {
            let w = side.min(width - x);
            out.push(TileRect { x, y, width: w, height: h });
            x += w;
        }
        y += h;
    }
    out
}

/// Box-filtered mipmaps of `rect` of the `stride`-wide image, after its first level, each level
/// half the one before (rounded down, as the GPU sizes them) down to 1 × 1.
fn mips(pixels: &[u8], stride: u32, rect: TileRect) -> Vec<(u32, u32, Vec<u8>)> {
    let mut out: Vec<(u32, u32, Vec<u8>)> = Vec::new();
    let (mut w, mut h) = (rect.width, rect.height);
    while w > 1 || h > 1 {
        let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
        let mut next = Vec::with_capacity(nw as usize * nh as usize);
        // The level before, as (data, its row length, where this rect starts in it).
        let (src, src_stride, x0, y0) = match out.last() {
            Some((_, _, d)) => (d.as_slice(), w as usize, 0, 0),
            None => (pixels, stride as usize, rect.x as usize, rect.y as usize),
        };
        let at = |x: usize, y: usize| u32::from(src.get((y0 + y) * src_stride + x0 + x).copied().unwrap_or(0));
        for j in 0..nh as usize {
            let (y_a, y_b) = (2 * j, (2 * j + 1).min(h as usize - 1));
            for i in 0..nw as usize {
                let (x_a, x_b) = (2 * i, (2 * i + 1).min(w as usize - 1));
                let sum = at(x_a, y_a) + at(x_b, y_a) + at(x_a, y_b) + at(x_b, y_b);
                next.push(((sum + 2) / 4) as u8);
            }
        }
        out.push((nw, nh, next));
        (w, h) = (nw, nh);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use resin_engine::slice::Run;

    #[test]
    fn a_wide_layer_splits_into_columns_that_join() {
        let t = tiles(12000, 5000, 8192);
        assert_eq!(
            t,
            [TileRect { x: 0, y: 0, width: 8192, height: 5000 }, TileRect { x: 8192, y: 0, width: 3808, height: 5000 }],
            "two column tiles, the second starting where the first ends"
        );
        assert_eq!(tiles(7680, 4320, 8192).len(), 1);
        assert_eq!(tiles(100, 100, 6000).iter().map(|r| r.width).max(), Some(100));
        let odd = tiles(10000, 10000, 6000);
        assert_eq!(odd.len(), 9, "a limit that is no power of two rounds down to one: 4096");
        assert_eq!(odd.iter().map(|r| u64::from(r.width) * u64::from(r.height)).sum::<u64>(), 100_000_000);
    }

    #[test]
    fn tiles_and_mipmaps_cover_the_image() {
        // 6 × 3: lit on the left half.
        let runs = (0..3).flat_map(|_| [Run { len: 3, grey: 255 }, Run { len: 3, grey: 0 }]).collect();
        let layer = Layer { runs, ..Default::default() };
        let profile = PrinterProfile { resolution_x: 6, resolution_y: 3, ..Default::default() };
        let img = LayerImage::decode((Stamp::default(), 0), &layer, &profile, 4);
        assert_eq!(img.pixels.len(), 18);
        assert_eq!(img.tiles.iter().map(|t| t.rect.width).collect::<Vec<_>>(), [4, 2]);
        // The second tile starts at pixel 4: dark.
        let right = &img.tiles[1];
        assert_eq!(img.grey(right.rect.x, 0), Some(0));
        assert_eq!(img.grey(2, 2), Some(255));
        assert_eq!(img.grey(6, 0), None);
        let left = &img.tiles[0];
        assert_eq!(left.mips.iter().map(|(w, h, _)| (*w, *h)).collect::<Vec<_>>(), [(2, 1), (1, 1)]);
        assert_eq!(left.mips[0].2, [255, 128], "columns 2 and 3: one lit, one dark");
        assert_eq!(img.regions, 1);
    }

    #[test]
    fn pixels_map_back_to_the_plate() {
        let mut p = PrinterProfile { resolution_x: 100, resolution_y: 50, display_x: 200.0, display_y: 100.0, ..Default::default() };
        let m = PlateMap::new(&p);
        assert_eq!(m.to_plate(50.0, 25.0), (0.0, 0.0), "centre to centre");
        assert_eq!(m.to_plate(0.0, 0.0), (-100.0, 50.0), "row 0 is +Y");
        p.mirror_x = true;
        p.mirror_y = true;
        assert_eq!(PlateMap::new(&p).to_plate(0.0, 0.0), (100.0, -50.0));
    }
}
