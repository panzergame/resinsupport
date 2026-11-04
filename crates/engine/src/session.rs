use std::collections::BTreeSet;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

use resin_geom::{Aabb3, Affine3, Mesh, MeshIndex, Placement, RayHit, Vec3};
use resin_io::{Project, ProjectModel, ProjectSupport};
use resin_render::Camera;
use serde_json::Value;

use crate::{CommandSpec, EngineError, Result, SupportSettings, cmd};

/// How far from the origin a model may be moved or grown, in millimetres: well past any build
/// plate, and still exact enough in the f32 the mesh is stored in.
pub const MAX_EXTENT: f64 = 1.0e6;

/// Scale factors outside this range are typos, not intents.
pub const SCALE_RANGE: std::ops::RangeInclusive<f64> = 0.001..=1000.0;

/// How high above the plate a lifted model's lowest point sits, in millimetres, until changed:
/// room for supports under it.
pub const DEFAULT_LIFT_HEIGHT: f64 = 5.0;

/// The lift heights `scene.lift_height` takes.
pub const LIFT_RANGE: std::ops::RangeInclusive<f64> = 0.0..=500.0;

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
    /// Its lowest point is kept at [`Scene::lift_height`]: placing it any way but moving it up or
    /// down puts it back there.
    pub lifted: bool,
}

