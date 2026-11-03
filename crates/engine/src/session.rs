use std::collections::BTreeSet;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

use resin_geom::{Aabb3, Affine3, Mesh, MeshIndex, Placement, RayHit, Vec3};
use resin_render::Camera;
use serde_json::Value;

use crate::{EngineError, Result, cmd};

/// How far from the origin a model may be moved or grown, in millimetres: well past any build
/// plate, and still exact enough in the f32 the mesh is stored in.
pub const MAX_EXTENT: f64 = 1.0e6;

/// Scale factors outside this range are typos, not intents.
pub const SCALE_RANGE: std::ops::RangeInclusive<f64> = 0.001..=1000.0;

/// One imported model. Its mesh and index stay as imported, so the front end can key GPU buffers
/// on `id`; where it sits is `placement`, always measured from the imported mesh.
#[derive(Clone, Debug)]
pub struct Model {
    pub id: u64,
    pub name: String,
    /// The mesh as imported, in its own coordinates.
    pub mesh: Arc<Mesh>,
    /// The mesh's triangles in a hierarchy, for picking.
    pub index: Arc<MeshIndex>,
    /// The imported mesh's bounds centre: the point `placement.position` puts somewhere.
    pub pivot: Vec3,
    pub placement: Placement,
    /// `placement` as a map from the mesh's coordinates to the scene's.
    pub transform: Affine3,
    /// In the scene.
    pub bounds: Aabb3,
}

impl Model {
    /// The world placement: transform and bounds for `p`. Fails when the model would collapse
    /// or leave [`MAX_EXTENT`].
    fn placed(&self, p: Placement) -> Result<(Affine3, Aabb3)> {
        let bad = |why: &str| Err(EngineError::BadParams(format!("model {}: {why}", self.id)));
        if !p.is_finite() {
            return bad("the placement must be finite numbers");
        }
        if ![p.scale.x, p.scale.y, p.scale.z].iter().all(|k| SCALE_RANGE.contains(k)) {
            return bad(&format!("scale factors must be from {} to {}", SCALE_RANGE.start(), SCALE_RANGE.end()));
        }
        let xf = p.affine(self.pivot);
        let bounds: Aabb3 = self.mesh.triangles.iter().flatten().map(|&v| xf.point(Vec3::from(v))).collect();
        let far = bounds.min.min(-bounds.max);
        if bounds.is_empty() || !(far.x >= -MAX_EXTENT && far.y >= -MAX_EXTENT && far.z >= -MAX_EXTENT) {
            return bad(&format!("it would leave the {MAX_EXTENT} mm workspace"));
        }
        Ok((xf, bounds))
    }
    /// The outward unit normal of triangle `i`, in the scene.
    pub fn face_normal(&self, i: usize) -> Option<Vec3> {
        let n = self.mesh.triangles.get(i).and_then(Mesh::face_normal)?;
        self.transform.normal(n)
    }
    /// The enclosed volume in the scene, mm³ (meaningless for an open mesh).
    pub fn volume(&self) -> f64 {
        (self.mesh.signed_volume() * self.transform.determinant()).abs()
    }
    /// The model's size along its own axes, scaled: what Scale's target sizes measure.
    pub fn scaled_size(&self) -> Vec3 {
        let s = self.mesh.bounds().size();
        let k = self.placement.scale;
        Vec3::new(s.x * k.x, s.y * k.y, s.z * k.z)
    }
}

/// A support's tip: where it touches a model's surface.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Support {
    pub id: u64,
    pub model: u64,
    /// In the scene.
    pub tip: Vec3,
    /// The surface's outward unit normal at the tip, in the scene.
    pub normal: Vec3,
    /// The same in the model's own coordinates: what the tip keeps as the model is placed.
    local_tip: Vec3,
    local_normal: Vec3,
}

impl Support {
    /// The tip's contact sphere, in millimetres. Fixed until supports get settings.
    pub const TIP_DIAMETER: f64 = 0.5;
}

#[derive(Clone, Debug, Default)]
pub struct Scene {
    pub models: Vec<Model>,
    pub supports: Vec<Support>,
    /// Selected supports, by id; only ids in `supports`.
    pub selected: BTreeSet<u64>,
    /// Selected models, by id; only ids in `models`.
    pub selected_models: BTreeSet<u64>,
    next_id: u64,
    next_support_id: u64,
    revision: u64,
}

