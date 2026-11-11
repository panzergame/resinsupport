//! The supports' routes and the solids they are drawn with, kept between frames. Routes are
//! derived from the scene and the preset in use. They are worked out again when either changes,
//! a bounded amount of search per frame: a gizmo drag changes the scene every frame, and
//! re-routing hundreds of supports in one frame would stall the window. Routes not yet redone
//! are drawn where they were, the oldest redone first.

use std::collections::{BTreeSet, HashMap, VecDeque};
use std::sync::Arc;

use resin_engine::geom::{Aabb3, ShadedVertex, Vec3, frustum, ray_capsule, ray_sphere};
use resin_engine::{Primitive, Route, Scene, SupportPart, SupportPreset, route, support_primitives};

/// How far round a joint a press picks it, as a multiple of the body's radius: the joint is drawn
/// no wider than the body, and a press must not need to land on its sliver beside the columns.
const JOINT_PICK_SCALE: f64 = 1.5;

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
}

/// Every support's solids: triangles grouped by look, the round joints that fill each bend,
/// drawn as spheres, and the joints' handles.
#[derive(Debug, Default)]
pub struct SupportMesh {
    pub vertices: Vec<ShadedVertex>,
    /// Per [`Look`], the range of `vertices` drawn in it.
    pub ranges: [std::ops::Range<u32>; 3],
    pub joints: Vec<(Vec3, f32, Look)>,
    pub handles: Vec<Handle>,
    pub bounds: Aabb3,
}

/// What a ray meets on a support, other than a joint's handle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Piece {
    /// The tip or the base: the support as a whole.
    Whole,
    Segment(u32),
    /// A joint that is not selected: picked where it shows, as the columns are.
    Joint(u32),
}

type MeshKey = (u64, bool, BTreeSet<u64>, BTreeSet<(u64, SupportPart)>);

#[derive(Default)]
pub struct RouteCache {
    key: Option<(u64, SupportPreset)>,
    /// Each support's route, and the generation of the key it was made under.
    routes: HashMap<u64, (Route, u64)>,
    todo: VecDeque<u64>,
    generation: u64,
    /// Counts route changes: the solids are stale once it moves.
    changed: u64,
    mesh: Option<(MeshKey, Arc<SupportMesh>)>,
}

