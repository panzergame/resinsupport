//! L0: vectors, bounding boxes, footprints arranged on the plate, triangle meshes, their measures and distances, and the solids
//! supports, rafts and labels are drawn with, and text as outlines. Depends on nothing of ours.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod aabb;
mod arrange;
mod bvh;
mod distance;
mod field2d;
mod label;
mod mesh;
mod raft;
mod ray;
mod solid;
mod sphere;
mod text;
mod transform;
mod vec;

pub use aabb::Aabb3;
pub use arrange::{ARRANGE_MARGIN, Item, Rect, Spot, arrange};
pub use bvh::{Bvh, MeshIndex};
pub use distance::{closest_point_triangle, closest_points_segments, segment_triangle_distance};
pub use field2d::Field2;
pub use label::{
    DEFAULT_LABEL_DEPTH, DEFAULT_LABEL_SIZE, LABEL_DEPTH_RANGE, LABEL_SIZE_RANGE, Label, LabelLayout, LabelSide, MAX_LABEL_CHARS, MAX_LABEL_LINES,
};
pub use mesh::{Mesh, RayHit};
pub use raft::{RaftProfile, raft_solid};
pub use solid::{ShadedVertex, frustum, smooth_shaded};
pub use sphere::{ray_capsule, ray_sphere, unit_sphere};
pub use text::{MAX_TEXT_CHARS, TextOutline, missing_chars, text_outline};
pub use transform::{Affine3, Placement};
pub use vec::Vec3;

/// Lengths below this are zero (model units are millimetres).
pub const EPS: f64 = 1e-12;
