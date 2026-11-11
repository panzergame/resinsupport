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
