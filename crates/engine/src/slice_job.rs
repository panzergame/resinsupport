//! What the slicer is given, and the job it gives back. The job is derived from the scene and the
//! settings, never an undo step: it lives on the session, and its [`Stamp`] says whether it still
//! matches them.

use std::hash::{DefaultHasher, Hash, Hasher};

use resin_geom::Vec3;
use resin_slice::{SliceInput, SlicedJob, Solid, SolidKind, Stamp};
use serde::Serialize;

use crate::support_solids::SupportTessellator;
use crate::{PrintConfig, Scene, Session, SupportPreset, route};

/// A sliced job and what was left out of it.
#[derive(Clone, Debug, PartialEq)]
pub struct Job {
    pub sliced: SlicedJob,
    /// Supports with no path to the plate: not printed.
    pub unreachable_supports: usize,
}

/// Everything a slice is built from, copied out of the session (the meshes are shared, not
/// copied), so the supports can be routed and the solids built away from the window thread.
#[derive(Clone, Debug)]
pub struct SliceSource {
    scene: Scene,
    preset: SupportPreset,
    config: PrintConfig,
    stamp: Stamp,
}

impl SliceSource {
    pub fn stamp(&self) -> Stamp {
        self.stamp
    }

    /// The slicer's input: each model placed, each support that reaches the plate (a branch: through
    /// its parent) as its closed solids; and how many supports do not reach it, which are left out.
    pub fn input(&self) -> (SliceInput, usize) {
        let models = self.scene.models();
        let mut solids: Vec<Solid> = models
            .iter()
            .map(|m| Solid {
                kind: SolidKind::Model,
                triangles: m.mesh.triangles.iter().map(|t| t.map(|v| m.transform.point(Vec3::from(v)).to_f32())).collect(),
            })
            .collect();
        let mut unreachable = 0;
        let mut tess = SupportTessellator::default();
        // A branch hangs from its parent, which comes before it: printed without it, it would float.
        let mut printed = std::collections::BTreeSet::new();
        for s in self.scene.supports() {
            let r = route(&self.scene, s, &self.preset);
            if !r.reachable() || s.parent.is_some_and(|p| !printed.contains(&p)) {
                unreachable += 1;
                continue;
            }
            printed.insert(s.id);
            let mut triangles = Vec::new();
            tess.triangles(&r, &self.preset, &mut triangles);
            solids.push(Solid { kind: SolidKind::Support, triangles });
        }
        let input = SliceInput { solids, profile: self.config.printer.clone(), settings: self.config.settings, stamp: self.stamp };
        (input, unreachable)
    }
}

fn hash_of(v: &impl Serialize) -> u64 {
    let mut h = DefaultHasher::new();
    serde_json::to_string(v).unwrap_or_default().hash(&mut h);
    h.finish()
}

impl Session {
    /// What a job sliced now would be stamped with: the scene's revision, the print settings
    /// and the support preset in use.
    pub fn slice_stamp(&self) -> Stamp {
        Stamp { scene: self.scene.revision(), print: hash_of(&self.print_config), supports: hash_of(self.support_settings.current()) }
    }

    pub fn slice_source(&self) -> SliceSource {
        SliceSource {
            scene: self.scene.clone(),
            preset: *self.support_settings.current(),
            config: self.print_config.clone(),
            stamp: self.slice_stamp(),
        }
    }

    /// The slicer's input now, and how many supports are left out: see [`SliceSource::input`].
    pub fn slice_input(&self) -> (SliceInput, usize) {
        self.slice_source().input()
    }

    /// Keep `job` if it was sliced from the scene and settings as they are now. Whether it was
    /// kept; a stale job is dropped.
    pub fn adopt_job(&mut self, sliced: SlicedJob, unreachable_supports: usize) -> bool {
        if sliced.stamp != self.slice_stamp() {
            return false;
        }
        self.job = Some(Job { sliced, unreachable_supports });
        true
    }

    /// The last job kept, current or not.
    pub fn job(&self) -> Option<&Job> {
        self.job.as_ref()
    }

    /// Whether there is a job and it matches the scene and settings as they are now.
    pub fn job_is_current(&self) -> bool {
        self.job.as_ref().is_some_and(|j| j.sliced.stamp == self.slice_stamp())
    }
}
