//! The supports' routes and the solids they are drawn with, kept between frames. Routes are
//! derived from the scene and the preset in use. They are worked out again when either changes,
//! a bounded amount of search per frame: a gizmo drag changes the scene every frame, and
//! re-routing hundreds of supports in one frame would stall the window. Routes not yet redone
//! are drawn where they were, the oldest redone first. Braces' struts are worked out once every
//! route is, within the same budget. Each model's raft under them and its label on it are redone
//! once every route is, off the window thread: the old ones are drawn until the new ones are in.
//!
//! Everything is built for every model, shown or not, and tagged with its model: hiding a model
//! only leaves its runs out of the draw ([`Run::shown`]), so nothing is routed or built again.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::ops::Range;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, TryRecvError};

use resin_engine::geom::{Aabb3, Label, Mesh, MeshIndex, ShadedVertex, Vec3, frustum, ray_capsule, ray_sphere, smooth_shaded};
use resin_engine::{
    Owner, Primitive, Raft, Route, Scene, SupportPart, SupportPreset, brace_solids, label_sites, label_triangles, raft_feet, raft_triangles, route,
    strut_primitives, support_primitives,
};

/// How far round a joint a press picks it, as a multiple of the body's radius: the joint is drawn
/// no wider than the body, and a press must not need to land on its sliver beside the columns.
const JOINT_PICK_SCALE: f64 = 1.5;

/// The raft's faces shade smooth within this angle of each other: its edges are at least 30°
/// from its flat faces (`edge_angle`'s range), and its walls bend far less from cell to cell.
const RAFT_CREASE_DEGREES: f64 = 25.0;

/// Routing time per frame, at most (one route past it finishes). A clearance test costs from a
/// fraction of a microsecond to a few, with the mesh around it, so time is what is capped.
#[cfg(not(target_arch = "wasm32"))]
const BUDGET: std::time::Duration = std::time::Duration::from_millis(8);
/// The browser has no clock here: clearance tests per frame instead.
#[cfg(target_arch = "wasm32")]
const BUDGET: u32 = 5_000;

/// What this frame may still spend on routing.
struct Budget {
    #[cfg(not(target_arch = "wasm32"))]
    start: std::time::Instant,
    #[cfg(target_arch = "wasm32")]
    tests: u32,
}

impl Budget {
    fn new() -> Budget {
        #[cfg(not(target_arch = "wasm32"))]
        return Budget { start: std::time::Instant::now() };
        #[cfg(target_arch = "wasm32")]
        return Budget { tests: 0 };
    }
    fn spend(&mut self, _tests: u32) {
        #[cfg(target_arch = "wasm32")]
        {
            self.tests = self.tests.saturating_add(_tests.max(1));
        }
    }
    fn left(&self) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        return self.start.elapsed() < BUDGET;
        #[cfg(target_arch = "wasm32")]
        return self.tests < BUDGET;
    }
}

/// How a support is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Look {
    Plain,
    Selected,
    /// No path reaches the plate.
    Unreachable,
}

impl Look {
    pub const ALL: [Look; 3] = [Look::Plain, Look::Selected, Look::Unreachable];
    pub fn index(self) -> usize {
        match self {
            Look::Plain => 0,
            Look::Selected => 1,
            Look::Unreachable => 2,
        }
    }
}

/// A joint's handle: a sphere the body's width, drawn over everything in Support edition while
/// it is selected.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Handle {
    pub center: Vec3,
    pub radius: f32,
    pub selected: bool,
    /// The model its support holds up.
    pub model: u64,
}

/// Vertices of the support mesh drawn alike: in one look, tips or not, and whose they are.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Run {
    pub look: Look,
    pub tip: bool,
    /// The models they belong to: twice the same but for a brace between two models' supports.
    pub models: [u64; 2],
    pub range: Range<u32>,
}

impl Run {
    /// Whether it is drawn while the models in `hidden` are hidden: a brace needs both its ends.
    pub fn shown(&self, hidden: &BTreeSet<u64>) -> bool {
        !self.models.iter().any(|m| hidden.contains(m))
    }
}

/// Every support's solids and every model's raft, hidden models' too: triangles grouped by look,
/// the round joints that fill each bend, drawn as spheres, and the joints' handles.
#[derive(Debug, Default)]
pub struct SupportMesh {
    pub vertices: Vec<ShadedVertex>,
    /// Per [`Look`], the range of `vertices` drawn in it.
    pub ranges: [Range<u32>; 3],
    /// Per [`Look`], the start of its range holding the tips: the rest, bodies, bases and braces,
    /// fades when the view looks up at them.
    pub tips: [Range<u32>; 3],
    /// `vertices` cut by look, tips or not, and model: in order, together `ranges`.
    pub runs: Vec<Run>,
    /// The round solids: centre, radius, look, whether it is a tip's contact, and its model.
    pub joints: Vec<(Vec3, f32, Look, bool, u64)>,
    pub handles: Vec<Handle>,
    pub bounds: Aabb3,
}

/// What a ray meets on a support, other than a joint's handle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Piece {
    /// The tip: the support as a whole.
    Whole,
    Segment(u32),
    /// A joint that is not selected: picked where it shows, as the columns are.
    Joint(u32),
}

/// The preset is part of it: a base or a raft setting changes the solids and no route. So is
/// the raft drawn, which arrives later.
/// Route changes, rafts drawn, the preset, what is lit and the tip left out. Not the hidden
/// models: the mesh holds theirs too.
type MeshKey = (u64, u64, SupportPreset, Lit, Option<u64>);

