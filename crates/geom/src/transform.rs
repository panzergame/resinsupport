use crate::Vec3;

/// An affine map `p ↦ m·p + t`. `m` is row-major.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Affine3 {
    pub m: [[f64; 3]; 3],
    pub t: Vec3,
}

impl Default for Affine3 {
    fn default() -> Self {
        Affine3::IDENTITY
    }
}

impl Affine3 {
    pub const IDENTITY: Affine3 = Affine3 { m: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], t: Vec3::ZERO };

    pub fn translation(t: Vec3) -> Affine3 {
        Affine3 { t, ..Affine3::IDENTITY }
    }
    pub fn scale(s: Vec3) -> Affine3 {
        Affine3 { m: [[s.x, 0.0, 0.0], [0.0, s.y, 0.0], [0.0, 0.0, s.z]], t: Vec3::ZERO }
    }
    /// Turn `degrees.x` about X, then `degrees.y` about Y, then `degrees.z` about Z (world axes,
    /// right-handed).
    pub fn rotation_xyz(degrees: Vec3) -> Affine3 {
        let (sx, cx) = degrees.x.to_radians().sin_cos();
        let (sy, cy) = degrees.y.to_radians().sin_cos();
        let (sz, cz) = degrees.z.to_radians().sin_cos();
        let rx = Affine3 { m: [[1.0, 0.0, 0.0], [0.0, cx, -sx], [0.0, sx, cx]], t: Vec3::ZERO };
        let ry = Affine3 { m: [[cy, 0.0, sy], [0.0, 1.0, 0.0], [-sy, 0.0, cy]], t: Vec3::ZERO };
        let rz = Affine3 { m: [[cz, -sz, 0.0], [sz, cz, 0.0], [0.0, 0.0, 1.0]], t: Vec3::ZERO };
        rx.then(&ry).then(&rz)
    }
    /// Turn `angle` radians about the unit `axis` through the origin (right-handed).
    pub fn rotation_about(axis: Vec3, angle: f64) -> Affine3 {
        let (c, s) = (angle.cos(), angle.sin());
        let t = 1.0 - c;
        let (x, y, z) = (axis.x, axis.y, axis.z);
        Affine3 {
            m: [
                [t * x * x + c, t * x * y - s * z, t * x * z + s * y],
                [t * x * y + s * z, t * y * y + c, t * y * z - s * x],
                [t * x * z - s * y, t * y * z + s * x, t * z * z + c],
            ],
            t: Vec3::ZERO,
        }
    }
    /// The angles [`Affine3::rotation_xyz`] takes to give this map's linear part, which must be
    /// a rotation. Each in (-180, 180]; at a quarter turn about Y the X and Z turns are one, all
    /// given to X.
    pub fn euler_xyz_degrees(&self) -> Vec3 {
        let m = &self.m;
        let y = (-m[2][0]).clamp(-1.0, 1.0).asin();
        let (x, z) = if m[2][0].abs() < 1.0 - 1e-9 { (m[2][1].atan2(m[2][2]), m[1][0].atan2(m[0][0])) } else { ((-m[1][2]).atan2(m[1][1]), 0.0) };
        Vec3::new(x.to_degrees(), y.to_degrees(), z.to_degrees())
    }
    /// The map undoing this one; None when it is degenerate.
    pub fn inverse(&self) -> Option<Affine3> {
        let det = self.determinant();
        if !(det.abs() > 1e-300) || !det.is_finite() {
            return None;
        }
        let m = &self.m;
        let c = |r0: usize, r1: usize, c0: usize, c1: usize| m[r0][c0] * m[r1][c1] - m[r0][c1] * m[r1][c0];
        let inv = [
            [c(1, 2, 1, 2) / det, -c(0, 2, 1, 2) / det, c(0, 1, 1, 2) / det],
            [-c(1, 2, 0, 2) / det, c(0, 2, 0, 2) / det, -c(0, 1, 0, 2) / det],
            [c(1, 2, 0, 1) / det, -c(0, 2, 0, 1) / det, c(0, 1, 0, 1) / det],
        ];
        let lin = Affine3 { m: inv, t: Vec3::ZERO };
        Some(Affine3 { m: inv, t: -lin.vector(self.t) })
    }
    /// This map's linear part applied about `pivot` instead of the origin.
    pub fn about(&self, pivot: Vec3) -> Affine3 {
        Affine3::translation(-pivot).then(self).then(&Affine3::translation(pivot))
    }
    /// `self`, then `next`.
    pub fn then(&self, next: &Affine3) -> Affine3 {
        let a = &next.m;
        let b = &self.m;
        let mut m = [[0.0; 3]; 3];
        for (i, row) in m.iter_mut().enumerate() {
            for (j, v) in row.iter_mut().enumerate() {
                *v = a[i][0] * b[0][j] + a[i][1] * b[1][j] + a[i][2] * b[2][j];
            }
        }
        Affine3 { m, t: next.vector(self.t) + next.t }
    }
    /// A direction or offset carried along: the linear part only.
    pub fn vector(&self, v: Vec3) -> Vec3 {
        let r = |row: [f64; 3]| row[0] * v.x + row[1] * v.y + row[2] * v.z;
        Vec3::new(r(self.m[0]), r(self.m[1]), r(self.m[2]))
    }
    pub fn point(&self, p: Vec3) -> Vec3 {
        self.vector(p) + self.t
    }
    /// The matrix carrying normals (rows): the cofactor matrix, which is the inverse transpose
    /// up to a positive factor, so it keeps normals perpendicular under non-uniform scale.
    pub fn normal_matrix(&self) -> [[f64; 3]; 3] {
        let m = &self.m;
        let c0 = Vec3::new(m[0][0], m[1][0], m[2][0]);
        let c1 = Vec3::new(m[0][1], m[1][1], m[2][1]);
        let c2 = Vec3::new(m[0][2], m[1][2], m[2][2]);
        let flip = if self.determinant() < 0.0 { -1.0 } else { 1.0 };
        // These are the matrix's columns: the images of the axes' normals.
        let cols = [c1.cross(c2) * flip, c2.cross(c0) * flip, c0.cross(c1) * flip];
        [0, 1, 2].map(|i| {
            let [x, y, z] = cols.map(|c| <[f64; 3]>::from(c)[i]);
            [x, y, z]
        })
    }
    /// A surface normal carried along, unit length; None when degenerate.
    pub fn normal(&self, n: Vec3) -> Option<Vec3> {
        let r = self.normal_matrix().map(Vec3::from);
        Vec3::new(r[0].dot(n), r[1].dot(n), r[2].dot(n)).normalized()
    }
    pub fn determinant(&self) -> f64 {
        let m = &self.m;
        m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
    }
    pub fn is_finite(&self) -> bool {
        self.t.is_finite() && self.m.iter().flatten().all(|v| v.is_finite())
    }
}

