use std::collections::BTreeSet;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

use resin_geom::{Aabb3, Affine3, Item, Mesh, MeshIndex, Placement, RayHit, Rect, Vec3};
use resin_io::{Project, ProjectBrace, ProjectModel, ProjectSupport};
use resin_render::Camera;
use serde_json::Value;

use crate::{Brace, BraceParams, CommandSpec, EngineError, PrintConfig, Result, SupportPreset, SupportSettings, cmd};

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
    /// For a branch, the support whose head its tip runs straight to, sharing that support's body:
    /// never itself a branch, and always one before it in [`Scene::supports`].
    pub parent: Option<u64>,
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
    /// Struts between two supports' bodies, neither a branch.
    braces: Vec<Brace>,
    /// Selected supports, by id; only ids in `supports`.
    pub selected: BTreeSet<u64>,
    /// Selected parts of supports: (support id, part); only ids in `supports`.
    pub selected_parts: BTreeSet<(u64, SupportPart)>,
    /// Selected models, by id; only ids in `models`.
    pub selected_models: BTreeSet<u64>,
    /// Selected braces, by id; only ids in `braces`.
    pub selected_braces: BTreeSet<u64>,
    /// Where lifted models' lowest points sit. Here rather than beside the support settings:
    /// changing it moves models, so undoing that must put it back with them.
    lift_height: f64,
    next_id: u64,
    next_support_id: u64,
    next_brace_id: u64,
    revision: u64,
}