/// Which supports, braces and rafts a mesh draws in the selected look.
#[derive(Clone, Debug, Default, PartialEq)]
struct Lit {
    /// Models whose raft is lit.
    models: BTreeSet<u64>,
    /// Supports whose tip alone is lit: a body may be shared with other tips.
    tips: BTreeSet<u64>,
    /// Supports lit whole: tip, body and base.
    wholes: BTreeSet<u64>,
    parts: BTreeSet<(u64, SupportPart)>,
    braces: BTreeSet<u64>,
}

/// What the support mesh shows as selected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Selection<'a> {
    None,
    /// The support selection: Support edition.
    Supports,
    /// These models' supports, the braces they hold and their rafts: a model with all that stands
    /// under it is one object in Object and Slice edition.
    Models(&'a BTreeSet<u64>),
}

/// What a model's raft is built from: its settings, the bases' diameter and the model's feet, as
/// bits; and its label with those feet.
type RaftKey = (Raft, u64, Vec<[u64; 2]>, Vec<(Label, Vec<[f64; 2]>)>);

/// A model's raft with its label: as drawn, and indexed for picking.
struct BuiltRaft {
    vertices: Vec<ShadedVertex>,
    mesh: Mesh,
    index: MeshIndex,
}

/// Rafts being built on their own thread: every model's key it is for, and those it builds.
struct RaftJob {
    keys: BTreeMap<u64, RaftKey>,
    done: Receiver<Vec<(u64, BuiltRaft)>>,
}

/// A brace's struts.
pub type Struts = Vec<(Vec3, Vec3)>;

#[derive(Default)]
pub struct RouteCache {
    key: Option<(u64, SupportPreset)>,
    /// Each support's route, and the generation of the key it was made under.
    routes: HashMap<u64, (Route, u64)>,
    todo: VecDeque<u64>,
    /// Each brace's struts, and the generation of the key they were made under.
    braces: HashMap<u64, (Struts, u64)>,
    brace_todo: VecDeque<u64>,
    generation: u64,
    /// Counts route changes: the solids are stale once it moves.
    changed: u64,
    mesh: Option<(MeshKey, Arc<SupportMesh>)>,
    /// Each model's raft drawn, and what it was built from: hidden models' too.
    raft: BTreeMap<u64, (RaftKey, Arc<BuiltRaft>)>,
    /// Counts rafts drawn: the solids are stale once it moves.
    rafts: u64,
    raft_job: Option<RaftJob>,
    /// The generation and route changes the drawn or building raft's feet were gathered at.
    raft_checked: Option<(u64, u64)>,
}

impl RouteCache {
    /// Bring the routes towards `scene` under `preset`, within this frame's budget. Whether some
    /// are still out of date.
    pub fn update(&mut self, scene: &Scene, preset: &SupportPreset) -> bool {
        // Not the revision: showing or hiding a model moves no route.
        let key = (scene.shape_revision(), *preset);
        if self.key != Some(key) {
            self.key = Some(key);
            self.generation += 1;
            let supports = scene.supports();
            let before = self.routes.len();
            self.routes.retain(|id, _| supports.iter().any(|s| s.id == *id));
            if self.routes.len() != before {
                self.changed += 1;
            }
            // Placed corners first: they cost a few tests, and they are what a joint drag moves.
            // Branches too: they are never searched.
            let mut ids: Vec<(bool, u64, u64)> =
                supports.iter().map(|s| (s.corners.is_none() && s.parent.is_none(), self.routes.get(&s.id).map_or(0, |(_, g)| *g), s.id)).collect();
            ids.sort_unstable();
            self.todo = ids.into_iter().map(|(.., id)| id).collect();
            let before = self.braces.len();
            self.braces.retain(|id, _| scene.braces().iter().any(|b| b.id == *id));
            if self.braces.len() != before {
                self.changed += 1;
            }
            self.brace_todo = scene.braces().iter().map(|b| b.id).collect();
        }
        let mut budget = Budget::new();
        while budget.left() {
            let Some(id) = self.todo.pop_front() else { break };
            let Some(s) = scene.supports().iter().find(|s| s.id == id) else { continue };
            let r = route(scene, s, preset);
            budget.spend(r.tests);
            if self.routes.get(&id).is_none_or(|(old, _)| *old != r) {
                self.changed += 1;
            }
            self.routes.insert(id, (r, self.generation));
        }
        // After every route: a brace runs between two of them.
        while self.todo.is_empty() && budget.left() {
            let Some(id) = self.brace_todo.pop_front() else { break };
            let Some(b) = scene.braces().iter().find(|b| b.id == id) else { continue };
            let (struts, tests) = match (self.route(b.a), self.route(b.b)) {
                (Some(ra), Some(rb)) => brace_solids(scene, ra, rb, &b.params, preset),
                _ => (Vec::new(), 0),
            };
            budget.spend(tests);
            if self.braces.get(&id).is_none_or(|(old, _)| *old != struts) {
                self.changed += 1;
            }
            self.braces.insert(id, (struts, self.generation));
        }
        self.poll_raft(false);
        // Not while routes are being redone, so a drag does not rebuild it for every route it moves.
        if self.todo.is_empty() {
            self.refresh_raft(scene, preset);
        }
        !self.todo.is_empty() || !self.brace_todo.is_empty()
    }

    /// Whether a raft is being built: it is drawn once in.
    pub fn raft_pending(&self) -> bool {
        self.raft_job.is_some()
    }

    /// Wait for the raft being built, if any, so it is drawn: for a still image.
    pub fn wait_raft(&mut self) {
        self.poll_raft(true);
    }

