//! L0: vectors, bounding boxes, footprints arranged on the plate, triangle meshes, their measures and distances, and the solids
//! supports are drawn with. Depends on nothing of ours.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod aabb;
mod arrange;
mod bvh;
mod distance;
mod mesh;
mod ray;
mod solid;
mod sphere;
mod transform;
mod vec;

pub use aabb::Aabb3;
pub use arrange::{ARRANGE_MARGIN, Item, Rect, Spot, arrange};
pub use bvh::{Bvh, MeshIndex};
pub use distance::{closest_point_triangle, closest_points_segments, segment_triangle_distance};
pub use mesh::{Mesh, RayHit};
pub use solid::{ShadedVertex, frustum};
pub use sphere::{ray_capsule, ray_sphere, unit_sphere};
pub use transform::{Affine3, Placement};
pub use vec::Vec3;

/// Lengths below this are zero (model units are millimetres).
pub const EPS: f64 = 1e-12;