impl RouteCache {
    /// Bring the routes towards `scene` under `preset`, within this frame's budget. Whether some
    /// are still out of date.
    pub fn update(&mut self, scene: &Scene, preset: &SupportPreset) -> bool {
        let key = (scene.revision(), *preset);
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
            let mut ids: Vec<(bool, u64, u64)> =
                supports.iter().map(|s| (s.corners.is_none(), self.routes.get(&s.id).map_or(0, |(_, g)| *g), s.id)).collect();
            ids.sort_unstable();
            self.todo = ids.into_iter().map(|(.., id)| id).collect();
        }
        let mut budget = Budget::new();
        while budget.left() {
            let Some(id) = self.todo.pop_front() else { break };
            let Some(s) = scene.supports().iter().find(|s| s.id == id) else { continue };
            let r = route(scene.models(), s, preset);
            budget.spend(r.tests);
            if self.routes.get(&id).is_none_or(|(old, _)| *old != r) {
                self.changed += 1;
            }
            self.routes.insert(id, (r, self.generation));
        }
        !self.todo.is_empty()
    }

    /// How many supports have no path to the plate, as last routed.
    pub fn unreachable(&self) -> usize {
        self.routes.values().filter(|(r, _)| !r.reachable()).count()
    }

    /// Support `id`'s route, as last worked out.
    pub fn route(&self, id: u64) -> Option<&Route> {
        self.routes.get(&id).map(|(r, _)| r)
    }

    /// Moves whenever a route changes: what was picked from the routes is stale once it does.
    pub fn changes(&self) -> u64 {
        self.changed
    }

    /// The solids of every routed support, the selected ones in their look when `selection`,
    /// rebuilt only when a route or the selection changed.
    pub fn mesh(&mut self, scene: &Scene, preset: &SupportPreset, selection: bool) -> Arc<SupportMesh> {
        let none = (BTreeSet::new(), BTreeSet::new());
        let (selected, parts) = if selection { (&scene.selected, &scene.selected_parts) } else { (&none.0, &none.1) };
        let key = (self.changed, selection, selected.clone(), parts.clone());
        if let Some((k, mesh)) = &self.mesh
            && *k == key
        {
            return Arc::clone(mesh);
        }
        let mut groups: [SupportMesh; 3] = Default::default();
        let mut handles = Vec::new();
        for s in scene.supports() {
            let Some((r, _)) = self.routes.get(&s.id) else { continue };
            let whole = if selected.contains(&s.id) {
                Look::Selected
            } else if r.reachable() && !r.fault {
                Look::Plain
            } else {
                Look::Unreachable
            };
            let look = |part: SupportPart| if parts.contains(&(s.id, part)) { Look::Selected } else { whole };
            solids(r, preset, whole, look, &mut groups);
            let radius = (preset.body.diameter / 2.0) as f32;
            for (i, c) in joints(r) {
                handles.push(Handle { center: c, radius, selected: look(SupportPart::Joint(i)) == Look::Selected });
            }
        }
        let mut out = SupportMesh { handles, ..Default::default() };
        for (look, g) in Look::ALL.into_iter().zip(groups) {
            let start = out.vertices.len() as u32;
            out.vertices.extend(g.vertices);
            if let Some(range) = out.ranges.get_mut(look.index()) {
                *range = start..out.vertices.len() as u32;
            }
            out.joints.extend(g.joints);
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
                let selected = move |i| scene.selected.contains(&id) || scene.selected_parts.contains(&(id, SupportPart::Joint(i)));
                joints(r).filter(move |(i, _)| selected(*i)).filter_map(move |(i, c)| ray_sphere(origin, dir, c, radius).map(|t| (id, i, t)))
            })
            .min_by(|a, b| a.2.total_cmp(&b.2))
    }

    /// The nearest support solid along the ray: its support, the piece and the distance. The
    /// tip and the base stand for the whole support.
    pub fn piece_hit(&self, scene: &Scene, preset: &SupportPreset, origin: Vec3, dir: Vec3) -> Option<(u64, Piece, f64)> {
        let body = preset.body.diameter / 2.0;
        let tip = preset.tip.upper_diameter.max(preset.tip.contact_diameter) / 2.0;
        let base = preset.base.diameter / 2.0;
        self.routed(scene)
            .flat_map(|(id, r)| {
                let foot = r.body.last().map(|&f| (f, f + Vec3::Z * preset.base.height));
                let whole = std::iter::once((r.contact, r.head, tip)).chain(foot.map(|(a, b)| (a, b, base)));
                let whole = whole.filter_map(move |(a, b, rad)| ray_capsule(origin, dir, a, b, rad).map(|t| (id, Piece::Whole, t)));
                let segments = r.body.windows(2).zip(0u32..).filter_map(move |(w, i)| match w {
                    [a, b] => ray_capsule(origin, dir, *a, *b, body).map(|t| (id, Piece::Segment(i), t)),
                    _ => None,
                });
                // First, so a joint wins a tie with the column ending in it.
                let joints =
                    joints(r).filter_map(move |(i, c)| ray_sphere(origin, dir, c, body * JOINT_PICK_SCALE).map(|t| (id, Piece::Joint(i), t)));
                joints.chain(whole).chain(segments)
            })
            .min_by(|a, b| a.2.total_cmp(&b.2))
    }

    /// Each support in the scene with its route, as last worked out.
    fn routed<'a>(&'a self, scene: &'a Scene) -> impl Iterator<Item = (u64, &'a Route)> + 'a {
        scene.supports().iter().filter_map(|s| self.routes.get(&s.id).map(|(r, _)| (s.id, r)))
    }
}

