// Ported from SolidCraft (MIT OR Apache-2.0), crates/geom/src/mesh.rs at 4bdffef0; see ATTRIBUTION.md.
use serde::{Deserialize, Serialize};

use crate::Vec3;

/// Axis-aligned bounding box.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Aabb3 {
    pub min: Vec3,
    pub max: Vec3,
}

impl Default for Aabb3 {
    fn default() -> Self {
        Aabb3::EMPTY
    }
}

impl Aabb3 {
    pub const EMPTY: Aabb3 = Aabb3 {
        min: Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY),
        max: Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY),
    };
    pub fn is_empty(&self) -> bool {
        !(self.min.x <= self.max.x && self.min.y <= self.max.y && self.min.z <= self.max.z)
    }
    pub fn add(&mut self, p: Vec3) {
        if p.is_finite() {
            self.min = self.min.min(p);
            self.max = self.max.max(p);
        }
    }
    pub fn union(&self, o: &Aabb3) -> Aabb3 {
        if self.is_empty() {
            return *o;
        }
        if o.is_empty() {
            return *self;
        }
        Aabb3 { min: self.min.min(o.min), max: self.max.max(o.max) }
    }
    pub fn center(&self) -> Vec3 {
        if self.is_empty() { Vec3::ZERO } else { (self.min + self.max) * 0.5 }
    }
    pub fn size(&self) -> Vec3 {
        if self.is_empty() { Vec3::ZERO } else { self.max - self.min }
    }
    pub fn diagonal(&self) -> f64 {
        self.size().len()
    }
    /// Whether the boxes share a point, sides included (never for an empty box).
    pub fn overlaps(&self, o: &Aabb3) -> bool {
        self.min.x <= o.max.x
            && o.min.x <= self.max.x
            && self.min.y <= o.max.y
            && o.min.y <= self.max.y
            && self.min.z <= o.max.z
            && o.min.z <= self.max.z
    }
    /// Whether `p` lies in the box, its sides included (never in an empty box, never for NaN).
    pub fn contains(&self, p: Vec3) -> bool {
        p.x >= self.min.x && p.y >= self.min.y && p.z >= self.min.z && p.x <= self.max.x && p.y <= self.max.y && p.z <= self.max.z
    }
}

/// The box around the finite points (non-finite ones are skipped, as [`Aabb3::add`] does).
impl FromIterator<Vec3> for Aabb3 {
    fn from_iter<I: IntoIterator<Item = Vec3>>(points: I) -> Aabb3 {
        let mut b = Aabb3::EMPTY;
        for p in points {
            b.add(p);
        }
        b
    }
}