/// Where a model sits, measured from how it was imported: its imported bounds' centre (the pivot)
/// moved to `position`, after scaling by `scale` along the model's own axes and turning by
/// `rotation` degrees (about X, then Y, then Z).
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Placement {
    pub position: Vec3,
    pub rotation: Vec3,
    pub scale: Vec3,
}

impl Placement {
    /// As imported: the pivot where it was, unturned, unscaled.
    pub fn at(pivot: Vec3) -> Placement {
        Placement { position: pivot, rotation: Vec3::ZERO, scale: Vec3::new(1.0, 1.0, 1.0) }
    }
    /// The map from imported coordinates to where the model is now.
    pub fn affine(&self, pivot: Vec3) -> Affine3 {
        Affine3::translation(-pivot)
            .then(&Affine3::scale(self.scale))
            .then(&Affine3::rotation_xyz(self.rotation))
            .then(&Affine3::translation(self.position))
    }
    pub fn is_finite(&self) -> bool {
        self.position.is_finite() && self.rotation.is_finite() && self.scale.is_finite()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Vec3, b: Vec3) -> bool {
        a.dist(b) < 1e-9
    }

    #[test]
    fn rotations_turn_the_right_way() {
        let r = Affine3::rotation_xyz(Vec3::new(0.0, 0.0, 90.0));
        assert!(close(r.point(Vec3::X), Vec3::Y));
        let r = Affine3::rotation_xyz(Vec3::new(90.0, 0.0, 0.0));
        assert!(close(r.point(Vec3::Y), Vec3::Z));
        // X first, then Z: Y goes to Z, which Z leaves alone.
        let r = Affine3::rotation_xyz(Vec3::new(90.0, 0.0, 90.0));
        assert!(close(r.point(Vec3::Y), Vec3::Z));
        assert!(close(r.point(Vec3::X), Vec3::Y));
    }

