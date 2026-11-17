//! The raft: one plate under every base that reaches the plate, drawn and printed alike. Like the
//! routes it stands on, it is derived, never stored.

use resin_geom::{Field2, RaftProfile, Vec3, raft_solid};

use crate::{Route, SupportPreset};

/// The outline's grid, mm. Contours are interpolated (a chord 0.002 mm off a 4 mm disc) and a
/// sloped wall is exact between them, so it need not match a printer's pixel; a sloped wall's
/// triangles grow with the inverse square of it: 0.1 mm made 0.7 M of them under 200 feet.
const CELL: f64 = 0.25;

/// The feet the raft joins: each route's that ends on the plate on a base.
pub fn raft_feet<'a>(routes: impl IntoIterator<Item = &'a Route>) -> Vec<Vec3> {
    routes.into_iter().filter(|r| r.foot).filter_map(|r| r.body.last().copied()).filter(|f| f.is_finite()).collect()
}

/// The raft under `feet` sized by `p`, closed and wound outwards; nothing when `p` has none.
pub fn raft_triangles(feet: &[Vec3], p: &SupportPreset) -> Vec<[[f32; 3]; 3]> {
    let raft = &p.raft;
    if !raft.enabled || feet.is_empty() {
        return Vec::new();
    }
    let profile = RaftProfile { thickness: raft.thickness, slope: raft.slope(), rim_height: raft.rim_height, rim_width: raft.rim_width };
    let centers: Vec<[f64; 2]> = feet.iter().map(|f| [f.x, f.y]).collect();
    let Some(field) =
        Field2::closed_discs(&centers, p.base.diameter / 2.0 + raft.margin, raft.merge_distance, profile.reach(), profile.depth(), CELL)
    else {
        return Vec::new();
    };
    raft_solid(&field, &profile)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PresetId, RaftEdge};
    use resin_geom::Mesh;

    fn foot(x: f64, y: f64) -> Vec3 {
        Vec3::new(x, y, 0.0)
    }

    #[test]
    fn the_raft_joins_near_feet_under_their_bases_and_stays_off_without_one() {
        let mut p = PresetId::Medium.defaults();
        p.raft.edge = RaftEdge::Straight;
        // Medium: base 4.2 mm, margin 2, merge 5: the outline is 4.1 mm round each foot.
        let near = raft_triangles(&[foot(0.0, 0.0), foot(10.0, 0.0)], &p);
        let mesh = Mesh::new(near.clone());
        let b = mesh.bounds();
        assert!((b.min.x + 4.1).abs() < 0.05 && (b.max.x - 14.1).abs() < 0.05, "{b:?}");
        assert!(b.min.z.abs() < 1e-6 && (b.max.z - p.raft.thickness).abs() < 1e-6);
        // Two discs and the bridge between them, under a 1.8 mm gap.
        let discs = 2.0 * std::f64::consts::PI * 4.1 * 4.1 * p.raft.thickness;
        assert!(mesh.signed_volume() > discs, "{}", mesh.signed_volume());

        let far = Mesh::new(raft_triangles(&[foot(0.0, 0.0), foot(20.0, 0.0)], &p)).signed_volume();
        assert!((far - discs).abs() < discs * 0.01, "{far} {discs}");

        p.raft.enabled = false;
        assert!(raft_triangles(&[foot(0.0, 0.0)], &p).is_empty());
        assert_eq!(p.foot_z(), 0.0);
    }

    #[test]
    fn hostile_feet_give_no_raft_or_a_bounded_one() {
        let p = PresetId::Medium.defaults();
        assert!(raft_triangles(&[], &p).is_empty());
        assert!(raft_triangles(&[Vec3::new(f64::NAN, 0.0, 0.0)], &p).is_empty());
        assert!(raft_triangles(&[foot(0.0, 0.0), foot(1e12, 0.0)], &p).is_empty());
    }
}
