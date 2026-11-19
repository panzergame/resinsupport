//! Models' labels on the raft. Like the raft they join, their solids are derived, never stored:
//! a label is placed from its model's supports' feet each time. The raft takes in each label's
//! plate ([`crate::raft_triangles`]); the letters standing on it are made here.

use resin_geom::Label;

use crate::raft::label_layouts;
use crate::{Route, Scene, SupportPreset, raft_feet};

/// Each labelled model's label and the feet of its supports on the plate, `(x, y)`, in model
/// order: what the labels are built from. `routes` are the printed supports' routes, by the model
/// they hold up. A model with no foot is left out.
pub fn label_sites<'a>(scene: &Scene, routes: impl IntoIterator<Item = (u64, &'a Route)>) -> Vec<(Label, Vec<[f64; 2]>)> {
    let routes: Vec<(u64, &Route)> = routes.into_iter().collect();
    scene
        .models()
        .iter()
        .filter_map(|m| {
            let label = m.label.as_ref()?;
            let feet: Vec<[f64; 2]> =
                raft_feet(routes.iter().filter(|(model, _)| *model == m.id).map(|(_, r)| *r)).iter().map(|f| [f.x, f.y]).collect();
            (!feet.is_empty()).then(|| (label.clone(), feet))
        })
        .collect()
}

/// The letters of the labels at `sites`, standing on the raft `p` makes, closed and wound
/// outwards; nothing when `p` has no raft.
pub fn label_triangles(sites: &[(Label, Vec<[f64; 2]>)], p: &SupportPreset) -> Vec<[[f32; 3]; 3]> {
    label_layouts(sites, p).iter().flat_map(|l| l.letters(p.raft.thickness)).collect()
}
