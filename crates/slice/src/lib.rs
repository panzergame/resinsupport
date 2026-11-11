//! L2: what a resin print needs besides the scene. The printer profile (what the machine is),
//! the print settings (how this resin is exposed and how the plate moves), and the sliced job the
//! slicer fills and the printer formats write, and the slicer. Wasm-friendly: no files, no
//! threads.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod job;
mod raster;
mod settings;
mod slicer;

pub use job::{JobStats, Layer, Run, SlicedJob, Stamp};
pub use settings::{
    ANTI_ALIASING, Estimates, Exposure, FIELDS, Field, GROUPS, LayerMotion, Layers, MAX_NAME_CHARS, MIN_PIXEL_PITCH, Motion, PrintSettings,
    PrinterProfile, Slicing, field, validate,
};
pub use slicer::{Cancel, Progress, SliceError, SliceInput, Solid, SolidKind, slice, slice_with};