impl Model {
    /// The world placement for `p`, its Z moved so the lowest point is at `lift` if given:
    /// placement, transform and bounds. Fails when the model would collapse or leave
    /// [`MAX_EXTENT`].
    fn placed(&self, mut p: Placement, lift: Option<f64>) -> Result<(Placement, Affine3, Aabb3)> {
        let bad = |why: &str| Err(EngineError::BadParams(format!("model {}: {why}", self.id)));
        if !p.is_finite() {
            return bad("the placement must be finite numbers");
        }
        if ![p.scale.x, p.scale.y, p.scale.z].iter().all(|k| SCALE_RANGE.contains(k)) {
            return bad(&format!("scale factors must be from {} to {}", SCALE_RANGE.start(), SCALE_RANGE.end()));
        }
        let mut xf = p.affine(self.pivot);
        let mut bounds: Aabb3 = self.mesh.triangles.iter().flatten().map(|&v| xf.point(Vec3::from(v))).collect();
        if let Some(h) = lift.filter(|_| !bounds.is_empty()) {
            // A translation only: the bounds move with it, no vertex is walked again.
            let up = Vec3::new(0.0, 0.0, h - bounds.min.z);
            p.position += up;
            xf = xf.then(&Affine3::translation(up));
            bounds = Aabb3 { min: bounds.min + up, max: bounds.max + up };
        }
        let far = bounds.min.min(-bounds.max);
        if bounds.is_empty() || !(far.x >= -MAX_EXTENT && far.y >= -MAX_EXTENT && far.z >= -MAX_EXTENT) {
            return bad(&format!("it would leave the {MAX_EXTENT} mm workspace"));
        }
        Ok((p, xf, bounds))
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

/// A support: where its tip touches a model's surface, and the path the user gave it, if any.
#[derive(Clone, Debug, PartialEq)]
pub struct Support {
    pub id: u64,
    pub model: u64,
    /// In the scene.
    pub tip: Vec3,
    /// The surface's outward unit normal at the tip, in the scene.
    pub normal: Vec3,
    /// The body's corners the user placed (the head, then each knee), in the scene; the foot
    /// drops straight down from the last. `None` while the route is worked out from the tip.
    /// Moving the model carries them with it, under the plate too (flagged); turning or scaling it
    /// leaves them where they were put, and the tip stretches to them.
    pub corners: Option<Vec<Vec3>>,
    /// The same in the model's own coordinates: what the tip keeps as the model is placed.
    local_tip: Vec3,
    local_normal: Vec3,
}

/// A part of a support, numbered along its route's body ([`crate::Route::body`]): joint `i` is
/// corner `i` (the head is 0, a knee 1), segment `i` runs from corner `i` to corner `i + 1`.
/// Routes are derived, so a number can outlive the route it named: anything reading one skips a
/// number the route no longer has.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SupportPart {
    Joint(u32),
    Segment(u32),
}

impl SupportPart {
    /// The corners of `body` it spans, if the route still has it: one for a joint (never the
    /// foot), two for a segment.
    pub fn corners(self, body: &[Vec3]) -> Option<(Vec3, Option<Vec3>)> {
        match self {
            SupportPart::Joint(i) => {
                let i = i as usize;
                (i + 1 < body.len()).then(|| body.get(i).map(|&p| (p, None))).flatten()
            }
            SupportPart::Segment(i) => {
                let i = i as usize;
                Some((*body.get(i)?, Some(*body.get(i + 1)?)))
            }
        }
    }
}

/// Everything an undo step puts back: what is in the scene and what is selected. The camera is
/// not part of it. New user state belongs here, so that undo covers it without further work.
#[derive(Clone, Debug)]
pub struct Scene {
    // Private so that every change goes through a method that bumps `revision`: that bump is what
    // makes a command an undo step and invalidates pick caches.
    models: Vec<Model>,
    supports: Vec<Support>,
    /// Selected supports, by id; only ids in `supports`.
    pub selected: BTreeSet<u64>,
    /// Selected parts of supports: (support id, part); only ids in `supports`.
    pub selected_parts: BTreeSet<(u64, SupportPart)>,
    /// Selected models, by id; only ids in `models`.
    pub selected_models: BTreeSet<u64>,
    /// Where lifted models' lowest points sit. Here rather than beside the support settings:
    /// changing it moves models, so undoing that must put it back with them.
    lift_height: f64,
    next_id: u64,
    next_support_id: u64,
    revision: u64,
}

impl Default for Scene {
    fn default() -> Scene {
        Scene {
            models: Vec::new(),
            supports: Vec::new(),
            selected: BTreeSet::new(),
            selected_parts: BTreeSet::new(),
            selected_models: BTreeSet::new(),
            lift_height: DEFAULT_LIFT_HEIGHT,
            next_id: 0,
            next_support_id: 0,
            revision: 0,
        }
    }
}

impl Scene {
    pub fn models(&self) -> &[Model] {
        &self.models
    }
    pub fn supports(&self) -> &[Support] {
        &self.supports
    }
    pub fn bounds(&self) -> Aabb3 {
        self.models.iter().fold(Aabb3::EMPTY, |b, m| b.union(&m.bounds))
    }
    pub fn model(&self, id: u64) -> Option<&Model> {
        self.models.iter().find(|m| m.id == id)
    }
    /// How high above the plate lifted models sit.
    pub fn lift_height(&self) -> f64 {
        self.lift_height
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
        self.add_shared(name, Arc::new(mesh))
    }
    fn add_shared(&mut self, name: String, mesh: Arc<Mesh>) -> u64 {
        self.next_id += 1;
        self.revision += 1;
        let bounds = mesh.bounds();
        let pivot = bounds.center();
        let index = Arc::new(MeshIndex::new(&mesh));
        self.models.push(Model {
            id: self.next_id,
            name,
            mesh,
            index,
            pivot,
            placement: Placement::at(pivot),
            transform: Affine3::IDENTITY,
            bounds,
            lifted: false,
        });
        self.next_id
    }
    /// Give each listed model its placement and, when given, whether it is lifted (`None` keeps
    /// what it is), carrying its supports along. A lifted model's Z is then moved to the lift
    /// height. All or nothing: a placement that fails (unknown model, out of range) changes
    /// nothing.
    pub(crate) fn place(&mut self, placements: &[(u64, Placement, Option<bool>)]) -> Result<()> {
        let mut placed = Vec::with_capacity(placements.len());
        for (id, p, lifted) in placements {
            let m = self.model(*id).ok_or_else(|| EngineError::BadParams(format!("no model {id}")))?;
            let lifted = lifted.unwrap_or(m.lifted);
            let (p, xf, bounds) = m.placed(*p, lifted.then_some(self.lift_height))?;
            placed.push((*id, p, lifted, xf, bounds));
        }
        // Placing where it already is must not bump: it would make an empty undo step.
        if !placed.iter().any(|(id, p, lifted, ..)| self.model(*id).is_some_and(|m| m.placement != *p || m.lifted != *lifted)) {
            return Ok(());
        }
        self.revision += 1;
        for (id, p, lifted, xf, bounds) in placed {
            let mut shift = None;
            if let Some(m) = self.models.iter_mut().find(|m| m.id == id) {
                // Only a move carries placed corners: turned or scaled, the model leaves them where
                // the user put them, and the tip stretches to them.
                if p.rotation == m.placement.rotation && p.scale == m.placement.scale {
                    shift = Some(p.position - m.placement.position);
                }
                m.placement = p;
                m.transform = xf;
                m.bounds = bounds;
                m.lifted = lifted;
            }
            for s in self.supports.iter_mut().filter(|s| s.model == id) {
                s.tip = xf.point(s.local_tip);
                s.normal = xf.normal(s.local_normal).unwrap_or(s.normal);
                if let (Some(d), Some(corners)) = (shift, s.corners.as_mut()) {
                    // Even under the plate: the route is flagged, and the user moves the corners back.
                    for c in corners {
                        *c += d;
                    }
                }
            }
        }
        Ok(())
    }
    /// Set the lift height and put every lifted model at it. All or nothing, as [`Scene::place`].
    pub(crate) fn set_lift_height(&mut self, h: f64) -> Result<()> {
        if !LIFT_RANGE.contains(&h) {
            return Err(EngineError::BadParams(format!("the lift height must be from {} to {} mm", LIFT_RANGE.start(), LIFT_RANGE.end())));
        }
        if h == self.lift_height {
            return Ok(());
        }
        let old = std::mem::replace(&mut self.lift_height, h);
        let lifted: Vec<(u64, Placement, Option<bool>)> = self.models.iter().filter(|m| m.lifted).map(|m| (m.id, m.placement, None)).collect();
        if let Err(e) = self.place(&lifted) {
            self.lift_height = old;
            return Err(e);
        }
        // The height alone is a change too: it decides where the next lifted model goes.
        self.revision += 1;
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
        self.supports.push(Support { id: self.next_support_id, model, tip, normal, corners: None, local_tip, local_normal });
        Ok(self.next_support_id)
    }
    /// Give each listed support the corners the user placed, or (`None`) its worked-out route
    /// back. All or nothing: an unknown support changes nothing.
    pub(crate) fn set_corners(&mut self, corners: Vec<(u64, Option<Vec<Vec3>>)>) -> Result<()> {
        if let Some((id, _)) = corners.iter().find(|(id, _)| !self.supports.iter().any(|s| s.id == *id)) {
            return Err(EngineError::BadParams(format!("no support {id}")));
        }
        // Setting what is already there must not bump: it would make an empty undo step.
        if corners.iter().all(|(id, c)| self.supports.iter().any(|s| s.id == *id && s.corners == *c)) {
            return Ok(());
        }
        self.revision += 1;
        for (id, c) in corners {
            if let Some(s) = self.supports.iter_mut().find(|s| s.id == id) {
                s.corners = c;
            }
        }
        Ok(())
    }
    /// Remove these models and their supports; whether any model went.
    pub(crate) fn remove_models(&mut self, gone: impl Fn(u64) -> bool) -> bool {
        let before = self.models.len();
        self.models.retain(|m| !gone(m.id));
        self.supports.retain(|s| !gone(s.model));
        self.prune_selection();
        // Removing nothing must not bump: it would make an empty undo step.
        let removed = self.models.len() != before;
        if removed {
            self.revision += 1;
        }
        removed
    }
    pub(crate) fn remove_supports(&mut self, ids: &[u64]) {
        let before = self.supports.len();
        self.supports.retain(|t| !ids.contains(&t.id));
        self.prune_selection();
        if self.supports.len() != before {
            self.revision += 1;
        }
    }
    /// What a project file keeps: each model's mesh, name and placement, and each support's tip
    /// and normal on its model and placed corners. Not the lift (a model setting), ids or the
    /// selection.
    pub fn project(&self) -> Project {
        let models = self.models.iter().map(|m| ProjectModel { name: m.name.clone(), mesh: m.mesh.clone(), placement: m.placement }).collect();
        let supports = self
            .supports
            .iter()
            .filter_map(|s| {
                let model = self.models.iter().position(|m| m.id == s.model)?;
                Some(ProjectSupport { model, tip: s.local_tip, normal: s.local_normal, corners: s.corners.clone() })
            })
            .collect();
        Project { models, supports }
    }
    /// Become the project `p`: everything in the scene goes, its models and
    /// supports come in with new ids, unlifted, exactly where they were saved. The lift height
    /// stays. Fails, half done, on a placement or support the scene would refuse: the caller puts
    /// the scene back, as [`Session::execute`] does.
    pub(crate) fn adopt(&mut self, p: Project) -> Result<()> {
        self.models.clear();
        self.supports.clear();
        self.prune_selection();
        self.revision += 1;
        let mut ids = Vec::with_capacity(p.models.len());
        for pm in p.models {
            let id = self.add_shared(pm.name, pm.mesh);
            let m = self.model(id).ok_or_else(|| EngineError::Internal(format!("model {id} went missing")))?;
            let (placement, transform, bounds) = m.placed(pm.placement, None)?;
            if let Some(m) = self.models.iter_mut().find(|m| m.id == id) {
                m.placement = placement;
                m.transform = transform;
                m.bounds = bounds;
            }
            ids.push(id);
        }
        for ps in p.supports {
            let bad = |why: &str| EngineError::Failed(format!("a support in the file {why}"));
            let model = *ids.get(ps.model).ok_or_else(|| bad("names no model"))?;
            let m = self.model(model).ok_or_else(|| bad("names no model"))?;
            let local_normal = ps.normal.normalized().ok_or_else(|| bad("has no normal"))?;
            let tip = m.transform.point(ps.tip);
            let normal = m.transform.normal(local_normal).ok_or_else(|| bad("has no normal"))?;
            let inside = |p: &Vec3| p.is_finite() && p.x.abs() <= MAX_EXTENT && p.y.abs() <= MAX_EXTENT && p.z.abs() <= MAX_EXTENT;
            if !inside(&tip) || !ps.corners.as_ref().is_none_or(|c| !c.is_empty() && c.iter().all(inside)) {
                return Err(bad(&format!("is outside the {MAX_EXTENT} mm workspace")));
            }
            self.next_support_id += 1;
            self.supports.push(Support { id: self.next_support_id, model, tip, normal, corners: ps.corners, local_tip: ps.tip, local_normal });
        }
        Ok(())
    }
    /// Become `older` (an undo or redo step). Ids and the revision only ever grow: a model id is
    /// never reused for another mesh (the GPU keys meshes on it), and a cache keyed on the
    /// revision never mistakes the restored scene for one it has seen.
    pub(crate) fn restore(&mut self, mut older: Scene) -> Scene {
        older.next_id = older.next_id.max(self.next_id);
        older.next_support_id = older.next_support_id.max(self.next_support_id);
        older.revision = older.revision.max(self.revision) + 1;
        std::mem::replace(self, older)
    }
    fn prune_selection(&mut self) {
        let supports = &self.supports;
        self.selected.retain(|id| supports.iter().any(|s| s.id == *id));
        self.selected_parts.retain(|(id, _)| supports.iter().any(|s| s.id == *id));
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
    /// The nearest support tip along the ray, a sphere of `r` around its contact: its id and the
    /// distance to it.
    pub fn support_hit(&self, origin: Vec3, dir: Vec3, r: f64) -> Option<(u64, f64)> {
        self.supports.iter().filter_map(|s| resin_geom::ray_sphere(origin, dir, s.tip, r).map(|t| (s.id, t))).min_by(|a, b| a.1.total_cmp(&b.1))
    }
}

/// How many undo steps are kept; the oldest go first.
pub const MAX_UNDO: usize = 200;

/// The scene before (on the undo stack) or after (on the redo stack) one step.
#[derive(Clone, Debug)]
pub struct Snapshot {
    /// The label of the command that made the step.
    pub label: &'static str,
    /// The id of that command.
    pub command: &'static str,
    /// Numbers steps in the order they were made, so a mark survives the oldest steps dropping.
    pub(crate) serial: u64,
    pub scene: Scene,
}

/// Undo and redo. Taken out of the session while a command runs, so that `execute` copies only
/// the scene and camera to put back on failure.
#[derive(Clone, Debug, Default)]
pub(crate) struct History {
    pub(crate) undo: Vec<Snapshot>,
    pub(crate) redo: Vec<Snapshot>,
    next_serial: u64,
    /// `Some` between `begin_step` and `end_step`; true once the step has been recorded.
    open: Option<bool>,
}

/// What is open: the models and the view onto them, the support settings, and the undo history.
#[derive(Clone, Debug, Default)]
pub struct Session {
    pub scene: Scene,
    pub camera: Camera,
    /// Outside `scene` on purpose: a settings change is never an undo step, and undoing a support
    /// edit never changes a setting. Changed only through validated commands or the setter.
    pub(crate) support_settings: SupportSettings,
    pub(crate) history: History,
    /// The scene's file and the scene revision last saved to or opened from it.
    pub(crate) file: Option<(String, u64)>,
}

impl Session {
    pub fn new() -> Session {
        Session::default()
    }

