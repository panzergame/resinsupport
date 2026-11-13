//! The slicer: closed solids in, a [`SlicedJob`] out. Each layer is a plane cut filled with the
//! nonzero rule, so overlapping solids print as their union and nothing needs merging first.
//! No threads here: the caller says how layers are run ([`slice_with`]).

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crate::raster::{self, Segment, Segments1D};
use crate::{JobStats, Layer, PrintSettings, PrinterProfile, SlicedJob, Stamp, validate};

/// What a solid is, for the statistics.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SolidKind {
    Model,
    Support,
}

/// A closed triangle soup in scene millimetres, wound counter-clockwise seen from outside.
#[derive(Clone, Debug, PartialEq)]
pub struct Solid {
    pub kind: SolidKind,
    pub triangles: Vec<[[f32; 3]; 3]>,
}

/// Everything a slice needs: plain data, so it can go to another thread.
#[derive(Clone, Debug, PartialEq)]
pub struct SliceInput {
    pub solids: Vec<Solid>,
    pub profile: PrinterProfile,
    pub settings: PrintSettings,
    pub stamp: Stamp,
}

/// Layers done out of layers to do, read from any thread while a slice runs.
#[derive(Debug, Default)]
pub struct Progress {
    done: AtomicUsize,
    total: AtomicUsize,
}

impl Progress {
    /// (done, total).
    pub fn get(&self) -> (usize, usize) {
        (self.done.load(Ordering::Relaxed), self.total.load(Ordering::Relaxed))
    }

    /// Start over with `total` steps to do.
    pub fn begin(&self, total: usize) {
        self.total.store(total, Ordering::Relaxed);
        self.done.store(0, Ordering::Relaxed);
    }

    /// One more step done.
    pub fn advance(&self) {
        self.done.fetch_add(1, Ordering::Relaxed);
    }
}

/// Set from any thread to stop a slice early.
#[derive(Debug, Default)]
pub struct Cancel(AtomicBool);

impl Cancel {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SliceError {
    Cancelled,
    /// The settings do not pass [`validate`].
    BadInput(String),
    /// The layer runner skipped a layer.
    Incomplete,
}

impl std::fmt::Display for SliceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SliceError::Cancelled => write!(f, "the slice was cancelled"),
            SliceError::BadInput(why) => write!(f, "cannot slice: {why}"),
            SliceError::Incomplete => write!(f, "the slice left layers out"),
        }
    }
}

impl std::error::Error for SliceError {}

/// Slice `input`, one layer after another on this thread.
pub fn slice(input: &SliceInput, progress: &Progress, cancel: &Cancel) -> Result<SlicedJob, SliceError> {
    slice_with(input, progress, cancel, &|count, layer| (0..count).for_each(layer))
}

/// Slice `input`, `for_each_layer(count, layer)` calling `layer(i)` once for each `i` below
/// `count`, on whatever threads it likes and in any order.
pub fn slice_with(
    input: &SliceInput,
    progress: &Progress,
    cancel: &Cancel,
    for_each_layer: &dyn Fn(usize, &(dyn Fn(usize) + Sync)),
) -> Result<SlicedJob, SliceError> {
    let (profile, settings) = (&input.profile, &input.settings);
    validate(profile, settings).map_err(SliceError::BadInput)?;
    let aa = u32::from(settings.slicing.anti_aliasing.max(1));
    let res = (profile.resolution_x, profile.resolution_y);
    let place = Placement::new(profile, aa);
    let h = settings.layers.height;

    let mut clipped = 0;
    let mut top = 0.0f64;
    let mut solids = Vec::with_capacity(input.solids.len());
    for s in &input.solids {
        let (placed, outside, high) = place.solid(&s.triangles, profile.build_height);
        clipped += u32::from(outside);
        top = top.max(high);
        if !placed.0.is_empty() {
            solids.push(Segments1D::new(placed.0, placed.1));
        }
    }
    // Layer `i` samples `(i + 0.5) · h`: a layer exists while its plane is under the top.
    let fit = (profile.build_height / h + 1e-6).floor();
    let count = (top.min(profile.build_height) / h - 0.5 - 1e-9).ceil().clamp(0.0, fit) as usize;

    progress.begin(count);
    let slots: Vec<OnceLock<(Vec<crate::Run>, u64)>> = (0..count).map(|_| OnceLock::new()).collect();
    let layer = |i: usize| {
        if cancel.is_cancelled() {
            return;
        }
        let z = ((i as f64 + 0.5) * h) as f32;
        let mut segments: Vec<Segment> = Vec::new();
        for s in &solids {
            s.intersect_plane(z, &mut segments);
        }
        let out = raster::layer(aa, res, &segments, cancel);
        if let Some(slot) = slots.get(i) {
            let _ = slot.set(out);
        }
        progress.advance();
    };
    for_each_layer(count, &layer);
    if cancel.is_cancelled() {
        return Err(SliceError::Cancelled);
    }

    let (px, py) = profile.pixel_pitch();
    let mut stats = JobStats { clipped, ..JobStats::default() };
    let mut layers = Vec::with_capacity(count);
    for (i, slot) in slots.into_iter().enumerate() {
        let (runs, open) = slot.into_inner().ok_or(SliceError::Incomplete)?;
        let grey: u64 = runs.iter().map(|r| r.len * u64::from(r.grey)).sum();
        let lit = grey as f64 / 255.0;
        let motion = settings.layer_motion(i as u32);
        stats.volume_ml += lit * px * py * h / 1000.0;
        stats.print_time_s += motion.seconds();
        stats.open_rows += open;
        layers.push(Layer {
            runs,
            z: ((i + 1) as f64 * h) as f32,
            exposure: motion.exposure as f32,
            light_off: motion.light_off as f32,
            lit: lit.round() as u64,
        });
    }
    Ok(SlicedJob { layers, profile: profile.clone(), settings: *settings, stats, stamp: input.stamp })
}