    /// Each model's raft key and feet: of its supports that reach the plate, shown or not. None
    /// for a model with no foot, or without a raft.
    fn raft_inputs(&self, scene: &Scene, preset: &SupportPreset) -> BTreeMap<u64, (RaftKey, Vec<Vec3>)> {
        if !preset.raft.enabled {
            return BTreeMap::new();
        }
        let grounded = self.grounded(scene);
        let mut held: BTreeMap<u64, Vec<&Route>> = BTreeMap::new();
        for s in scene.supports().iter().filter(|s| grounded.contains(&s.id)) {
            if let Some((r, _)) = self.routes.get(&s.id) {
                held.entry(s.model).or_default().push(r);
            }
        }
        held.into_iter()
            .filter_map(|(m, routes)| {
                let feet = raft_feet(routes.iter().copied());
                if feet.is_empty() {
                    return None;
                }
                let sites = label_sites(scene, routes.iter().map(|r| (m, *r)));
                let bits = feet.iter().map(|f| [f.x.to_bits(), f.y.to_bits()]).collect();
                Some((m, ((preset.raft, preset.base.diameter.to_bits(), bits, sites), feet)))
            })
            .collect()
    }

    /// Start building the rafts of the models whose feet, label or raft settings changed, and
    /// their labels. One job at a time: while one runs, the feet are looked at again once it is
    /// in, so a drag builds the latest rafts rather than one per frame.
    fn refresh_raft(&mut self, scene: &Scene, preset: &SupportPreset) {
        let seen = (self.generation, self.changed);
        if self.raft_checked == Some(seen) {
            return;
        }
        let inputs = self.raft_inputs(scene, preset);
        let keys: BTreeMap<u64, RaftKey> = inputs.iter().map(|(m, (k, _))| (*m, k.clone())).collect();
        if self.raft.len() == keys.len() && keys.iter().all(|(m, k)| self.raft.get(m).is_some_and(|(d, _)| d == k)) {
            // A job for other feet would bring back rafts older than these.
            self.raft_job = None;
            self.raft_checked = Some(seen);
            return;
        }
        match &self.raft_job {
            Some(job) if job.keys == keys => {
                self.raft_checked = Some(seen);
                return;
            }
            Some(_) => return,
            None => {}
        }
        self.raft_checked = Some(seen);
        // A model left without a raft loses it at once; the others' are drawn until theirs are in.
        let before = self.raft.len();
        self.raft.retain(|m, _| keys.contains_key(m));
        if self.raft.len() != before {
            self.rafts += 1;
        }
        let todo: Vec<(u64, Vec<Vec3>, Vec<(Label, Vec<[f64; 2]>)>)> =
            inputs.into_iter().filter(|(m, (k, _))| self.raft.get(m).is_none_or(|(d, _)| d != k)).map(|(m, (k, feet))| (m, feet, k.3)).collect();
        if todo.is_empty() {
            return;
        }
        let (tx, done) = mpsc::channel();
        let preset = *preset;
        // Its walls are cut in many small faces: flat, each would shade apart from its neighbours.
        let job = move || {
            let built = todo
                .into_iter()
                .map(|(m, feet, sites)| {
                    let mut solid = raft_triangles(&feet, &sites, &preset);
                    solid.extend(label_triangles(&sites, &preset));
                    let vertices = smooth_shaded(&solid, RAFT_CREASE_DEGREES);
                    // Indexed here, so a press never waits for it.
                    let mesh = Mesh::new(solid);
                    let index = MeshIndex::new(&mesh);
                    (m, BuiltRaft { vertices, mesh, index })
                })
                .collect();
            // The receiver is gone when newer rafts replaced these: nobody wants them.
            let _ = tx.send(built);
        };
        #[cfg(not(target_arch = "wasm32"))]
        if let Err(e) = std::thread::Builder::new().name("raft".into()).spawn(job) {
            log::error!("could not start the raft thread: {e}");
        }
        #[cfg(target_arch = "wasm32")]
        job();
        self.raft_job = Some(RaftJob { keys, done });
    }

    /// Draw the rafts being built once they are in, waiting for them when `wait`.
    fn poll_raft(&mut self, wait: bool) {
        let Some(job) = self.raft_job.take() else { return };
        let got = if wait { job.done.recv().map_err(|_| TryRecvError::Disconnected) } else { job.done.try_recv() };
        match got {
            Ok(built) => {
                self.raft.retain(|m, _| job.keys.contains_key(m));
                for (m, raft) in built {
                    if let Some(key) = job.keys.get(&m) {
                        self.raft.insert(m, (key.clone(), Arc::new(raft)));
                    }
                }
                self.rafts += 1;
                // They may be for feet older than the latest: look at them again.
                self.raft_checked = None;
            }
            Err(TryRecvError::Empty) => self.raft_job = Some(job),
            // Tried again on the next change, not every frame.
            Err(TryRecvError::Disconnected) => log::error!("the raft could not be built"),
        }
    }

    /// Brace `id`'s struts, as last worked out.
    pub fn struts(&self, id: u64) -> Option<&Struts> {
        self.braces.get(&id).map(|(s, _)| s)
    }

    /// How many supports of shown models have no path to the plate, as last routed: a branch has
    /// none when its parent has none.
    pub fn unreachable(&self, scene: &Scene) -> usize {
        let grounded = self.grounded(scene);
        scene.supports().iter().filter(|s| scene.shows(s.model) && self.routes.contains_key(&s.id) && !grounded.contains(&s.id)).count()
    }

    /// The supports whose route reaches the plate, a branch's through its parents; and those not
    /// yet routed, which are not known to fail.
    fn grounded(&self, scene: &Scene) -> BTreeSet<u64> {
        let mut out = BTreeSet::new();
        // A parent comes before its branches.
        for s in scene.supports() {
            let own = self.routes.get(&s.id).is_none_or(|(r, _)| r.reachable());
            if own && s.parent.is_none_or(|p| out.contains(&p)) {
                out.insert(s.id);
            }
        }
        out
    }