    /// Run command `id` with `params`. A failing command leaves the session as it was; a panic
    /// inside one (which would be a bug) is caught, the session restored and
    /// [`EngineError::Internal`] returned. A command that changes the scene (its revision) is one
    /// undo step, unless it is [`CommandSpec::undoable`] false or a step is open
    /// ([`Session::begin_step`]).
    pub fn execute(&mut self, id: &str, params: &Value) -> Result<Value> {
        let spec = cmd::find(id).ok_or_else(|| EngineError::UnknownCommand(id.to_owned()))?;
        let (scene, camera, settings) = (self.scene.clone(), self.camera, self.support_settings);
        let history = if spec.undoable { None } else { Some(self.history.clone()) };
        let restore = |s: &mut Session| {
            s.scene = scene.clone();
            s.camera = camera;
            s.support_settings = settings;
            if let Some(h) = &history {
                s.history = h.clone();
            }
        };
        let outcome = catch_unwind(AssertUnwindSafe(|| (spec.run)(self, params)));
        let result = match outcome {
            Ok(Ok(v)) if self.camera.is_valid() => Ok(v),
            Ok(Ok(_)) => {
                restore(self);
                Err(EngineError::Internal(format!("{id} left the camera invalid")))
            }
            Ok(Err(e)) => {
                restore(self);
                Err(e)
            }
            Err(panic) => {
                restore(self);
                let msg = panic.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| panic.downcast_ref::<String>().cloned()).unwrap_or_default();
                log::error!("command {id} panicked: {msg}");
                Err(EngineError::Internal(format!("{id} panicked: {msg}")))
            }
        };
        if result.is_ok() && spec.undoable && self.scene.revision != scene.revision {
            self.record(spec, scene);
        }
        result
    }

