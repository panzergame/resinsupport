//! L0: vectors, bounding boxes, triangle meshes and their measures. Depends on nothing of ours.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod aabb;
mod bvh;
mod mesh;
mod ray;
mod sphere;
mod transform;
mod vec;

pub use aabb::Aabb3;
pub use bvh::{Bvh, MeshIndex};
pub use mesh::{Mesh, RayHit};
pub use sphere::{ray_sphere, unit_sphere};
pub use transform::{Affine3, Placement};
pub use vec::Vec3;

/// Lengths below this are zero (model units are millimetres).
pub const EPS: f64 = 1e-12;
