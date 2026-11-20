//! What the slicer is given, and the job it gives back. The job is derived from the scene and the
//! settings, never an undo step: it lives on the session, and its [`Stamp`] says whether it still
//! matches them.

use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Arc;

use resin_geom::Vec3;
use resin_slice::{CtbFormat, PrinterProfile, SliceInput, SlicedJob, Solid, SolidKind, Stamp};
use serde::Serialize;

use crate::support_solids::SupportTessellator;
use crate::{PrintConfig, Scene, Session, SupportPreset, brace_solids, label_sites, label_triangles, raft_feet, raft_triangles, route};

/// A sliced job and what was left out of it.
#[derive(Clone, Debug, PartialEq)]
pub struct Job {
    /// Shared, so a file can be written from it on another thread without copying its layers.
    pub sliced: Arc<SlicedJob>,
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

    pub fn profile(&self) -> &PrinterProfile {
        &self.config.printer
    }

    /// The slicer's input: each model placed, each support that reaches the plate (a branch: through
    /// its parent) as its closed solids, the raft under them and the models' labels on it; and how
    /// many supports do not reach it, which are left out. A model with slicing settings of its own
    /// takes them, and so do its supports and the braces between them; the raft and the labels
    /// keep the plate's.
    pub fn input(&self) -> (SliceInput, usize) {
        let models = self.scene.models();
        let plate = self.config.settings.slicing;
        let own = |model: u64| models.iter().find(|m| m.id == model).filter(|m| !m.slicing.is_empty()).map(|m| m.slicing.apply(&plate));
        let mut solids: Vec<Solid> = models
            .iter()
            .map(|m| Solid {
                kind: SolidKind::Model,
                triangles: m.mesh.triangles.iter().map(|t| t.map(|v| m.transform.point(Vec3::from(v)).to_f32())).collect(),
                slicing: own(m.id),
            })
            .collect();
        let mut unreachable = 0;
        let mut tess = SupportTessellator::default();
        // A branch hangs from its parent, which comes before it: printed without it, it would float.
        let mut printed = std::collections::BTreeMap::new();
        for s in self.scene.supports() {
            let r = route(&self.scene, s, &self.preset);
            if !r.reachable() || s.parent.is_some_and(|p| !printed.contains_key(&p)) {
                unreachable += 1;
                continue;
            }
            let mut triangles = Vec::new();
            tess.triangles(&r, &self.preset, &mut triangles);
            solids.push(Solid { kind: SolidKind::Support, triangles, slicing: own(s.model) });
            printed.insert(s.id, (r, s.model));
        }
        // Only between printed supports: a brace to one left out would hang in the air.
        for b in self.scene.braces() {
            let (Some((ra, ma)), Some((rb, mb))) = (printed.get(&b.a), printed.get(&b.b)) else { continue };
            let (struts, _) = brace_solids(&self.scene, ra, rb, &b.params, &self.preset);
            if struts.is_empty() {
                continue;
            }
            let mut triangles = Vec::new();
            tess.strut_triangles(&struts, b.params.diameter_ratio, &self.preset, &mut triangles);
            // Between two models' supports it is neither's: the plate's.
            let slicing = if ma == mb { own(*ma) } else { None };
            solids.push(Solid { kind: SolidKind::Support, triangles, slicing });
        }
        let sites = label_sites(&self.scene, printed.values().map(|(r, m)| (*m, r)));
        let raft = raft_triangles(&raft_feet(printed.values().map(|(r, _)| r)), &sites, &self.preset);
        if !raft.is_empty() {
            solids.push(Solid { kind: SolidKind::Support, triangles: raft, slicing: None });
        }
        let labels = label_triangles(&sites, &self.preset);
        if !labels.is_empty() {
            solids.push(Solid { kind: SolidKind::Support, triangles: labels, slicing: None });
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
    /// and the support preset in use. The CTB format is left out: it changes the file, not the
    /// layers.
    pub fn slice_stamp(&self) -> Stamp {
        let mut print = self.print_config.clone();
        print.printer.ctb_format = CtbFormat::default();
        Stamp { scene: self.scene.revision(), print: hash_of(&print), supports: hash_of(self.support_settings.current()) }
    }

    pub fn slice_source(&self) -> SliceSource {
        let preset = *self.support_settings.current();
        // A hidden model is not printed, nor its supports.
        SliceSource { scene: self.scene.shown(&preset), preset, config: self.print_config.clone(), stamp: self.slice_stamp() }
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
        self.job = Some(Job { sliced: Arc::new(sliced), unreachable_supports });
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