    fn record(&mut self, spec: &CommandSpec, before: Scene) {
        let h = &mut self.history;
        h.redo.clear();
        if h.open == Some(true) {
            return;
        }
        if h.open.is_some() {
            h.open = Some(true);
        }
        h.undo.push(Snapshot { label: spec.label, command: spec.id, serial: h.next_serial, scene: before });
        h.next_serial += 1;
        if h.undo.len() > MAX_UNDO {
            h.undo.remove(0);
        }
    }

    /// Forget every undo and redo step: what came before is out of reach. The serial keeps
    /// counting, so a [`Session::history_mark`] taken earlier never names a later step.
    pub(crate) fn clear_history(&mut self) {
        self.history.undo.clear();
        self.history.redo.clear();
        if self.history.open.is_some() {
            self.history.open = Some(false);
        }
    }

    /// Make every command until [`Session::end_step`] one undo step: a drag, or a delete of
    /// several models. Its label is the first command's. Opening a step already open does
    /// nothing.
    pub fn begin_step(&mut self) {
        if self.history.open.is_none() {
            self.history.open = Some(false);
        }
    }

    pub fn end_step(&mut self) {
        self.history.open = None;
    }

    /// Whether a step is open.
    pub fn step_open(&self) -> bool {
        self.history.open.is_some()
    }

