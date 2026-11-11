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
    /// Lit pixels, weighted by grey.
    pub lit: u64,
}

/// What the slicer measured.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct JobStats {
    pub volume_ml: f64,
    pub print_time_s: f64,
}

/// A whole job, with the profile and settings it was sliced for.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SlicedJob {
    pub layers: Vec<Layer>,
    pub profile: PrinterProfile,
    pub settings: PrintSettings,
    pub stats: JobStats,
}