    /// Support `id`'s route, as last worked out.
    pub fn route(&self, id: u64) -> Option<&Route> {
        self.routes.get(&id).map(|(r, _)| r)
    }

    /// Moves whenever a route changes: what was picked from the routes is stale once it does.
    pub fn changes(&self) -> u64 {
        self.changed
    }

    /// The solids of every routed support and every model's raft, hidden models' too, those
    /// `selection` takes in the selected look, and support `hidden`'s tip left out, rebuilt only
    /// when a route, a raft, the selection or `hidden` changed.
    pub fn mesh(&mut self, scene: &Scene, preset: &SupportPreset, selection: Selection<'_>, hidden: Option<u64>) -> Arc<SupportMesh> {
        let lit = match selection {
            Selection::None => Lit::default(),
            Selection::Supports => {
                Lit { tips: scene.selected.clone(), parts: scene.selected_parts.clone(), braces: scene.selected_braces.clone(), ..Lit::default() }
            }
            Selection::Models(models) => {
                let wholes: BTreeSet<u64> = scene.supports().iter().filter(|s| models.contains(&s.model)).map(|s| s.id).collect();
                let braces = scene.braces().iter().filter(|b| wholes.contains(&b.a) || wholes.contains(&b.b)).map(|b| b.id).collect();
                Lit { models: models.clone(), wholes, braces, ..Lit::default() }
            }
        };
        let key = (self.changed, self.rafts, *preset, lit, hidden);
        if let Some((k, mesh)) = &self.mesh
            && *k == key
        {
            return Arc::clone(mesh);
        }
        let lit = &key.3;
        // By the models they belong to, so a hidden model's are drawn apart from the others.
        let mut groups: BTreeMap<[u64; 2], [Group; 3]> = BTreeMap::new();
        let mut handles = Vec::new();
        let grounded = self.grounded(scene);
        for s in scene.supports() {
            let Some((r, _)) = self.routes.get(&s.id) else { continue };
            let whole = if lit.wholes.contains(&s.id) {
                Look::Selected
            } else if grounded.contains(&s.id) && !r.fault {
                Look::Plain
            } else {
                Look::Unreachable
            };
            let tip = if hidden == Some(s.id) {
                None
            } else if lit.tips.contains(&s.id) {
                Some(Look::Selected)
            } else {
                Some(whole)
            };
            let look = |part: SupportPart| if lit.parts.contains(&(s.id, part)) { Look::Selected } else { whole };
            solids(r, preset, tip, whole, look, groups.entry([s.model; 2]).or_default());
            let radius = (preset.body.diameter / 2.0) as f32;
            for (i, c) in joints(r) {
                handles.push(Handle { center: c, radius, selected: look(SupportPart::Joint(i)) == Look::Selected, model: s.model });
            }
        }
        let model_of = |id: u64| scene.supports().iter().find(|s| s.id == id).map(|s| s.model);
        for b in scene.braces() {
            let (Some((struts, _)), Some(ma), Some(mb)) = (self.braces.get(&b.id), model_of(b.a), model_of(b.b)) else { continue };
            let look = if lit.braces.contains(&b.id) { Look::Selected } else { Look::Plain };
            let Some(g) = groups.entry([ma.min(mb), ma.max(mb)]).or_default().get_mut(look.index()) else { continue };
            strut_primitives(struts, b.params.diameter_ratio, preset, |prim| {
                if let Primitive::Frustum { a, b, ra, rb, sides, smooth, caps } = prim {
                    frustum(a, b, ra, rb, sides, smooth, caps, &mut g.body);
                }
            });
        }
        for (m, (_, raft)) in &self.raft {
            let look = if lit.models.contains(m) { Look::Selected } else { Look::Plain };
            if let Some(g) = groups.entry([*m; 2]).or_default().get_mut(look.index()) {
                g.body.extend(raft.vertices.iter());
            }
        }
        let mut out = SupportMesh { handles, ..Default::default() };
        for look in Look::ALL {
            let start = out.vertices.len() as u32;
            let mut tips_end = start;
            for tip in [true, false] {
                for (models, owned) in &mut groups {
                    let Some(g) = owned.get_mut(look.index()) else { continue };
                    let from = out.vertices.len() as u32;
                    out.vertices.append(if tip { &mut g.tip } else { &mut g.body });
                    let to = out.vertices.len() as u32;
                    if to > from {
                        out.runs.push(Run { look, tip, models: *models, range: from..to });
                    }
                }
                if tip {
                    tips_end = out.vertices.len() as u32;
                }
            }
            if let (Some(range), Some(tips)) = (out.ranges.get_mut(look.index()), out.tips.get_mut(look.index())) {
                *range = start..out.vertices.len() as u32;
                *tips = start..tips_end;
            }
            for (models, owned) in &mut groups {
                if let Some(g) = owned.get_mut(look.index()) {
                    out.joints.extend(g.joints.drain(..).map(|(c, r, l, t)| (c, r, l, t, models[0])));
                }
            }
        }
        out.bounds = out.vertices.iter().map(|v| Vec3::from(v.pos)).collect();
        let mesh = Arc::new(out);
        self.mesh = Some((key, Arc::clone(&mesh)));
        mesh
    }

    /// The nearest selected joint along the ray from `origin` along unit `dir`: its support, the
    /// joint and the distance. Their handles are drawn over everything, so nothing hides one.
    pub fn joint_hit(&self, scene: &Scene, preset: &SupportPreset, origin: Vec3, dir: Vec3) -> Option<(u64, u32, f64)> {
        let radius = preset.body.diameter / 2.0 * JOINT_PICK_SCALE;
        self.routed(scene)
            .flat_map(|(id, r)| {
                joints(r)
                    .filter(move |(i, _)| scene.selected_parts.contains(&(id, SupportPart::Joint(*i))))
                    .filter_map(move |(i, c)| ray_sphere(origin, dir, c, radius).map(|t| (id, i, t)))
            })
            .min_by(|a, b| a.2.total_cmp(&b.2))
    }