impl Default for Scene {
    fn default() -> Scene {
        Scene {
            models: Vec::new(),
            supports: Vec::new(),
            braces: Vec::new(),
            selected: BTreeSet::new(),
            selected_parts: BTreeSet::new(),
            selected_models: BTreeSet::new(),
            selected_braces: BTreeSet::new(),
            lift_height: DEFAULT_LIFT_HEIGHT,
            next_id: 0,
            next_support_id: 0,
            next_brace_id: 0,
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
    pub fn braces(&self) -> &[Brace] {
        &self.braces
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
    /// Arrange models `ids` on a `plate` (width, depth) centred on the origin, around the other
    /// models, which stay put ([`resin_geom::arrange`]): moved across, turned a quarter about Z
    /// to lay their long side along the plate's or when only that fits, and put down on the
    /// plate (a lifted one at the lift height). A model with supports is not turned: placed
    /// corners would stay behind. Returns the ids that found no room on the plate and went beside
    /// it. All or nothing, as [`Scene::place`].
    pub(crate) fn arrange(&mut self, ids: &[u64], plate: (f64, f64)) -> Result<Vec<u64>> {
        let footprint = |b: &Aabb3| Rect { min: [b.min.x, b.min.y], max: [b.max.x, b.max.y] };
        let fixed: Vec<Rect> = self.models.iter().filter(|m| !ids.contains(&m.id)).map(|m| footprint(&m.bounds)).collect();
        let moving: Vec<&Model> = self.models.iter().filter(|m| ids.contains(&m.id)).collect();
        let items: Vec<Item> =
            moving.iter().map(|m| Item { size: footprint(&m.bounds).size(), turnable: !self.supports.iter().any(|s| s.model == m.id) }).collect();
        let spots = resin_geom::arrange(Rect::centred(plate.0, plate.1), &fixed, &items);
        let mut beside = Vec::new();
        let mut placements = Vec::with_capacity(moving.len());
        for (m, spot) in moving.iter().zip(spots) {
            let (mut p, mut b) = (m.placement, footprint(&m.bounds));
            if spot.turned {
                // The last of the rotations is about the world Z through `position`, so a quarter
                // turn added to it turns the bounds about that point, exactly.
                let turned = |d: f64| {
                    let z = (p.rotation.z + d).rem_euclid(360.0);
                    if z > 180.0 { z - 360.0 } else { z }
                };
                let left = turned(90.0).abs() <= turned(-90.0).abs();
                p.rotation.z = if left { turned(90.0) } else { turned(-90.0) };
                // Both ways give the same size, but the bounds land elsewhere unless centred on
                // `position`.
                let (px, py) = (p.position.x, p.position.y);
                b = if left {
                    Rect { min: [px - (b.max[1] - py), py + (b.min[0] - px)], max: [px - (b.min[1] - py), py + (b.max[0] - px)] }
                } else {
                    Rect { min: [px + (b.min[1] - py), py - (b.max[0] - px)], max: [px + (b.max[1] - py), py - (b.min[0] - px)] }
                };
            }
            let (c, at) = (b.centre(), spot.centre);
            // Rounding noise is no move: arranging an arranged scene must not make an undo step.
            let snap = |d: f64| if d.abs() < 1e-9 { 0.0 } else { d };
            p.position += Vec3::new(snap(at[0] - c[0]), snap(at[1] - c[1]), if m.lifted { 0.0 } else { snap(-m.bounds.min.z) });
            placements.push((m.id, p, None));
            if !spot.on_plate {
                beside.push(m.id);
            }
        }
        self.place(&placements)?;
        Ok(beside)
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
    /// A tip at scene point `tip` with scene normal `normal` on `model`, which must exist; when
    /// `parent` is given, which must exist too, a branch sharing its body (a branch of a branch
    /// shares the same one).
    pub(crate) fn add_support(&mut self, model: u64, tip: Vec3, normal: Vec3, parent: Option<u64>) -> Result<u64> {
        let parent = match parent {
            Some(p) => Some(self.root_of(p).ok_or_else(|| EngineError::BadParams(format!("no support {p}")))?),
            None => None,
        };
        let m = self.model(model).ok_or_else(|| EngineError::BadParams(format!("no model {model}")))?;
        let inv = m.transform.inverse().ok_or_else(|| EngineError::Failed(format!("model {model} is degenerate")))?;
        let local_tip = inv.point(tip);
        let local_normal = inv.normal(normal).unwrap_or(normal);
        self.next_support_id += 1;
        self.revision += 1;
        self.supports.push(Support { id: self.next_support_id, model, tip, normal, corners: None, parent, local_tip, local_normal });
        Ok(self.next_support_id)
    }
    /// Put support `id`'s tip at scene point `tip` with scene normal `normal` on `model`, which
    /// may be another model than its own. Its placed corners and its branches stay.
    pub(crate) fn set_tip(&mut self, id: u64, model: u64, tip: Vec3, normal: Vec3) -> Result<()> {
        let m = self.model(model).ok_or_else(|| EngineError::BadParams(format!("no model {model}")))?;
        let inv = m.transform.inverse().ok_or_else(|| EngineError::Failed(format!("model {model} is degenerate")))?;
        let local_tip = inv.point(tip);
        let local_normal = inv.normal(normal).unwrap_or(normal);
        let s = self.supports.iter_mut().find(|s| s.id == id).ok_or_else(|| EngineError::BadParams(format!("no support {id}")))?;
        // Putting it where it is must not bump: it would make an empty undo step.
        if (s.model, s.tip, s.normal) == (model, tip, normal) {
            return Ok(());
        }
        (s.model, s.tip, s.normal, s.local_tip, s.local_normal) = (model, tip, normal, local_tip, local_normal);
        // A branch's tip leaves the body as it was; a support's own tip moves its head.
        if s.parent.is_none() {
            self.drop_braces_of(&[id]);
        }
        self.revision += 1;
        Ok(())
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
        let mut edited = Vec::new();
        for (id, c) in corners {
            if let Some(s) = self.supports.iter_mut().find(|s| s.id == id)
                && s.corners != c
            {
                s.corners = c;
                edited.push(id);
            }
        }
        self.drop_braces_of(&edited);
        Ok(())
    }
    /// Remove these models and their supports, as [`Scene::remove_supports`]; whether any model went.
    pub(crate) fn remove_models(&mut self, gone: impl Fn(u64) -> bool, preset: &SupportPreset) -> bool {
        let before = self.models.len();
        self.drop_supports(|s| gone(s.model), preset);
        self.models.retain(|m| !gone(m.id));
        self.prune_selection();
        // Removing nothing must not bump: it would make an empty undo step.
        let removed = self.models.len() != before;
        if removed {
            self.revision += 1;
        }
        removed
    }
    /// Remove these tips. A body goes with its last tip: when a support goes and some of its
    /// branches stay, the first of them takes its body, as placed corners under `preset`, and the
    /// others branch from it.
    pub(crate) fn remove_supports(&mut self, ids: &[u64], preset: &SupportPreset) {
        let before = self.supports.len();
        self.drop_supports(|s| ids.contains(&s.id), preset);
        self.prune_selection();
        if self.supports.len() != before {
            self.revision += 1;
        }
    }
    /// Make `pairs` the braces among supports `among`: every brace joining two of them goes, and
    /// each pair is braced with its settings; braces to other supports stay. Returns the new
    /// braces' ids in the order given, and how many braces went. Asking for the braces already
    /// there keeps them and makes no change. All or nothing, as [`Scene::add_braces`].
    pub(crate) fn rebuild_braces(&mut self, among: &BTreeSet<u64>, pairs: &[(u64, u64, BraceParams)]) -> Result<(Vec<u64>, usize)> {
        let inside = |b: &Brace| among.contains(&b.a) && among.contains(&b.b);
        let mut old: Vec<(u64, u64, BraceParams)> = self.braces.iter().filter(|b| inside(b)).map(|b| (b.a, b.b, b.params)).collect();
        let mut new: Vec<(u64, u64, BraceParams)> = pairs.iter().map(|&(a, b, p)| (a.min(b), a.max(b), p)).collect();
        old.sort_by_key(|&(a, b, _)| (a, b));
        new.sort_by_key(|&(a, b, _)| (a, b));
        // The same braces again must not bump: it would make an empty undo step.
        if old == new {
            return self.add_braces(pairs).map(|ids| (ids, 0));
        }
        let (kept, gone): (Vec<Brace>, Vec<Brace>) = self.braces.iter().partition(|b| !inside(b));
        let before = std::mem::replace(&mut self.braces, kept);
        match self.add_braces(pairs) {
            Ok(ids) => {
                self.prune_selection();
                self.revision += 1;
                Ok((ids, gone.len()))
            }
            Err(e) => {
                self.braces = before;
                Err(e)
            }
        }
    }
    /// Brace each pair of supports `(a, b)` with its settings, replacing a brace the pair already
    /// has. Returns the braces' ids in the order given; a pair braced already as asked keeps its
    /// brace and makes no change. All or nothing: a support that is unknown or a branch, a pair
    /// of one support, or settings out of range change nothing.
    pub(crate) fn add_braces(&mut self, pairs: &[(u64, u64, BraceParams)]) -> Result<Vec<u64>> {
        for (a, b, params) in pairs {
            for id in [a, b] {
                if !self.supports.iter().any(|s| s.id == *id && s.parent.is_none()) {
                    return Err(EngineError::BadParams(format!("support {id} is not a support with a body")));
                }
            }
            if a == b {
                return Err(EngineError::BadParams(format!("support {a} cannot be braced to itself")));
            }
            params.validate().map_err(EngineError::BadParams)?;
        }
        let mut ids = Vec::with_capacity(pairs.len());
        let mut changed = false;
        for &(a, b, params) in pairs {
            let (a, b) = (a.min(b), a.max(b));
            if let Some(old) = self.braces.iter().find(|x| (x.a, x.b) == (a, b)) {
                if old.params == params {
                    ids.push(old.id);
                    continue;
                }
                let gone = old.id;
                self.braces.retain(|x| x.id != gone);
            }
            self.next_brace_id += 1;
            self.braces.push(Brace { id: self.next_brace_id, a, b, params });
            ids.push(self.next_brace_id);
            changed = true;
        }
        if changed {
            self.prune_selection();
            self.revision += 1;
        }
        Ok(ids)
    }
    /// Remove these braces; how many went.
    pub(crate) fn remove_braces(&mut self, ids: &[u64]) -> usize {
        let before = self.braces.len();
        self.braces.retain(|b| !ids.contains(&b.id));
        let removed = before - self.braces.len();
        if removed > 0 {
            self.prune_selection();
            self.revision += 1;
        }
        removed
    }
    /// Remove the braces of supports `ids`, without the bump: the caller's change bumps.
    fn drop_braces_of(&mut self, ids: &[u64]) {
        self.braces.retain(|b| !ids.contains(&b.a) && !ids.contains(&b.b));
        let braces = &self.braces;
        self.selected_braces.retain(|id| braces.iter().any(|b| b.id == *id));
    }
    /// What a project file keeps: each model's mesh, name and placement, each support's tip and
    /// normal on its model and placed corners, and the braces. Not the lift (a model setting), ids
    /// or the selection.
    pub fn project(&self) -> Project {
        self.project_of(|_| true)
    }
    /// [`Scene::project`] for the models `keep` takes, and their supports.
    pub(crate) fn project_of(&self, keep: impl Fn(u64) -> bool) -> Project {
        let kept: Vec<&Model> = self.models.iter().filter(|m| keep(m.id)).collect();
        let models = kept.iter().map(|m| ProjectModel { name: m.name.clone(), mesh: m.mesh.clone(), placement: m.placement }).collect();
        // A branch goes with the support it joins: a parent always comes before its branches, so
        // it is already kept, or left out, by the time they come.
        let mut kept_supports: Vec<u64> = Vec::new();
        let mut supports = Vec::new();
        for s in &self.supports {
            let Some(model) = kept.iter().position(|m| m.id == s.model) else { continue };
            let parent = match s.parent {
                None => None,
                Some(p) => match kept_supports.iter().position(|k| *k == p) {
                    Some(i) => Some(i),
                    None => continue,
                },
            };
            kept_supports.push(s.id);
            supports.push(ProjectSupport { model, tip: s.local_tip, normal: s.local_normal, corners: s.corners.clone(), parent });
        }
        let index = |id: u64| kept_supports.iter().position(|k| *k == id);
        let braces = self
            .braces
            .iter()
            .filter_map(|b| {
                let p = b.params;
                Some(ProjectBrace {
                    a: index(b.a)?,
                    b: index(b.b)?,
                    pattern: p.pattern,
                    angle: p.angle,
                    diameter_ratio: p.diameter_ratio,
                    head_gap: p.head_gap,
                    foot_gap: p.foot_gap,
                })
            })
            .collect();
        Project { models, supports, braces }
    }
    /// Become the project `p`: everything in the scene goes, its models and
    /// supports come in with new ids, unlifted, exactly where they were saved. The lift height
    /// stays. Fails, half done, on a placement or support the scene would refuse: the caller puts
    /// the scene back, as [`Session::execute`] does.
    pub(crate) fn adopt(&mut self, p: Project) -> Result<()> {
        self.models.clear();
        self.supports.clear();
        self.braces.clear();
        self.prune_selection();
        self.revision += 1;
        self.append(p).map(|_| ())
    }
    /// Add the project `p`'s models and supports beside what is there, with new ids, unlifted,
    /// exactly where they were saved; returns the new models' ids in `p`'s order. Fails half done,
    /// as [`Scene::adopt`].
    pub(crate) fn append(&mut self, p: Project) -> Result<Vec<u64>> {
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
        let mut support_ids: Vec<u64> = Vec::with_capacity(p.supports.len());
        for ps in p.supports {
            let bad = |why: &str| EngineError::Failed(format!("a support in the file {why}"));
            // Only an earlier support: `resin_io` checks that too, but a project need not come from a file.
            let parent = match ps.parent {
                None => None,
                Some(i) => {
                    let p = support_ids.get(i).copied().ok_or_else(|| bad("branches from no support before it"))?;
                    Some(self.root_of(p).ok_or_else(|| bad("branches from no support before it"))?)
                }
            };
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
            self.supports.push(Support {
                id: self.next_support_id,
                model,
                tip,
                normal,
                corners: ps.corners,
                parent,
                local_tip: ps.tip,
                local_normal,
            });
            support_ids.push(self.next_support_id);
        }
        let mut braces = Vec::with_capacity(p.braces.len());
        for pb in p.braces {
            let bad = || EngineError::Failed("a brace in the file joins no two supports".into());
            let params =
                BraceParams { pattern: pb.pattern, angle: pb.angle, diameter_ratio: pb.diameter_ratio, head_gap: pb.head_gap, foot_gap: pb.foot_gap };
            params.validate().map_err(|e| EngineError::Failed(format!("a brace in the file: {e}")))?;
            braces.push((*support_ids.get(pb.a).ok_or_else(bad)?, *support_ids.get(pb.b).ok_or_else(bad)?, params));
        }
        self.add_braces(&braces).map_err(|e| EngineError::Failed(format!("a brace in the file: {e}")))?;
        Ok(ids)
    }
    /// Become `older` (an undo or redo step). Ids and the revision only ever grow: a model id is
    /// never reused for another mesh (the GPU keys meshes on it), and a cache keyed on the
    /// revision never mistakes the restored scene for one it has seen.
    pub(crate) fn restore(&mut self, mut older: Scene) -> Scene {
        older.next_id = older.next_id.max(self.next_id);
        older.next_support_id = older.next_support_id.max(self.next_support_id);
        older.next_brace_id = older.next_brace_id.max(self.next_brace_id);
        older.revision = older.revision.max(self.revision) + 1;
        std::mem::replace(self, older)
    }
    /// The support whose body `id` shares: itself unless it is a branch. `None` when `id` is unknown.
    pub fn root_of(&self, id: u64) -> Option<u64> {
        let s = self.supports.iter().find(|s| s.id == id)?;
        match s.parent {
            None => Some(s.id),
            Some(p) => self.supports.iter().find(|t| t.id == p && t.parent.is_none()).map(|t| t.id),
        }
    }
    /// [`Scene::remove_supports`] for the supports `gone` takes, without the bump.
    fn drop_supports(&mut self, gone: impl Fn(&Support) -> bool, preset: &SupportPreset) {
        let mut heirs: Vec<(u64, u64, Vec<Vec3>)> = Vec::new();
        for root in self.supports.iter().filter(|s| s.parent.is_none() && gone(s)) {
            // The first in order: it comes before the other branches, as a parent must.
            if let Some(heir) = self.supports.iter().find(|b| b.parent == Some(root.id) && !gone(b)) {
                heirs.push((root.id, heir.id, crate::route::placed_corners(self, root, preset)));
            }
        }
        for (root, heir, corners) in heirs {
            for s in &mut self.supports {
                if s.id == heir {
                    s.parent = None;
                    s.corners = Some(corners.clone());
                } else if s.parent == Some(root) {
                    s.parent = Some(heir);
                }
            }
        }
        let gone_ids: Vec<u64> = self.supports.iter().filter(|s| gone(s)).map(|s| s.id).collect();
        self.drop_braces_of(&gone_ids);
        self.supports.retain(|s| !gone(s));
        // A parent always comes before its branches: one pass in order finds the orphans, which
        // only a broken scene has.
        let mut kept: BTreeSet<u64> = BTreeSet::new();
        self.supports.retain(|s| {
            let keep = s.parent.is_none_or(|p| kept.contains(&p));
            if keep {
                kept.insert(s.id);
            }
            keep
        });
        let kept = &self.supports;
        self.braces.retain(|b| kept.iter().any(|s| s.id == b.a) && kept.iter().any(|s| s.id == b.b));
    }
    fn prune_selection(&mut self) {
        let supports = &self.supports;
        self.selected.retain(|id| supports.iter().any(|s| s.id == *id));
        self.selected_parts.retain(|(id, _)| supports.iter().any(|s| s.id == *id));
        let braces = &self.braces;
        self.selected_braces.retain(|id| braces.iter().any(|b| b.id == *id));
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

/// What is open: the models and the view onto them, the support and print settings, and the undo
/// history.
#[derive(Clone, Debug, Default)]
pub struct Session {
    pub scene: Scene,
    pub camera: Camera,
    /// Outside `scene` on purpose: a settings change is never an undo step, and undoing a support
    /// edit never changes a setting. Changed only through validated commands or the setter.
    pub(crate) support_settings: SupportSettings,
    /// The printer profile and print settings: outside `scene` for the same reason.
    pub(crate) print_config: PrintConfig,
    pub(crate) history: History,
    /// The scene's file and the scene revision last saved to or opened from it.
    pub(crate) file: Option<(String, u64)>,
    /// The last slice kept: derived, never an undo step.
    pub(crate) job: Option<crate::Job>,
    /// What `edit.copy` copied, for `edit.paste`: the models themselves rather than ids, so a
    /// copied model deleted since still pastes. Outside `scene`: copying is no undo step, and
    /// undoing never changes what Paste pastes.
    pub(crate) clipboard: Option<Clipboard>,
}

/// Copied models and their supports, and which of the models are lifted.
#[derive(Clone, Debug)]
pub(crate) struct Clipboard {
    pub(crate) project: Project,
    pub(crate) lifted: Vec<bool>,
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
        let print = self.print_config.clone();
        let restore = |s: &mut Session| {
            s.scene = scene.clone();
            s.camera = camera;
            s.support_settings = settings;
            s.print_config = print.clone();
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

    /// Whether `edit.paste` has something to paste.
    pub fn can_paste(&self) -> bool {
        self.clipboard.is_some()
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

    pub fn print_config(&self) -> &PrintConfig {
        &self.print_config
    }

    /// Replace the print configuration (the stored one, at start), if it passes
    /// [`PrintConfig::validate`]. No undo step, as for `print.configure`.
    pub fn set_print_config(&mut self, c: PrintConfig) -> Result<()> {
        c.validate()?;
        self.print_config = c;
        Ok(())
    }
}