    #[test]
    fn euler_angles_round_trip() {
        for d in [Vec3::new(10.0, 20.0, 30.0), Vec3::new(-170.0, 45.0, 90.0), Vec3::new(0.0, 89.0, -45.0), Vec3::ZERO] {
            let back = Affine3::rotation_xyz(d).euler_xyz_degrees();
            assert!(back.dist(d) < 1e-6, "{d:?} -> {back:?}");
        }
        // At a quarter turn about Y the angle still reproduces the rotation.
        let r = Affine3::rotation_xyz(Vec3::new(30.0, 90.0, 20.0));
        let back = Affine3::rotation_xyz(r.euler_xyz_degrees());
        assert!(close(back.point(Vec3::new(1.0, 2.0, 3.0)), r.point(Vec3::new(1.0, 2.0, 3.0))));
    }

    #[test]
    fn rotation_about_matches_the_axis_turns() {
        let a = Affine3::rotation_about(Vec3::Z, 90f64.to_radians());
        assert!(close(a.point(Vec3::X), Vec3::Y));
        let b = Affine3::rotation_about(Vec3::X, 90f64.to_radians());
        assert!(close(b.point(Vec3::Y), Vec3::Z));
    }

    #[test]
    fn inverse_undoes() {
        let p = Placement { position: Vec3::new(5.0, -3.0, 2.0), rotation: Vec3::new(10.0, 20.0, 30.0), scale: Vec3::new(2.0, 0.5, 3.0) };
        let a = p.affine(Vec3::new(1.0, 1.0, 1.0));
        let q = Vec3::new(7.0, 8.0, -9.0);
        assert!(close(a.inverse().unwrap().point(a.point(q)), q));
        assert!(Affine3::scale(Vec3::new(0.0, 1.0, 1.0)).inverse().is_none());
        // The pivot lands on the position.
        assert!(close(a.point(Vec3::new(1.0, 1.0, 1.0)), p.position));
    }

    #[test]
    fn about_a_pivot_keeps_the_pivot() {
        let p = Vec3::new(5.0, 5.0, 5.0);
        let s = Affine3::scale(Vec3::new(2.0, 3.0, 4.0)).about(p);
        assert!(close(s.point(p), p));
        assert!(close(s.point(Vec3::ZERO), Vec3::new(-5.0, -10.0, -15.0)));
    }

    #[test]
    fn normals_stay_perpendicular_under_stretch() {
        // A 45° face, stretched along X: its normal tilts toward X less, not more.
        let s = Affine3::scale(Vec3::new(4.0, 1.0, 1.0));
        let n = s.normal(Vec3::new(1.0, 0.0, 1.0)).unwrap();
        let tangent = s.point(Vec3::new(1.0, 0.0, -1.0));
        assert!(n.dot(tangent).abs() < 1e-9);
        assert!(n.x < n.z);
        assert!(Affine3::scale(Vec3::new(0.0, 1.0, 1.0)).normal(Vec3::Y).is_none());
        // Turned and stretched, too: the rows hold up off the diagonal.
        let a = Affine3::rotation_xyz(Vec3::new(20.0, -35.0, 50.0)).then(&Affine3::scale(Vec3::new(3.0, 0.5, 2.0)));
        let (n, t) = (Vec3::new(0.3, -0.8, 0.5), Vec3::new(0.8, 0.3, 0.0));
        assert!(a.normal(n).unwrap().dot(a.vector(t)).abs() < 1e-9);
    }
}