    /// The nearest support solid along the ray: its support, the piece and the distance. The
    /// tip stands for the whole support, the base for the segment above it. Without `bodies`,
    /// the tips alone.
    pub fn piece_hit(&self, scene: &Scene, preset: &SupportPreset, origin: Vec3, dir: Vec3, bodies: bool) -> Option<(u64, Piece, f64)> {
        let body = preset.body.diameter / 2.0;
        let tip = preset.tip.upper_diameter.max(preset.tip.contact_diameter) / 2.0;
        let base = preset.base.diameter / 2.0;
        self.routed(scene)
            .flat_map(|(id, r)| {
                let foot = r.body.last().filter(|_| r.foot && bodies).map(|&f| (f + Vec3::Z * preset.foot_z(), f + Vec3::Z * preset.base_top()));
                let bottom = bottom_segment(r).map_or(Piece::Whole, Piece::Segment);
                let whole = std::iter::once((r.contact, r.head, tip, Piece::Whole)).chain(foot.map(|(a, b)| (a, b, base, bottom)));
                let whole = whole.filter_map(move |(a, b, rad, piece)| ray_capsule(origin, dir, a, b, rad).map(|t| (id, piece, t)));
                let segments = r.body.windows(2).zip(0u32..).filter(move |_| bodies).filter_map(move |(w, i)| match w {
                    [a, b] => ray_capsule(origin, dir, *a, *b, body).map(|t| (id, Piece::Segment(i), t)),
                    _ => None,
                });
                // First, so a joint wins a tie with the column ending in it.
                let joints = joints(r)
                    .filter(move |_| bodies)
                    .filter_map(move |(i, c)| ray_sphere(origin, dir, c, body * JOINT_PICK_SCALE).map(|t| (id, Piece::Joint(i), t)));
                joints.chain(whole).chain(segments)
            })
            .min_by(|a, b| a.2.total_cmp(&b.2))
    }

    /// The nearest raft of a shown model along the ray from `origin` along unit `dir`: its model
    /// and the distance. The rafts drawn: the old ones while new ones are built.
    pub fn raft_hit(&self, scene: &Scene, origin: Vec3, dir: Vec3) -> Option<(u64, f64)> {
        self.raft
            .iter()
            .filter(|(m, _)| scene.shows(**m))
            .filter_map(|(m, (_, r))| r.index.ray_hit(&r.mesh, origin, dir).map(|h| (*m, h.t)))
            .min_by(|a, b| a.1.total_cmp(&b.1))
    }

    /// The nearest brace strut along the ray: its brace and the distance. Picked a body's width
    /// at least, so a thin strut is no harder to hit than a column.
    pub fn brace_hit(&self, scene: &Scene, preset: &SupportPreset, origin: Vec3, dir: Vec3) -> Option<(u64, f64)> {
        self.brace_hits(scene, preset, origin, dir).min_by(|a, b| a.1.total_cmp(&b.1))
    }

    /// Every brace the ray hits, as [`Self::brace_hit`] picks them, in no order.
    pub fn brace_hits<'a>(&'a self, scene: &'a Scene, preset: &SupportPreset, origin: Vec3, dir: Vec3) -> impl Iterator<Item = (u64, f64)> + 'a {
        let body = preset.body.diameter;
        scene.braces().iter().filter(|b| scene.shows_support(b.a) && scene.shows_support(b.b)).filter_map(move |b| {
            let r = (body * b.params.diameter_ratio).max(body) / 2.0;
            let struts = &self.braces.get(&b.id)?.0;
            struts.iter().filter_map(|(p, q)| ray_capsule(origin, dir, *p, *q, r)).min_by(f64::total_cmp).map(|t| (b.id, t))
        })
    }

    /// Each support of a shown model with its route, as last worked out.
    fn routed<'a>(&'a self, scene: &'a Scene) -> impl Iterator<Item = (u64, &'a Route)> + 'a {
        scene.supports().iter().filter(|s| scene.shows(s.model)).filter_map(|s| self.routes.get(&s.id).map(|(r, _)| (s.id, r)))
    }
}

/// A route's joints, numbered as [`SupportPart::Joint`]: every corner of the body but the foot,
/// so there is always one where the tip meets the body. None when no path reaches the plate.
fn joints(r: &Route) -> impl Iterator<Item = (u32, Vec3)> + '_ {
    r.body.split_last().map(|(_, corners)| corners).unwrap_or_default().iter().zip(0u32..).map(|(&c, i)| (i, c))
}

/// The segment ending on the foot, which the base belongs to: picked and selected with it, as
/// moving that segment carries the base along. None for a route with no segment.
fn bottom_segment(r: &Route) -> Option<u32> {
    r.body.len().checked_sub(2).and_then(|i| u32::try_from(i).ok())
}

/// One look's solids while the mesh is built.
#[derive(Default)]
struct Group {
    tip: Vec<ShadedVertex>,
    body: Vec<ShadedVertex>,
    joints: Vec<(Vec3, f32, Look, bool)>,
}