impl Scene {
    pub fn bounds(&self) -> Aabb3 {
        self.models.iter().fold(Aabb3::EMPTY, |b, m| b.union(&m.bounds))
    }
    pub fn model(&self, id: u64) -> Option<&Model> {
        self.models.iter().find(|m| m.id == id)
    }
    /// Changes whenever a model or support is added, removed or placed: a cache of anything
    /// picked from the scene is stale once it does.
    pub fn revision(&self) -> u64 {
        self.revision
    }
    /// The box around these models.
    pub fn bounds_of(&self, ids: &[u64]) -> Aabb3 {
        self.models.iter().filter(|m| ids.contains(&m.id)).fold(Aabb3::EMPTY, |b, m| b.union(&m.bounds))
    }
    pub(crate) fn add(&mut self, name: String, mesh: Mesh) -> u64 {
        self.next_id += 1;
        self.revision += 1;
        let bounds = mesh.bounds();
        let pivot = bounds.center();
        let index = Arc::new(MeshIndex::new(&mesh));
        self.models.push(Model {
            id: self.next_id,
            name,
            mesh: Arc::new(mesh),
            index,
            pivot,
            placement: Placement::at(pivot),
            transform: Affine3::IDENTITY,
            bounds,
        });
        self.next_id
    }
    /// Give each listed model its placement, carrying its supports along. All or nothing: a
    /// placement that fails (unknown model, out of range) changes nothing.
    pub(crate) fn place(&mut self, placements: &[(u64, Placement)]) -> Result<()> {
        let mut placed = Vec::with_capacity(placements.len());
        for (id, p) in placements {
            let m = self.model(*id).ok_or_else(|| EngineError::BadParams(format!("no model {id}")))?;
            let (xf, bounds) = m.placed(*p)?;
            placed.push((*id, *p, xf, bounds));
        }
        self.revision += 1;
        for (id, p, xf, bounds) in placed {
            if let Some(m) = self.models.iter_mut().find(|m| m.id == id) {
                m.placement = p;
                m.transform = xf;
                m.bounds = bounds;
            }
            for s in self.supports.iter_mut().filter(|s| s.model == id) {
                s.tip = xf.point(s.local_tip);
                s.normal = xf.normal(s.local_normal).unwrap_or(s.normal);
            }
        }
        Ok(())
    }
    /// A tip at scene point `tip` with scene normal `normal` on `model`, which must exist.
    pub(crate) fn add_support(&mut self, model: u64, tip: Vec3, normal: Vec3) -> Result<u64> {
        let m = self.model(model).ok_or_else(|| EngineError::BadParams(format!("no model {model}")))?;
        let inv = m.transform.inverse().ok_or_else(|| EngineError::Failed(format!("model {model} is degenerate")))?;
        let local_tip = inv.point(tip);
        let local_normal = inv.normal(normal).unwrap_or(normal);
        self.next_support_id += 1;
        self.revision += 1;
        self.supports.push(Support { id: self.next_support_id, model, tip, normal, local_tip, local_normal });
        Ok(self.next_support_id)
    }
    /// Remove these models and their supports; whether any model went.
    pub(crate) fn remove_models(&mut self, gone: impl Fn(u64) -> bool) -> bool {
        let before = self.models.len();
        self.models.retain(|m| !gone(m.id));
        self.supports.retain(|s| !gone(s.model));
        self.prune_selection();
        self.revision += 1;
        self.models.len() != before
    }
    pub(crate) fn remove_supports(&mut self, ids: &[u64]) {
        self.supports.retain(|t| !ids.contains(&t.id));
        self.prune_selection();
        self.revision += 1;
    }
    fn prune_selection(&mut self) {
        let supports = &self.supports;
        self.selected.retain(|id| supports.iter().any(|s| s.id == *id));
        let models = &self.models;
        self.selected_models.retain(|id| models.iter().any(|m| m.id == *id));
    }
    /// The nearest model surface along the ray from `origin` along unit `dir`: the model's id
    /// and where it is hit, in the scene. Each model is picked in its own coordinates.
    pub fn ray_hit(&self, origin: Vec3, dir: Vec3) -> Option<(u64, RayHit)> {
        self.models
            .iter()
            .filter_map(|m| {
                let inv = m.transform.inverse()?;
                let d = inv.vector(dir);
                let stretch = d.len();
                let h = m.index.ray_hit(&m.mesh, inv.point(origin), d.normalized()?)?;
                Some((m.id, RayHit { t: h.t / stretch, point: m.transform.point(h.point), triangle: h.triangle }))
            })
            .min_by(|a, b| a.1.t.total_cmp(&b.1.t))
    }
    /// The nearest support tip along the ray: its id and the distance to it.
    pub fn support_hit(&self, origin: Vec3, dir: Vec3) -> Option<(u64, f64)> {
        let r = Support::TIP_DIAMETER / 2.0;
        self.supports.iter().filter_map(|s| resin_geom::ray_sphere(origin, dir, s.tip, r).map(|t| (s.id, t))).min_by(|a, b| a.1.total_cmp(&b.1))
    }
}

/// What is open: the models and the view onto them.
#[derive(Clone, Debug, Default)]
pub struct Session {
    pub scene: Scene,
    pub camera: Camera,
}

impl Session {
    pub fn new() -> Session {
        Session::default()
    }

    /// Run command `id` with `params`. A failing command leaves the session as it was; a panic
    /// inside one (which would be a bug) is caught, the session restored and
    /// [`EngineError::Internal`] returned.
    pub fn execute(&mut self, id: &str, params: &Value) -> Result<Value> {
        let spec = cmd::find(id).ok_or_else(|| EngineError::UnknownCommand(id.to_owned()))?;
        let before = self.clone();
        let outcome = catch_unwind(AssertUnwindSafe(|| (spec.run)(self, params)));
        match outcome {
            Ok(Ok(v)) if self.camera.is_valid() => Ok(v),
            Ok(Ok(_)) => {
                *self = before;
                Err(EngineError::Internal(format!("{id} left the camera invalid")))
            }
            Ok(Err(e)) => {
                *self = before;
                Err(e)
            }
            Err(panic) => {
                *self = before;
                let msg = panic.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| panic.downcast_ref::<String>().cloned()).unwrap_or_default();
                log::error!("command {id} panicked: {msg}");
                Err(EngineError::Internal(format!("{id} panicked: {msg}")))
            }
        }
    }
}
