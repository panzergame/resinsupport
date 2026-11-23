//! The raft: a plate under each model's bases that reach the plate, widened out to its label,
//! drawn and printed alike. Each model has its own, never joined with another's, so hiding a
//! model leaves the others' rafts as they are. Like the routes it stands on, it is derived,
//! never stored.

use resin_geom::{Field2, Label, LabelLayout, RaftProfile, Vec3, raft_solid};

use crate::{Route, SupportPreset};

/// The outline's grid, mm. Contours are interpolated (a chord 0.002 mm off a 4 mm disc) and a
/// sloped wall is exact between them, so it need not match a printer's pixel; a sloped wall's
/// triangles grow with the inverse square of it: 0.1 mm made 0.7 M of them under 200 feet.
const CELL: f64 = 0.25;

/// The feet the raft joins: each route's that ends on the plate on a base.
pub fn raft_feet<'a>(routes: impl IntoIterator<Item = &'a Route>) -> Vec<Vec3> {
    routes.into_iter().filter(|r| r.foot).filter_map(|r| r.body.last().copied()).filter(|f| f.is_finite()).collect()
}

/// The raft's heights and edge in `p`.
fn profile(p: &SupportPreset) -> RaftProfile {
    let raft = &p.raft;
    RaftProfile { thickness: raft.thickness, slope: raft.slope(), rim_height: raft.rim_height, rim_width: raft.rim_width }
}

/// The radius of the raft's disc round each foot in `p`.
fn disc_radius(p: &SupportPreset) -> f64 {
    p.base.diameter / 2.0 + p.raft.margin
}

/// Where each of `labels` (a label and its model's feet, see [`crate::label_sites`]) goes on the
/// raft `p` makes; nothing when `p` has none.
pub(crate) fn label_layouts(labels: &[(Label, Vec<[f64; 2]>)], p: &SupportPreset) -> Vec<LabelLayout> {
    if !p.raft.enabled {
        return Vec::new();
    }
    let profile = profile(p);
    labels.iter().filter_map(|(label, feet)| LabelLayout::new(label, feet, p.base.diameter / 2.0, &profile)).collect()
}

/// The raft under `feet` sized by `p`, widened out to the plates of `labels` (see
/// [`crate::label_sites`]) so its edge and rim go round them too, closed and wound outwards;
/// nothing when `p` has none.
pub fn raft_triangles(feet: &[Vec3], labels: &[(Label, Vec<[f64; 2]>)], p: &SupportPreset) -> Vec<[[f32; 3]; 3]> {
    let raft = &p.raft;
    if !raft.enabled || feet.is_empty() {
        return Vec::new();
    }
    let profile = profile(p);
    let centers: Vec<[f64; 2]> = feet.iter().map(|f| [f.x, f.y]).collect();
    let plates = label_layouts(labels, p);
    let cover: Vec<[f64; 2]> = plates.iter().flat_map(LabelLayout::corners).collect();
    let Some(mut field) =
        Field2::closed_discs_covering(&centers, disc_radius(p), raft.merge_distance, profile.reach(), profile.depth(), CELL, &cover)
    else {
        return Vec::new();
    };
    // Past this a plate is farther than any level the raft draws.
    let past = profile.reach() + 2.0 * field.cell;
    for plate in &plates {
        let c = plate.corners();
        let lo = c.iter().fold([f64::INFINITY; 2], |m, q| [m[0].min(q[0]) - past, m[1].min(q[1]) - past]);
        let hi = c.iter().fold([f64::NEG_INFINITY; 2], |m, q| [m[0].max(q[0]) + past, m[1].max(q[1]) + past]);
        field.union_with(lo, hi, |q| plate.plate_distance(q));
    }
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
        let near = raft_triangles(&[foot(0.0, 0.0), foot(10.0, 0.0)], &[], &p);
        let mesh = Mesh::new(near.clone());
        let b = mesh.bounds();
        assert!((b.min.x + 4.1).abs() < 0.05 && (b.max.x - 14.1).abs() < 0.05, "{b:?}");
        assert!(b.min.z.abs() < 1e-6 && (b.max.z - p.raft.thickness).abs() < 1e-6);
        // Two discs and the bridge between them, under a 1.8 mm gap.
        let discs = 2.0 * std::f64::consts::PI * 4.1 * 4.1 * p.raft.thickness;
        assert!(mesh.signed_volume() > discs, "{}", mesh.signed_volume());

        let far = Mesh::new(raft_triangles(&[foot(0.0, 0.0), foot(20.0, 0.0)], &[], &p)).signed_volume();
        assert!((far - discs).abs() < discs * 0.01, "{far} {discs}");

        p.raft.enabled = false;
        assert!(raft_triangles(&[foot(0.0, 0.0)], &[], &p).is_empty());
        assert_eq!(p.foot_z(), 0.0);
    }

    #[test]
    fn a_label_widens_the_raft_and_its_rim_goes_round_it() {
        let mut p = PresetId::Medium.defaults();
        p.raft.edge = RaftEdge::Straight;
        (p.raft.rim_height, p.raft.rim_width) = (1.0, 1.0);
        let feet = [foot(0.0, 0.0), foot(10.0, 3.0)];
        let label = resin_geom::Label { text: "7".into(), side: resin_geom::LabelSide::Front, size: 5.0, depth: 0.5, margin: 1.5, preset: None };
        // The foot farthest to the front is (0, 0): the label is centred on it.
        let sites = [(label, vec![[0.0, 0.0], [10.0, 3.0]])];
        let bare = Mesh::new(raft_triangles(&feet, &[], &p));
        let widened = Mesh::new(raft_triangles(&feet, &sites, &p));
        let (b, w) = (bare.bounds(), widened.bounds());
        // Medium: the outline 4.1 mm round each foot; the plate past it, 5 mm of text and its margins.
        assert!((b.min.y + 4.1).abs() < 0.05 && w.min.y < -4.1 - 5.0, "{b:?} {w:?}");
        assert!(widened.signed_volume() > bare.signed_volume() + 50.0);
        // The rim's top runs along the plate's far edge, and nowhere inside it: the hollow joins the raft's.
        let top = p.raft.thickness + p.raft.rim_height;
        let rim_y: Vec<f64> = widened
            .triangles
            .iter()
            .flatten()
            .filter(|v| (f64::from(v[2]) - top).abs() < 1e-6 && f64::from(v[0]).abs() < 0.5)
            .map(|v| f64::from(v[1]))
            .collect();
        assert!(rim_y.iter().any(|y| *y < w.min.y + 1.5), "{rim_y:?}");
        assert!(rim_y.iter().all(|y| *y < w.min.y + 1.5 || *y > 4.1 - 1.5), "no rim across the join: {rim_y:?}");
        let letters = Mesh::new(crate::label_triangles(&sites, &p)).bounds();
        assert!((letters.max.y + 2.1 + 1.5).abs() < 0.05, "the margin from the base: {letters:?}");
        assert!(letters.min.y > w.min.y + 1.95, "past the rim's inset and the edge's margin: {letters:?} {w:?}");
    }

    #[test]
    fn hostile_feet_give_no_raft_or_a_bounded_one() {
        let p = PresetId::Medium.defaults();
        assert!(raft_triangles(&[], &[], &p).is_empty());
        assert!(raft_triangles(&[Vec3::new(f64::NAN, 0.0, 0.0)], &[], &p).is_empty());
        assert!(raft_triangles(&[foot(0.0, 0.0), foot(1e12, 0.0)], &[], &p).is_empty());
    }
}