/// From scene millimetres to (anti-aliased) pixels: the plate's centre is the image's, row 0 its
/// +Y edge, then the profile's mirrors.
struct Placement {
    scale: (f64, f64),
    offset: (f64, f64),
    size: (f64, f64),
    /// −1 when the map turns the plane over, which turns every triangle's winding with it.
    handedness: f64,
}

impl Placement {
    fn new(p: &PrinterProfile, aa: u32) -> Placement {
        let size = (f64::from(p.resolution_x) * f64::from(aa), f64::from(p.resolution_y) * f64::from(aa));
        let ([sx, sy], [ox, oy]) = p.image_map(aa);
        Placement { scale: (sx, sy), offset: (ox, oy), size, handedness: (sx * sy).signum() }
    }

    /// The solid's finite triangles placed, each with its depth change; whether any part of it
    /// lies off the plate, under it or above `build_height`; and its top, mm.
    fn solid(&self, triangles: &[[[f32; 3]; 3]], build_height: f64) -> ((Vec<[[f32; 3]; 3]>, Vec<i8>), bool, f64) {
        const SLACK: f64 = 1e-3;
        let mut placed = Vec::with_capacity(triangles.len());
        let mut delta = Vec::with_capacity(triangles.len());
        let mut outside = false;
        let mut top = 0.0f64;
        for t in triangles {
            if !t.iter().flatten().all(|c| c.is_finite()) {
                continue;
            }
            let p = t.map(|[x, y, z]| {
                let (x, y, z) = (f64::from(x), f64::from(y), f64::from(z));
                [x * self.scale.0 + self.offset.0, y * self.scale.1 + self.offset.1, z]
            });
            for [x, y, z] in p {
                outside |= x < -SLACK || y < -SLACK || x > self.size.0 + SLACK || y > self.size.1 + SLACK;
                outside |= z < -SLACK || z > build_height + SLACK;
                top = top.max(z);
            }
            let [a, b, c] = p;
            let (u, v) = ([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [c[0] - a[0], c[1] - a[1], c[2] - a[2]]);
            let normal_x = (u[1] * v[2] - u[2] * v[1]) * self.handedness;
            // Along +X a scanline enters the solid through a face looking towards −X.
            delta.push(if normal_x > 0.0 { -1 } else { 1 });
            placed.push(p.map(|v| v.map(|c| c as f32)));
        }
        ((placed, delta), outside, top)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A box from `min` to `max`, as twelve triangles wound outwards.
    fn cuboid(min: [f32; 3], max: [f32; 3]) -> Vec<[[f32; 3]; 3]> {
        let c =
            |i: usize| [if i & 1 == 0 { min[0] } else { max[0] }, if i & 2 == 0 { min[1] } else { max[1] }, if i & 4 == 0 { min[2] } else { max[2] }];
        let quads = [[0, 2, 3, 1], [4, 5, 7, 6], [0, 1, 5, 4], [2, 6, 7, 3], [0, 4, 6, 2], [1, 3, 7, 5]];
        quads.iter().flat_map(|&[a, b, cc, d]| [[c(a), c(b), c(cc)], [c(a), c(cc), c(d)]]).collect()
    }

    fn input(solids: Vec<Vec<[[f32; 3]; 3]>>) -> SliceInput {
        SliceInput {
            solids: solids.into_iter().map(|triangles| Solid { kind: SolidKind::Model, triangles }).collect(),
            profile: PrinterProfile::default(),
            settings: PrintSettings::default(),
            stamp: Stamp::default(),
        }
    }

    fn run(i: &SliceInput) -> SlicedJob {
        slice(i, &Progress::default(), &Cancel::default()).unwrap()
    }

    fn pixels(p: &PrinterProfile) -> u64 {
        u64::from(p.resolution_x) * u64::from(p.resolution_y)
    }

    /// The lit pixels' mean column and row.
    fn centroid(layer: &Layer, width: u64) -> (f64, f64) {
        let (mut at, mut sx, mut sy, mut n) = (0u64, 0.0, 0.0, 0.0);
        for r in &layer.runs {
            if r.grey > 0 {
                for p in at..at + r.len {
                    sx += (p % width) as f64;
                    sy += (p / width) as f64;
                    n += 1.0;
                }
            }
            at += r.len;
        }
        (sx / n, sy / n)
    }

    #[test]
    fn a_cube_on_the_plate_fills_its_square_on_every_layer() {
        let job = run(&input(vec![cuboid([-5.0, -5.0, 0.0], [5.0, 5.0, 10.0])]));
        assert_eq!(job.layers.len(), 200);
        assert!((job.height() - 10.0).abs() < 1e-4);
        let side = 10.0 / 0.0285;
        let total = pixels(&job.profile);
        for (i, l) in job.layers.iter().enumerate() {
            assert_eq!(l.runs.iter().map(|r| r.len).sum::<u64>(), total, "layer {i}");
            let (lo, hi) = ((side - 1.0) * (side - 1.0), (side + 1.0) * (side + 1.0));
            assert!((lo..=hi).contains(&(l.lit as f64)), "layer {i}: {} lit, {side}² expected", l.lit);
        }
        assert_eq!(job.stats.open_rows, 0);
        assert_eq!(job.stats.clipped, 0);
        let mut bitmap = Vec::new();
        let middle = &job.layers[100];
        middle.decode_into(job.profile.resolution_x, job.profile.resolution_y, &mut bitmap);
        assert_eq!(bitmap.len() as u64, total);
        let lit = bitmap.iter().map(|&g| f64::from(g) / 255.0).sum::<f64>();
        assert_eq!(lit.round() as u64, middle.lit);
        assert_eq!(middle.regions(job.profile.resolution_x), 1);
        // 1 ml, as rasterized.
        assert!((job.stats.volume_ml - 1.0).abs() < 0.02, "{}", job.stats.volume_ml);
        assert!(job.stats.print_time_s > 0.0);
        assert_eq!(job.layers[0].exposure, 30.0);
        assert_eq!(job.layers[199].exposure, 3.0);
    }

    #[test]
    fn overlapping_cubes_print_their_union() {
        let a = cuboid([-5.0, -5.0, 0.0], [5.0, 5.0, 1.0]);
        let b = cuboid([0.0, -5.0, 0.0], [10.0, 5.0, 1.0]);
        let one = run(&input(vec![cuboid([-5.0, -5.0, 0.0], [10.0, 5.0, 1.0])]));
        let both = run(&input(vec![a, b]));
        assert_eq!(both.layers.len(), one.layers.len());
        for (x, y) in both.layers.iter().zip(&one.layers) {
            assert_eq!(x.lit, y.lit);
        }
        assert_eq!(both.stats.open_rows, 0);
    }

    #[test]
    fn plus_x_is_right_plus_y_is_up_and_mirrors_swap() {
        let w = u64::from(PrinterProfile::default().resolution_x);
        let (cx, cy) = (3840.0, 2160.0);
        let right = run(&input(vec![cuboid([20.0, -2.0, 0.0], [24.0, 2.0, 0.5])]));
        let (x, y) = centroid(&right.layers[0], w);
        assert!(x > cx + 500.0 && (y - cy).abs() < 2.0, "{x} {y}");
        let up = run(&input(vec![cuboid([-2.0, 20.0, 0.0], [2.0, 24.0, 0.5])]));
        let (x, y) = centroid(&up.layers[0], w);
        assert!(y < cy - 500.0 && (x - cx).abs() < 2.0, "rows above the centre: {x} {y}");
        let mut mirrored = input(vec![cuboid([20.0, -2.0, 0.0], [24.0, 2.0, 0.5])]);
        mirrored.profile.mirror_x = true;
        let job = run(&mirrored);
        let (x, _) = centroid(&job.layers[0], w);
        assert!(x < cx - 500.0, "{x}");
        assert_eq!(job.stats.open_rows, 0, "the mirror keeps the inside inside");
        assert_eq!(job.layers[0].lit, right.layers[0].lit);
    }

    #[test]
    fn open_and_inside_out_meshes_slice_and_are_counted() {
        let mut open = cuboid([-5.0, -5.0, 0.0], [5.0, 5.0, 1.0]);
        open.retain(|t| !(t.iter().all(|v| v[0] == 5.0)));
        let job = run(&input(vec![open]));
        assert!(job.stats.open_rows > 0);
        // Half the sides turned inside out.
        let mut flipped = cuboid([-5.0, -5.0, 0.0], [5.0, 5.0, 1.0]);
        for t in flipped.iter_mut().filter(|t| t.iter().all(|v| v[0] == -5.0)) {
            t.swap(1, 2);
        }
        let job = run(&input(vec![flipped]));
        assert!(job.stats.open_rows > 0);
        for l in &job.layers {
            assert_eq!(l.runs.iter().map(|r| r.len).sum::<u64>(), pixels(&job.profile));
        }
    }

    #[test]
    fn anti_aliasing_greys_a_slanted_edge_and_keeps_the_area() {
        // A prism whose cut is a right triangle: its long side runs at 45°.
        let (a, b, c) = ([-5.0f32, -5.0], [5.0f32, -5.0], [-5.0f32, 5.0]);
        let tri = |z: f32, p: [f32; 2]| [p[0], p[1], z];
        let mut prism = vec![[tri(0.0, a), tri(0.0, c), tri(0.0, b)], [tri(1.0, a), tri(1.0, b), tri(1.0, c)]];
        for (p, q) in [(a, b), (b, c), (c, a)] {
            prism.push([tri(0.0, p), tri(0.0, q), tri(1.0, q)]);
            prism.push([tri(0.0, p), tri(1.0, q), tri(1.0, p)]);
        }
        let binary = run(&input(vec![prism.clone()]));
        let mut smooth = input(vec![prism]);
        smooth.settings.slicing.anti_aliasing = 4;
        let smooth = run(&smooth);
        let l = &smooth.layers[0];
        assert!(l.runs.iter().any(|r| r.grey > 0 && r.grey < 255), "grey along the slant");
        assert_eq!(l.runs.iter().map(|r| r.len).sum::<u64>(), pixels(&smooth.profile));
        let (x, y) = (binary.layers[0].lit as f64, l.lit as f64);
        assert!((x - y).abs() / x < 0.01, "{x} {y}");
        assert_eq!(smooth.stats.open_rows, 0);
    }

    #[test]
    fn a_cancelled_slice_stops() {
        let i = input(vec![cuboid([-50.0, -50.0, 0.0], [50.0, 50.0, 200.0])]);
        let cancel = Cancel::default();
        cancel.cancel();
        let start = std::time::Instant::now();
        assert_eq!(slice(&i, &Progress::default(), &cancel), Err(SliceError::Cancelled));
        assert!(start.elapsed().as_secs_f64() < 1.0);
        // Cancelled midway, from the runner.
        let cancel = Cancel::default();
        let r = slice_with(&i, &Progress::default(), &cancel, &|n, layer| {
            for k in 0..n {
                if k == 3 {
                    cancel.cancel();
                }
                layer(k);
            }
        });
        assert_eq!(r, Err(SliceError::Cancelled));
        // A runner that skips layers is caught.
        let r = slice_with(&i, &Progress::default(), &Cancel::default(), &|_, _| {});
        assert_eq!(r, Err(SliceError::Incomplete));
    }

    #[test]
    fn hostile_inputs_never_panic() {
        let empty = run(&input(Vec::new()));
        assert!(empty.layers.is_empty());
        let mut nan = cuboid([-5.0, -5.0, 0.0], [5.0, 5.0, 1.0]);
        nan[0][0][0] = f32::NAN;
        nan[1][1][2] = f32::INFINITY;
        run(&input(vec![nan]));
        // Far off the plate and past the build height: clipped, counted, sliced up to it.
        let job = run(&input(vec![cuboid([-500.0, -5.0, 0.0], [5.0, 5.0, 300.0])]));
        assert_eq!(job.stats.clipped, 1);
        assert_eq!(job.layers.len(), 5000);
        let mut huge = input(vec![cuboid([-5.0, -5.0, 0.0], [5.0, 5.0, 1.0])]);
        huge.profile.resolution_x = u32::MAX;
        assert!(matches!(slice(&huge, &Progress::default(), &Cancel::default()), Err(SliceError::BadInput(_))));
        let mut flat = input(vec![cuboid([-5.0, -5.0, 0.0], [5.0, 5.0, 1.0])]);
        flat.settings.layers.height = 0.0;
        assert!(matches!(slice(&flat, &Progress::default(), &Cancel::default()), Err(SliceError::BadInput(_))));
        flat.settings.layers.height = f64::NAN;
        assert!(matches!(slice(&flat, &Progress::default(), &Cancel::default()), Err(SliceError::BadInput(_))));
    }

    #[test]
    fn progress_counts_layers() {
        let p = Progress::default();
        slice(&input(vec![cuboid([-1.0, -1.0, 0.0], [1.0, 1.0, 1.0])]), &p, &Cancel::default()).unwrap();
        assert_eq!(p.get(), (20, 20));
    }
}