/// A route's joints, numbered as [`SupportPart::Joint`]: every corner of the body but the foot,
/// so there is always one where the tip meets the body. None when no path reaches the plate.
fn joints(r: &Route) -> impl Iterator<Item = (u32, Vec3)> + '_ {
    r.body.split_last().map(|(_, corners)| corners).unwrap_or_default().iter().zip(0u32..).map(|(&c, i)| (i, c))
}

/// One support's solids ([`support_primitives`]), each into the group of its look: the tip, its
/// round contact and the base in `whole`, the body's columns and the joint at each bend in
/// `look` of their part.
fn solids(r: &Route, p: &SupportPreset, whole: Look, look: impl Fn(SupportPart) -> Look, groups: &mut [SupportMesh; 3]) {
    support_primitives(r, p, |part, prim| {
        let l = part.map_or(whole, &look);
        let Some(g) = groups.get_mut(l.index()) else { return };
        match prim {
            Primitive::Frustum { a, b, ra, rb, sides, smooth, caps } => frustum(a, b, ra, rb, sides, smooth, caps, &mut g.vertices),
            Primitive::Sphere { center, radius } => g.joints.push((center, radius as f32, l)),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use resin_engine::{PresetId, Session};
    use serde_json::json;

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
        assert_eq!(cache.unreachable(), 0);
        let mesh = cache.mesh(&s.scene, &preset, true);
        assert!(!mesh.vertices.is_empty() && mesh.ranges[1].is_empty() && mesh.ranges[2].is_empty());
        assert!(Arc::ptr_eq(&mesh, &cache.mesh(&s.scene, &preset, true)), "nothing changed: the same solids");
        // Selecting one moves its solids into the selected range.
        s.execute("support.select", &json!({ "ids": [1] })).unwrap();
        assert!(!cache.mesh(&s.scene, &preset, true).ranges[1].is_empty());
        // A selected segment and joint: the segment's column moves into the selected range, the
        // joint's handle is selected, and the rest of the support stays plain.
        s.execute("support.select", &json!({ "parts": [{ "support": 3, "segment": 0 }, { "support": 3, "joint": 0 }] })).unwrap();
        let mesh = cache.mesh(&s.scene, &preset, true);
        assert!(!mesh.ranges[0].is_empty() && !mesh.ranges[1].is_empty());
        assert_eq!(mesh.handles.len(), 5, "one joint per straight support, where the tip meets the body");
        assert_eq!(mesh.handles.iter().filter(|h| h.selected).count(), 1);
        // A ray down onto support 3's head meets its handle; one from the side meets its body.
        let head = mesh.handles[2].center;
        assert_eq!(cache.joint_hit(&s.scene, &preset, head + Vec3::Z * 50.0, -Vec3::Z).map(|h| (h.0, h.1)), Some((3, 0)));
        // A joint not selected has no handle over everything, but shows, and is picked, where
        // it is.
        let other = mesh.handles[1].center;
        assert_eq!(cache.joint_hit(&s.scene, &preset, other + Vec3::Z * 50.0, -Vec3::Z), None);
        assert_eq!(cache.piece_hit(&s.scene, &preset, other - Vec3::Y * 50.0, Vec3::Y).map(|h| (h.0, h.1)), Some((2, Piece::Joint(0))));
        let side = Vec3::new(head.x, head.y - 50.0, head.z / 2.0);
        assert_eq!(cache.piece_hit(&s.scene, &preset, side, Vec3::Y).map(|h| (h.0, h.1)), Some((3, Piece::Segment(0))));
        assert_eq!(cache.piece_hit(&s.scene, &preset, Vec3::new(head.x, -50.0, 0.1), Vec3::Y).map(|h| (h.0, h.1)), Some((3, Piece::Whole)));
        // A deleted support's route goes.
        s.execute("support.delete", &json!({ "ids": [2] })).unwrap();
        while cache.update(&s.scene, &preset) {}
        assert_eq!(cache.routes.len(), 4);
        std::fs::remove_file(path).ok();
    }
}