    /// Where the history is now, for [`Session::forget_since`].
    pub fn history_mark(&self) -> u64 {
        self.history.next_serial
    }

    /// Drop the undo steps made since `mark`, if each was made by `command`: after a tool's
    /// Cancel has put the scene back, the history reads as though the tool never ran. Whether
    /// they were dropped; steps of any other command are kept, the scene and history must agree.
    pub fn forget_since(&mut self, mark: u64, command: &str) -> bool {
        let h = &mut self.history;
        let since = h.undo.iter().position(|s| s.serial >= mark).unwrap_or(h.undo.len());
        if !h.undo.get(since..).unwrap_or_default().iter().all(|s| s.command == command) {
            return false;
        }
        h.undo.truncate(since);
        // An open step may have been among them: the next change must record a new one.
        if h.open.is_some() {
            h.open = Some(false);
        }
        true
    }

    /// The command that made the step Undo would undo.
    pub fn undo_command(&self) -> Option<&'static str> {
        self.history.undo.last().map(|s| s.command)
    }

    /// The command that made the step Redo would redo.
    pub fn redo_command(&self) -> Option<&'static str> {
        self.history.redo.last().map(|s| s.command)
    }

    pub fn undo_depth(&self) -> usize {
        self.history.undo.len()
    }

    pub fn redo_depth(&self) -> usize {
        self.history.redo.len()
    }

    /// The file the scene was opened from or last saved to.
    pub fn file(&self) -> Option<&str> {
        self.file.as_ref().map(|(path, _)| path.as_str())
    }

    /// Whether closing now would lose work: the scene changed since it was saved or opened, or,
    /// never saved, it has models.
    pub fn modified(&self) -> bool {
        match &self.file {
            Some((_, revision)) => *revision != self.scene.revision(),
            None => !self.scene.models().is_empty(),
        }
    }

    /// The scene is now what the file at `path` holds.
    pub(crate) fn mark_saved(&mut self, path: String) {
        self.file = Some((path, self.scene.revision()));
    }

    /// Set the lift height (the stored one, at start), moving lifted models. Outside the undo
    /// history, as for [`Session::set_support_settings`].
    pub fn set_lift_height(&mut self, h: f64) -> Result<()> {
        self.scene.set_lift_height(h)
    }

    pub fn support_settings(&self) -> &SupportSettings {
        &self.support_settings
    }

    /// Replace the support settings (stored ones, at start), if they pass
    /// [`SupportSettings::validate`]. No undo step, as for `support.configure`.
    pub fn set_support_settings(&mut self, s: SupportSettings) -> Result<()> {
        s.validate()?;
        self.support_settings = s;
        Ok(())
    }
}
