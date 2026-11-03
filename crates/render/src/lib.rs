//! L1: headless view math. The camera, its projections, and the overlay geometry (grid, axes)
//! the front end draws. No GPU or window code lives here.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod camera;
pub mod overlay;

pub use camera::{Camera, CameraAnim, Mat4, StandardView, UpAxis};
pub use overlay::{Line, Overlay, OverlayStyle, grid_step};