/// One support's solids ([`support_primitives`]), each into the group of its look: the tip and
/// its round contact in `tip` (none: left out), the body's columns and the joint at each bend
/// in `look` of their part, the base in that of the segment above it.
fn solids(r: &Route, p: &SupportPreset, tip: Option<Look>, whole: Look, look: impl Fn(SupportPart) -> Look, groups: &mut [Group; 3]) {
    support_primitives(r, p, |owner, prim| {
        let l = match owner {
            Owner::Tip => match tip {
                Some(l) => l,
                None => return,
            },
            Owner::Base => bottom_segment(r).map_or(whole, |i| look(SupportPart::Segment(i))),
            Owner::Part(part) => look(part),
        };
        let Some(g) = groups.get_mut(l.index()) else { return };
        let is_tip = owner == Owner::Tip;
        match prim {
            Primitive::Frustum { a, b, ra, rb, sides, smooth, caps } => {
                frustum(a, b, ra, rb, sides, smooth, caps, if is_tip { &mut g.tip } else { &mut g.body })
            }
            Primitive::Sphere { center, radius } => g.joints.push((center, radius as f32, l, is_tip)),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use resin_engine::{PresetId, Session};
    use serde_json::json;

    /// A floating triangle facing down, imported where it is, with a support under it.
    fn floating(s: &mut Session, name: &str, x: f64) -> u64 {
        let path = std::env::temp_dir().join(format!("resin-ui-routes-{name}-{}.stl", std::process::id()));
        let text = "solid p\nfacet normal 0 0 0\nouter loop\nvertex 0 0 10\nvertex 0 20 10\nvertex 20 20 10\nendloop\nendfacet\nendsolid p\n";
        std::fs::write(&path, text).unwrap();
        let m = s.execute("file.import_stl", &json!({ "path": path, "fit": false })).unwrap()["id"].as_u64().unwrap();
        std::fs::remove_file(&path).ok();
        let at = s.scene.model(m).unwrap().bounds.min;
        s.execute("support.add", &json!({ "model": m, "point": [at.x + x, at.y + 10.0, at.z], "normal": [0, 0, -1] })).unwrap();
        m
    }

    /// A hidden model's supports are not drawn, picked or counted, and its raft is not drawn.
    #[test]
    fn a_hidden_models_supports_are_out_of_sight() {
        let mut s = Session::new();
        let m = floating(&mut s, "hidden", 5.0);
        let preset = PresetId::Medium.defaults();
        let mut cache = RouteCache::default();
        while cache.update(&s.scene, &preset) {}
        cache.wait_raft();
        let tip = s.scene.supports()[0].tip;
        let (o, d) = (Vec3::new(tip.x, tip.y - 50.0, 5.0), Vec3::new(0.0, 1.0, 0.0));
        assert!(cache.piece_hit(&s.scene, &preset, o, d, true).is_some());
        assert!(!cache.mesh(&s.scene, &preset, Selection::None, None).vertices.is_empty());

        s.execute("model.visibility", &json!({ "ids": [m], "visible": false })).unwrap();
        while cache.update(&s.scene, &preset) {}
        cache.wait_raft();
        assert!(cache.piece_hit(&s.scene, &preset, o, d, true).is_none());
        let mesh = cache.mesh(&s.scene, &preset, Selection::None, None);
        let hidden = s.scene.hidden_models();
        assert!(!mesh.runs.is_empty() && mesh.runs.iter().all(|r| !r.shown(&hidden)), "no support and no raft drawn");
        assert!(mesh.joints.iter().all(|j| j.4 == m) && mesh.handles.iter().all(|h| h.model == m));
        assert_eq!(cache.unreachable(&s.scene), 0);
    }

    /// Showing or hiding a model routes and builds nothing again, undone too: the same solids,
    /// each model's raft its own, and only the hidden model's runs left out.
    #[test]
    fn hiding_a_model_rebuilds_nothing() {
        let mut s = Session::new();
        let (a, b) = (floating(&mut s, "a", 5.0), floating(&mut s, "b", 15.0));
        let preset = PresetId::Medium.defaults();
        let mut cache = RouteCache::default();
        while cache.update(&s.scene, &preset) {}
        cache.wait_raft();
        assert_eq!(cache.raft.keys().copied().collect::<Vec<_>>(), vec![a, b], "a raft each");
        assert!(cache.raft.values().all(|(_, r)| !r.vertices.is_empty()));
        let mesh = cache.mesh(&s.scene, &preset, Selection::Models(&s.scene.selected_models), None);
        let generation = cache.generation;
        for (step, params) in [("model.visibility", json!({ "ids": [a], "visible": false })), ("edit.undo", json!({})), ("edit.redo", json!({}))] {
            s.execute(step, &params).unwrap();
            assert!(!cache.update(&s.scene, &preset), "{step}: nothing to route");
            assert!(cache.generation == generation && !cache.raft_pending(), "{step}: no raft built");
            assert!(Arc::ptr_eq(&mesh, &cache.mesh(&s.scene, &preset, Selection::Models(&s.scene.selected_models), None)), "{step}: the same solids");
        }
        let hidden = s.scene.hidden_models();
        assert_eq!(hidden, BTreeSet::from([a]));
        let shown: BTreeSet<[u64; 2]> = mesh.runs.iter().filter(|r| r.shown(&hidden)).map(|r| r.models).collect();
        assert_eq!(shown, BTreeSet::from([[b, b]]));
        assert!(mesh.runs.iter().any(|r| r.models == [a, a]));
        // The runs cover the vertices once, each look's tips first.
        let mut at = 0;
        for r in &mesh.runs {
            assert_eq!(r.range.start, at);
            at = r.range.end;
            let look = &mesh.ranges[r.look.index()];
            assert!(look.start <= r.range.start && r.range.end <= look.end && r.tip == (r.range.end <= mesh.tips[r.look.index()].end));
        }
        assert_eq!(at as usize, mesh.vertices.len());
    }

    /// A selected object is lit whole, its raft too, and nothing of the others; selecting
    /// supports lights no raft.
    #[test]
    fn a_selected_objects_raft_is_lit() {
        let mut s = Session::new();
        let (a, b) = (floating(&mut s, "lit-a", 5.0), floating(&mut s, "lit-b", 15.0));
        let preset = PresetId::Medium.defaults();
        let mut cache = RouteCache::default();
        while cache.update(&s.scene, &preset) {}
        cache.wait_raft();
        let raft = |m: u64| cache.raft.get(&m).map_or(0, |(_, r)| r.vertices.len());
        assert!(raft(a) > 0 && raft(b) > 0);
        let raft_a = raft(a);
        let mesh = cache.mesh(&s.scene, &preset, Selection::Models(&BTreeSet::from([a])), None);
        let runs = |models: [u64; 2]| mesh.runs.iter().filter(move |r| r.models == models);
        assert!(runs([a, a]).count() > 0 && runs([a, a]).all(|r| r.look == Look::Selected));
        assert!(runs([b, b]).count() > 0 && runs([b, b]).all(|r| r.look != Look::Selected));
        let lit_bodies: usize = mesh.runs.iter().filter(|r| r.models == [a, a] && !r.tip).map(|r| r.range.len()).sum();
        assert!(lit_bodies > raft_a, "the raft is among the lit bodies, with the support's");
        for selection in [Selection::None, Selection::Supports] {
            let mesh = cache.mesh(&s.scene, &preset, selection, None);
            assert!(mesh.runs.iter().all(|r| r.look != Look::Selected), "{selection:?}: nothing lit");
        }
    }

    /// A ray down through a raft picks its model, the nearest raft first; a hidden model's raft
    /// is not picked.
    #[test]
    fn a_raft_picks_its_model() {
        let mut s = Session::new();
        let m = floating(&mut s, "raft-hit", 5.0);
        let preset = PresetId::Medium.defaults();
        let mut cache = RouteCache::default();
        while cache.update(&s.scene, &preset) {}
        cache.wait_raft();
        let foot = cache.route(s.scene.supports()[0].id).and_then(|r| r.body.last().copied()).unwrap();
        let down = Vec3::new(0.0, 0.0, -1.0);
        // Beside the base, on the raft's margin: no support there.
        let o = Vec3::new(foot.x + preset.base.diameter / 2.0 + preset.raft.margin / 2.0, foot.y, 50.0);
        assert!(cache.piece_hit(&s.scene, &preset, o, down, true).is_none());
        let (id, t) = cache.raft_hit(&s.scene, o, down).unwrap();
        assert!(id == m && t > 50.0 - preset.raft.thickness - preset.raft.rim_height - 1e-6 && t <= 50.0, "{t}");
        assert!(cache.raft_hit(&s.scene, Vec3::new(foot.x + 100.0, foot.y, 50.0), down).is_none(), "beside the raft");

        s.execute("model.visibility", &json!({ "ids": [m], "visible": false })).unwrap();
        assert!(cache.raft_hit(&s.scene, o, down).is_none());
    }

    #[test]
    fn routes_follow_the_scene_within_the_budget() {
        let mut s = Session::new();
        let path = std::env::temp_dir().join(format!("resin-ui-routes-{}.stl", std::process::id()));
        let tri = |a: [f32; 3], b: [f32; 3], c: [f32; 3]| {
            format!(
                "facet normal 0 0 0\nouter loop\nvertex {} {} {}\nvertex {} {} {}\nvertex {} {} {}\nendloop\nendfacet\n",
                a[0], a[1], a[2], b[0], b[1], b[2], c[0], c[1], c[2]
            )
        };
        // A floating square plate, facing down.
        let text = format!(
            "solid p\n{}{}endsolid p\n",
            tri([0.0, 0.0, 10.0], [0.0, 20.0, 10.0], [20.0, 20.0, 10.0]),
            tri([0.0, 0.0, 10.0], [20.0, 20.0, 10.0], [20.0, 0.0, 10.0])
        );
        std::fs::write(&path, text).unwrap();
        let m = s.execute("file.import_stl", &json!({ "path": path })).unwrap()["id"].clone();
        for i in 0..5 {
            s.execute("support.add", &json!({ "model": m, "point": [2 + 3 * i, 5, 10], "normal": [0, 0, -1] })).unwrap();
        }
        let preset = PresetId::Medium.defaults();
        let mut cache = RouteCache::default();
        while cache.update(&s.scene, &preset) {}
        assert_eq!(cache.routes.len(), 5);
        assert_eq!(cache.unreachable(&s.scene), 0);
        let mesh = cache.mesh(&s.scene, &preset, Selection::Supports, None);
        assert!(!mesh.vertices.is_empty() && mesh.ranges[1].is_empty() && mesh.ranges[2].is_empty());
        assert!(Arc::ptr_eq(&mesh, &cache.mesh(&s.scene, &preset, Selection::Supports, None)), "nothing changed: the same solids");
        // Selecting one moves its tip alone into the selected range: its body and base stay plain,
        // and its joints get no handle.
        s.execute("support.select", &json!({ "ids": [1] })).unwrap();
        let mesh = cache.mesh(&s.scene, &preset, Selection::Supports, None);
        let mut tip = Vec::new();
        support_primitives(cache.route(1).unwrap(), &preset, |owner, prim| {
            if let (Owner::Tip, Primitive::Frustum { a, b, ra, rb, sides, smooth, caps }) = (owner, prim) {
                frustum(a, b, ra, rb, sides, smooth, caps, &mut tip);
            }
        });
        assert_eq!(mesh.ranges[1].len(), tip.len());
        assert_eq!(mesh.tips[1], mesh.ranges[1], "the selected range holds a tip alone");
        assert!(mesh.tips[0].len() < mesh.ranges[0].len() && mesh.tips[0].start == mesh.ranges[0].start, "tips first, then the bodies");
        assert!(mesh.handles.iter().all(|h| !h.selected));
        assert!(mesh.joints.iter().all(|j| j.2 != Look::Selected), "the head's joint stays plain");
        // A selected segment and joint: the segment's column and the base under it move into the
        // selected range, the joint's handle is selected, and the rest of the support stays plain.
        s.execute("support.select", &json!({ "parts": [{ "support": 3, "segment": 0 }, { "support": 3, "joint": 0 }] })).unwrap();
        let mesh = cache.mesh(&s.scene, &preset, Selection::Supports, None);
        let mut column = Vec::new();
        support_primitives(cache.route(3).unwrap(), &preset, |owner, prim| {
            if let (Owner::Part(SupportPart::Segment(0)) | Owner::Base, Primitive::Frustum { a, b, ra, rb, sides, smooth, caps }) = (owner, prim) {
                frustum(a, b, ra, rb, sides, smooth, caps, &mut column);
            }
        });
        assert_eq!(mesh.ranges[1].len(), column.len(), "the bottom segment and its base");
        assert!(mesh.tips[1].is_empty());
        assert_eq!(mesh.handles.len(), 5, "one joint per straight support, where the tip meets the body");
        assert_eq!(mesh.handles.iter().filter(|h| h.selected).count(), 1);
        // A ray down onto support 3's head meets its handle; one from the side meets its body.
        let head = mesh.handles[2].center;
        assert_eq!(cache.joint_hit(&s.scene, &preset, head + Vec3::Z * 50.0, -Vec3::Z).map(|h| (h.0, h.1)), Some((3, 0)));
        // A joint not selected has no handle over everything, but shows, and is picked, where
        // it is.
        let other = mesh.handles[1].center;
        assert_eq!(cache.joint_hit(&s.scene, &preset, other + Vec3::Z * 50.0, -Vec3::Z), None);
        assert_eq!(cache.piece_hit(&s.scene, &preset, other - Vec3::Y * 50.0, Vec3::Y, true).map(|h| (h.0, h.1)), Some((2, Piece::Joint(0))));
        let side = Vec3::new(head.x, head.y - 50.0, head.z / 2.0);
        assert_eq!(cache.piece_hit(&s.scene, &preset, side, Vec3::Y, true).map(|h| (h.0, h.1)), Some((3, Piece::Segment(0))));
        // The base is picked as the segment above it.
        assert_eq!(
            cache.piece_hit(&s.scene, &preset, Vec3::new(head.x, -50.0, 0.1), Vec3::Y, true).map(|h| (h.0, h.1)),
            Some((3, Piece::Segment(0)))
        );
        // Without the bodies, neither the column nor the base is met; the tip still is.
        assert_eq!(cache.piece_hit(&s.scene, &preset, side, Vec3::Y, false), None);
        assert_eq!(cache.piece_hit(&s.scene, &preset, Vec3::new(head.x, -50.0, 0.1), Vec3::Y, false), None);
        let contact = cache.route(3).unwrap().contact;
        assert_eq!(cache.piece_hit(&s.scene, &preset, contact - Vec3::Z * 50.0, Vec3::Z, false).map(|h| (h.0, h.1)), Some((3, Piece::Whole)));
        // A brace between supports 4 and 5: its struts are drawn, picked as the brace and selected in their look.
        let brace = s.execute("support.add_braces", &json!({ "ids": [4, 5], "angle": 15 })).unwrap()["ids"][0].as_u64().unwrap();
        while cache.update(&s.scene, &preset) {}
        let (p, q) = cache.struts(brace).unwrap()[0];
        let mid = (p + q) / 2.0;
        assert_eq!(cache.brace_hit(&s.scene, &preset, mid - Vec3::Y * 50.0, Vec3::Y).map(|h| h.0), Some(brace));
        s.execute("support.select", &json!({ "braces": [brace] })).unwrap();
        let mesh = cache.mesh(&s.scene, &preset, Selection::Supports, None);
        let mut struts = Vec::new();
        strut_primitives(cache.struts(brace).unwrap(), 0.5, &preset, |prim| {
            if let Primitive::Frustum { a, b, ra, rb, sides, smooth, caps } = prim {
                frustum(a, b, ra, rb, sides, smooth, caps, &mut struts);
            }
        });
        assert_eq!(mesh.ranges[1].len(), struts.len(), "the brace alone is selected");
        // A deleted support's route goes, and its brace with it.
        s.execute("support.delete", &json!({ "ids": [2, 4] })).unwrap();
        while cache.update(&s.scene, &preset) {}
        assert_eq!(cache.routes.len(), 3);
        assert!(cache.struts(brace).is_none());
        // The raft is built off the window thread: the old one is drawn until the new one is in.
        cache.wait_raft();
        let drawn = |cache: &RouteCache| cache.raft.values().next().map(|(k, r)| (k.0, r.vertices.len()));
        let before = drawn(&cache).unwrap();
        assert!(before.1 > 0 && !cache.raft_pending());
        let mut wider = preset;
        wider.raft.margin += 2.0;
        while cache.update(&s.scene, &wider) {}
        assert!(cache.raft_pending());
        let mesh = cache.mesh(&s.scene, &wider, Selection::Supports, None);
        assert_eq!(drawn(&cache), Some(before), "the old raft meanwhile");
        cache.wait_raft();
        assert!(!cache.raft_pending() && drawn(&cache).unwrap().0 == wider.raft);
        assert!(!Arc::ptr_eq(&mesh, &cache.mesh(&s.scene, &wider, Selection::Supports, None)), "the new raft redraws the solids");
        // Nothing changed: no new job.
        cache.update(&s.scene, &wider);
        assert!(!cache.raft_pending());
        // Turned off, it goes at once.
        wider.raft.enabled = false;
        while cache.update(&s.scene, &wider) {}
        assert!(!cache.raft_pending() && drawn(&cache).is_none());
        std::fs::remove_file(path).ok();
    }
}
