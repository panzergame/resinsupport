use serde_json::{Value, json};

use crate::{EngineError, Session, commands};
use resin_geom::Vec3;

fn write_cube_stl(dir: &std::path::Path) -> std::path::PathBuf {
    let s = |x: f32| x * 10.0;
    let v = |x: f32, y: f32, z: f32| format!("vertex {} {} {}", s(x), s(y), s(z));
    let quads = [
        [(0., 0., 0.), (0., 1., 0.), (1., 1., 0.), (1., 0., 0.)],
        [(0., 0., 1.), (1., 0., 1.), (1., 1., 1.), (0., 1., 1.)],
        [(0., 0., 0.), (1., 0., 0.), (1., 0., 1.), (0., 0., 1.)],
        [(0., 1., 0.), (0., 1., 1.), (1., 1., 1.), (1., 1., 0.)],
        [(0., 0., 0.), (0., 0., 1.), (0., 1., 1.), (0., 1., 0.)],
        [(1., 0., 0.), (1., 1., 0.), (1., 1., 1.), (1., 0., 1.)],
    ];
    let mut text = String::from("solid cube\n");
    for [a, b, c, d] in quads {
        for [p, q, r] in [[a, b, c], [a, c, d]] {
            text += &format!("facet normal 0 0 0\nouter loop\n{}\n{}\n{}\nendloop\nendfacet\n", v(p.0, p.1, p.2), v(q.0, q.1, q.2), v(r.0, r.1, r.2));
        }
    }
    text += "endsolid cube\n";
    let path = dir.join("cube.stl");
    std::fs::write(&path, text).unwrap();
    path
}

fn temp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("resin-engine-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn import_measures_and_frames_the_model() {
    let dir = temp_dir("import");
    let path = write_cube_stl(&dir);
    let mut s = Session::new();
    let r = s.execute("file.import_stl", &json!({ "path": path })).unwrap();
    assert_eq!(r["triangles"], 12);
    assert_eq!(r["name"], "cube");
    let m = &s.scene.models()[0];
    assert!((m.mesh.signed_volume() - 1000.0).abs() < 1e-6);
    assert_eq!(s.camera.target, m.bounds.center());

    let list = s.execute("model.list", &json!({})).unwrap();
    assert_eq!(list["models"].as_array().map(Vec::len), Some(1));
    let id = r["id"].clone();
    s.execute("model.delete", &json!({ "id": id })).unwrap();
    assert!(s.scene.models().is_empty());
    assert!(matches!(s.execute("model.delete", &json!({ "id": id })), Err(EngineError::BadParams(_))));
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn a_failed_import_leaves_the_session_unchanged() {
    let mut s = Session::new();
    let cam = s.camera;
    assert!(s.execute("file.import_stl", &json!({ "path": "/definitely/not/here.stl" })).is_err());
    assert!(s.scene.models().is_empty());
    assert_eq!(s.camera, cam);
}

#[test]
fn view_commands() {
    let mut s = Session::new();
    s.execute("view.standard", &json!({ "view": "top" })).unwrap();
    assert!(s.camera.pitch > 1.5);
    s.execute("view.projection", &json!({ "mode": "toggle" })).unwrap();
    assert!(s.camera.fov > 0.0);
    s.execute("view.projection", &json!({ "mode": "toggle" })).unwrap();
    assert_eq!(s.camera.fov, 0.0);
    s.execute("view.standard", &json!({ "view": "front" })).unwrap();
    s.execute("view.turn", &json!({ "direction": "right", "degrees": 90 })).unwrap();
    assert!((s.camera.yaw - std::f64::consts::FRAC_PI_2).abs() < 1e-9);
    s.execute("view.fit", &json!({ "aspect": 2.0 })).unwrap();
    assert!(s.camera.is_valid());
}

#[test]
fn supports_are_added_selected_and_deleted() {
    let dir = temp_dir("supports");
    let path = write_cube_stl(&dir);
    let mut s = Session::new();
    let model = s.execute("file.import_stl", &json!({ "path": path })).unwrap()["id"].clone();

    // A ray from below meets the cube's bottom face, as a click on it would.
    let (hit_model, hit) = s.scene.ray_hit(Vec3::new(5.0, 5.0, -50.0), Vec3::Z).unwrap();
    assert_eq!(json!(hit_model), model);
    let a = s.execute("support.add", &json!({ "model": model, "point": hit.point, "normal": [0, 0, -2] })).unwrap()["id"].clone();
    let b = s.execute("support.add", &json!({ "model": model, "point": [2, 2, 0] })).unwrap()["id"].clone();
    assert_eq!(s.scene.supports()[0].normal, -Vec3::Z);
    assert!(matches!(s.execute("support.add", &json!({ "model": model, "point": [50, 2, 0] })), Err(EngineError::BadParams(_))));
    assert!(matches!(s.execute("support.add", &json!({ "model": 99, "point": [2, 2, 0] })), Err(EngineError::BadParams(_))));

    // The tip sphere is nearer than the face it sits on.
    let (picked, t) = s.scene.support_hit(Vec3::new(5.0, 5.0, -50.0), Vec3::Z, 0.25).unwrap();
    assert_eq!(json!(picked), a);
    assert!(t < hit.t);

    s.execute("support.select", &json!({ "ids": [a] })).unwrap();
    s.execute("support.select", &json!({ "ids": [b], "mode": "toggle" })).unwrap();
    assert_eq!(s.scene.selected.len(), 2);
    s.execute("support.select", &json!({ "ids": [a], "mode": "toggle" })).unwrap();
    assert!(matches!(s.execute("support.select", &json!({ "ids": [a, 99] })), Err(EngineError::BadParams(_))));
    assert_eq!(json!(s.scene.selected), json!([b]));

    let removed = s.execute("support.delete", &json!({})).unwrap();
    assert_eq!(removed["removed"], json!([b]));
    assert!(s.scene.selected.is_empty());
    assert_eq!(s.execute("support.list", &json!({})).unwrap()["supports"].as_array().map(Vec::len), Some(1));
    assert_eq!(s.execute("support.delete", &json!({})).unwrap()["removed"], json!([]));

    // A model's supports go with it.
    s.execute("support.select", &json!({ "ids": [a] })).unwrap();
    s.execute("model.delete", &json!({ "id": model })).unwrap();
    assert!(s.scene.supports().is_empty() && s.scene.selected.is_empty());
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn models_are_selected_moved_rotated_and_scaled() {
    let dir = temp_dir("transform");
    let path = write_cube_stl(&dir);
    let mut s = Session::new();
    let a = s.execute("file.import_stl", &json!({ "path": path })).unwrap()["id"].clone();
    let b = s.execute("file.import_stl", &json!({ "path": path })).unwrap()["id"].clone();
    let tip = s.execute("support.add", &json!({ "model": a, "point": [5, 5, 0] })).unwrap()["id"].clone();

    // Nothing selected, nothing to act on.
    assert!(matches!(s.execute("model.move", &json!({ "by": [1, 0, 0] })), Err(EngineError::BadParams(_))));
    s.execute("model.select", &json!({ "ids": [a] })).unwrap();
    assert_eq!(json!(s.scene.selected_models), json!([a]));
    assert!(matches!(s.execute("model.select", &json!({ "ids": [99] })), Err(EngineError::BadParams(_))));

    let rev = s.scene.revision();
    s.execute("model.move", &json!({ "by": [10, 0, 5] })).unwrap();
    assert!(s.scene.revision() > rev);
    let m = s.scene.models()[0].clone();
    assert_eq!((m.bounds.min, m.bounds.max), (Vec3::new(10.0, 0.0, 5.0), Vec3::new(20.0, 10.0, 15.0)));
    assert_eq!(s.scene.supports()[0].tip, Vec3::new(15.0, 5.0, 5.0));
    assert_eq!(s.scene.models()[1].bounds.min, Vec3::ZERO);
    // Picking follows the moved mesh.
    assert_eq!(json!(s.scene.ray_hit(Vec3::new(15.0, 5.0, -50.0), Vec3::Z).map(|h| h.0)), a);

    // A quarter turn about X keeps a cube's box and turns its tip's normal with it.
    s.execute("support.select", &json!({ "ids": [tip] })).unwrap();
    s.execute("model.rotate", &json!({ "degrees": [90, 0, 0] })).unwrap();
    let m = &s.scene.models()[0];
    assert!(m.bounds.min.dist(Vec3::new(10.0, 0.0, 5.0)) < 1e-4 && m.bounds.max.dist(Vec3::new(20.0, 10.0, 15.0)) < 1e-4);
    assert!(s.scene.supports()[0].normal.dist(Vec3::Y) < 1e-9);
    assert!((m.volume() - 1000.0).abs() < 1e-2);
    assert!(m.placement.rotation.dist(Vec3::new(90.0, 0.0, 0.0)) < 1e-9);

    // Along the model's own axes: turned a quarter about X, its Y is the scene's Z.
    s.execute("model.scale", &json!({ "factors": [2, 1, 0.5] })).unwrap();
    let size = s.scene.models()[0].bounds.size();
    assert!(size.dist(Vec3::new(20.0, 5.0, 10.0)) < 1e-4);
    assert!(s.scene.models()[0].scaled_size().dist(Vec3::new(20.0, 10.0, 5.0)) < 1e-4);
    assert!(s.scene.models()[0].bounds.center().dist(Vec3::new(15.0, 5.0, 10.0)) < 1e-4);
    s.execute("model.scale", &json!({ "factors": 0.5, "ids": [b] })).unwrap();
    assert!(s.scene.models()[1].bounds.size().dist(Vec3::new(5.0, 5.0, 5.0)) < 1e-4);

    let before = s.scene.models()[0].bounds;
    for bad in [json!({ "factors": 0 }), json!({ "factors": [1, -1, 1] }), json!({ "factors": 1e9 }), json!({ "factors": "2" })] {
        assert!(matches!(s.execute("model.scale", &bad), Err(EngineError::BadParams(_))), "{bad}");
    }
    assert!(matches!(s.execute("model.move", &json!({ "by": [1e300, 0, 0] })), Err(EngineError::BadParams(_))));
    assert_eq!(s.scene.models()[0].bounds, before);

    // Placing is absolute, measured from the import: the tip comes back where it was placed.
    s.execute("model.place", &json!({ "models": [{ "id": a, "position": [5, 5, 5], "rotation": [0, 0, 0], "scale": [1, 1, 1] }] })).unwrap();
    assert!(s.scene.models()[0].bounds.min.dist(Vec3::ZERO) < 1e-9);
    assert!(s.scene.supports()[0].tip.dist(Vec3::new(5.0, 5.0, 0.0)) < 1e-9);
    assert!(s.scene.supports()[0].normal.dist(-Vec3::Z) < 1e-9);
    // Picking goes through the placement: a stretched, turned model is hit where it is drawn.
    s.execute("model.place", &json!({ "models": [{ "id": a, "position": [100, 0, 5], "rotation": [0, 0, 45], "scale": [3, 1, 1] }] })).unwrap();
    let (hit, h) = s.scene.ray_hit(Vec3::new(100.0, 0.0, 50.0), -Vec3::Z).unwrap();
    assert_eq!(json!(hit), a);
    assert!((h.t - 40.0).abs() < 1e-6 && (h.point.z - 10.0).abs() < 1e-6);
    let m = s.scene.models()[0].clone();
    assert!((m.volume() - 3000.0).abs() < 1e-2);
    assert!(m.face_normal(h.triangle).is_some_and(|n| n.dist(Vec3::Z) < 1e-6));
    // A tip added to the placed model stays on it through the next placement.
    let t2 = s.execute("support.add", &json!({ "model": a, "point": h.point, "normal": [0, 0, 1] })).unwrap()["id"].clone();
    s.execute("model.move", &json!({ "ids": [a], "by": [0, 0, 7] })).unwrap();
    let tip2 = s.scene.supports().iter().find(|t| json!(t.id) == t2).unwrap().tip;
    assert!(tip2.dist(h.point + Vec3::new(0.0, 0.0, 7.0)) < 1e-6);
    for bad in [
        json!({ "models": [{ "id": a, "scale": [0, 1, 1] }] }),
        json!({ "models": [{ "id": 99 }] }),
        json!({ "models": [{ "id": a, "position": [2e6, 0, 0] }] }),
    ] {
        assert!(matches!(s.execute("model.place", &bad), Err(EngineError::BadParams(_))), "{bad}");
    }

    s.execute("model.delete", &json!({ "id": a })).unwrap();
    assert!(s.scene.selected_models.is_empty());
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn commands_are_sorted_and_unique() {
    let ids: Vec<&str> = commands().iter().map(|c| c.id).collect();
    let mut sorted = ids.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(ids, sorted);
    assert!(ids.iter().all(|id| id.chars().all(|c| c.is_ascii_lowercase() || c == '.' || c == '_') && id.contains('.')));
}

/// Every command, with junk parameters, returns (an error or a value) and never panics or
/// leaves the camera unusable.
#[test]
fn hostile_params_never_panic() {
    let junk: Vec<Value> = vec![
        Value::Null,
        json!({}),
        json!([]),
        json!("x"),
        json!(1e308),
        json!({ "path": 1, "id": -1, "view": [], "mode": {}, "direction": null, "degrees": "a", "aspect": -1e308, "fit": "yes" }),
        json!({ "path": "", "id": u64::MAX, "view": "nope", "mode": "nope", "direction": "nope", "degrees": 1e300, "aspect": 0 }),
        json!({ "path": "/", "view": "iso", "mode": "perspective", "direction": "up", "degrees": -360, "aspect": 1e-300 }),
        json!({ "model": "1", "point": "x", "normal": [1, 2], "ids": "all" }),
        json!({ "model": 1, "point": [1, null, 3], "normal": [[]], "ids": [-1, 1.5, "a"], "mode": "toggle" }),
        json!({ "model": u64::MAX, "point": [1e308, -1e308, 0], "normal": [0, 0, 0], "ids": [u64::MAX], "mode": 3 }),
        json!({ "by": [1e308, 1e308, 1e308], "degrees": [1e308, -1e308, 0], "factors": [0, 0, 0], "ids": [] }),
        json!({ "by": "x", "degrees": 90, "factors": -1, "ids": [1, 1] }),
        json!({ "by": [1, 2], "degrees": [null, 0, 0], "factors": 1e308, "ids": [0] }),
        json!({ "models": [{ "id": 1, "position": [1e308, 0, 0], "rotation": "x", "scale": [-1, 0, 1e308] }] }),
        json!({ "models": "all" }),
        json!({ "models": [null, 1, {}] }),
        json!({ "preset": "x", "active": 1, "tip": 1, "body": [], "base": "x", "raft": null }),
        json!({ "preset": "light", "tip": { "contact_diameter": 1e308, "contact_shape": 3, "nope": 1 }, "body": { "segments": -1 } }),
        json!({ "active": "heavy", "body": { "segments": 1e20, "diameter": null, "shape": "cube" }, "raft": { "enabled": "yes", "margin": -1 } }),
        json!({ "parts": "all", "ids": null }),
        json!({ "joints": [{ "support": 1, "joint": 0, "position": [0, 0, -1] }, { "support": 1, "joint": u64::MAX, "position": [0, 0, 1e308] }] }),
        json!({ "joints": [{ "support": "1", "joint": -1, "position": "x" }, null], "ids": [u64::MAX] }),
        json!({ "joints": {}, "ids": [-1] }),
        json!({ "parts": [{ "support": 1, "joint": 0, "segment": 1 }, { "support": -1, "joint": 1e300 }, null], "mode": "add" }),
        json!({ "parts": [{ "support": u64::MAX, "segment": u64::MAX }, { "joint": 0 }], "ids": [1] }),
        json!({ "lifted": "yes", "height": -1, "models": [{ "id": 1, "lifted": 1 }] }),
        json!({ "lifted": true, "height": 1e308, "ids": [u64::MAX], "path": "/", "fit": false }),
        json!({ "tip": { "length": [], "penetration": {} }, "base": { "diameter": 0, "height": 1e-300 }, "raft": { "chamfer_angle": "45" } }),
    ];
    for spec in commands() {
        for params in &junk {
            let mut s = Session::new();
            let result = s.execute(spec.id, params);
            assert!(!matches!(result, Err(EngineError::Internal(_))), "{} {params}: {result:?}", spec.id);
            assert!(s.camera.is_valid(), "{} {params}", spec.id);
        }
    }
}

/// What a step must put back, selection aside: the models, where they are, their supports.
fn content(s: &Session) -> String {
    let models: Vec<_> = s.scene.models().iter().map(|m| (m.id, &m.name, m.placement, m.bounds, m.lifted)).collect();
    format!("{models:?} {:?} {}", s.scene.supports(), s.scene.lift_height())
}

/// All of it, selection included.
fn fingerprint(s: &Session) -> String {
    format!("{} {:?} {:?} {:?}", content(s), s.scene.selected_models, s.scene.selected, s.scene.selected_parts)
}

/// Axis-aligned boxes `(min, max)` as one mesh, wound outward.
fn boxes(list: &[([f64; 3], [f64; 3])]) -> resin_geom::Mesh {
    let mut tris = Vec::new();
    for (lo, hi) in list {
        let c = |i: usize| {
            [if i & 1 == 0 { lo[0] } else { hi[0] }, if i & 2 == 0 { lo[1] } else { hi[1] }, if i & 4 == 0 { lo[2] } else { hi[2] }].map(|v| v as f32)
        };
        for [a, b, cc, d] in [[0, 2, 3, 1], [4, 5, 7, 6], [0, 1, 5, 4], [2, 6, 7, 3], [0, 4, 6, 2], [1, 3, 7, 5]] {
            tris.push([c(a), c(b), c(cc)]);
            tris.push([c(a), c(cc), c(d)]);
        }
    }
    resin_geom::Mesh::new(tris)
}

/// Every body segment and the base keep the clearance from every triangle, measured without
/// the index; no segment leans past the limit; the foot is on the plate.
fn check_route(s: &Session, r: &crate::Route) {
    let p = s.support_settings().current();
    let tris: Vec<[Vec3; 3]> =
        s.scene.models().iter().flat_map(|m| m.mesh.triangles.iter().map(|t| t.map(|c| m.transform.point(Vec3::from(c))))).collect();
    let near = |a: Vec3, b: Vec3| tris.iter().map(|t| resin_geom::segment_triangle_distance(a, b, *t)).fold(f64::INFINITY, f64::min);
    let body_r = p.body.diameter / 2.0 + p.body.clearance;
    for w in r.body.windows(2) {
        assert!(near(w[0], w[1]) >= body_r - 1e-9, "{:?} comes within {} of a model", w, near(w[0], w[1]));
        let lean = (w[0] - w[1]).normalized().unwrap().dot(Vec3::Z).acos().to_degrees();
        assert!(lean <= p.body.max_angle + 1e-6, "{lean}° leans past the limit");
    }
    let foot = *r.body.last().unwrap();
    assert_eq!(foot.z, 0.0);
    assert!(near(foot, foot) >= p.base.diameter / 2.0 + p.body.clearance - 1e-9);
    assert!((r.head.dist(r.contact) - p.tip.length).abs() < 1e-9);
}

fn routes(s: &Session) -> Vec<crate::Route> {
    let p = *s.support_settings().current();
    s.scene.supports().iter().map(|t| crate::route(s.scene.models(), t, &p)).collect()
}

#[test]
fn a_support_goes_straight_down_when_it_can() {
    let mut s = Session::new();
    let m = s.scene.add("slab".into(), boxes(&[([0.0, 0.0, 10.0], [20.0, 20.0, 12.0])]));
    s.execute("support.add", &json!({ "model": m, "point": [10, 10, 10], "normal": [0, 0, -1] })).unwrap();
    let r = &routes(&s)[0];
    assert_eq!(r.body, vec![Vec3::new(10.0, 10.0, 8.0), Vec3::new(10.0, 10.0, 0.0)]);
    check_route(&s, r);
    let listed = s.execute("support.list", &json!({})).unwrap();
    assert_eq!(listed["supports"][0]["reachable"], true);
    assert_eq!(listed["supports"][0]["path"], json!([[10.0, 10.0, 10.0], [10.0, 10.0, 8.0], [10.0, 10.0, 0.0]]));
}

#[test]
fn joints_and_segments_are_selected_with_their_supports() {
    let mut s = Session::new();
    // A slab over a pillar: the support bends once, so it has two joints and two segments.
    let m = s.scene.add("arch".into(), boxes(&[([-10.0, -10.0, 12.0], [30.0, 30.0, 14.0]), ([8.0, 8.0, 0.0], [12.0, 12.0, 4.0])]));
    let a = s.execute("support.add", &json!({ "model": m, "point": [10, 10, 12], "normal": [0, 0, -1] })).unwrap()["id"].clone();
    let b = s.execute("support.add", &json!({ "model": m, "point": [25, 25, 12], "normal": [0, 0, -1] })).unwrap()["id"].clone();
    assert_eq!(routes(&s)[0].body.len(), 3);

    s.execute("support.select", &json!({ "ids": [b] })).unwrap();
    // A plain click on a part replaces the supports selected as well.
    s.execute("support.select", &json!({ "parts": [{ "support": a, "joint": 1 }] })).unwrap();
    assert!(s.scene.selected.is_empty());
    s.execute("support.select", &json!({ "parts": [{ "support": a, "segment": 0 }, { "support": a, "joint": 1 }], "mode": "toggle" })).unwrap();
    s.execute("support.select", &json!({ "parts": [{ "support": b, "segment": 0 }], "ids": [b], "mode": "add" })).unwrap();
    assert_eq!(s.scene.selected_parts.len(), 2);
    assert_eq!(json!(s.scene.selected), json!([b]));
    // A number the route does not have is kept, and listed only while the route has it; the
    // foot is no joint.
    s.execute("support.select", &json!({ "parts": [{ "support": a, "joint": 2 }, { "support": a, "segment": 7 }], "mode": "add" })).unwrap();
    let listed = s.execute("support.list", &json!({})).unwrap();
    assert_eq!(listed["supports"][0]["selected_segments"], json!([0]));
    assert_eq!(listed["supports"][0]["selected_joints"], json!([]));
    assert_eq!(listed["supports"][1]["selected_segments"], json!([0]));
    assert!(matches!(s.execute("support.select", &json!({ "parts": [{ "support": 99, "joint": 0 }] })), Err(EngineError::BadParams(_))));
    assert!(matches!(s.execute("support.select", &json!({ "parts": [{ "support": a, "joint": 0, "segment": 0 }] })), Err(EngineError::BadParams(_))));
    assert!(matches!(s.execute("support.select", &json!({})), Err(EngineError::BadParams(_))));
    assert_eq!(s.scene.selected_parts.len(), 4, "a failed select changes nothing");

    // Deleting a support drops its parts; undo brings both back.
    let before = fingerprint(&s);
    s.execute("support.delete", &json!({ "ids": [a] })).unwrap();
    assert!(s.scene.selected_parts.iter().all(|(id, _)| json!(id) == b));
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(fingerprint(&s), before);
    s.execute("support.select", &json!({ "ids": [] })).unwrap();
    assert!(s.scene.selected.is_empty() && s.scene.selected_parts.is_empty());
}

#[test]
fn placed_joints_keep_their_path() {
    let mut s = Session::new();
    // The arch: a slab over a pillar, so the worked-out route has one knee.
    let m = s.scene.add("arch".into(), boxes(&[([-10.0, -10.0, 12.0], [30.0, 30.0, 14.0]), ([8.0, 8.0, 0.0], [12.0, 12.0, 4.0])]));
    let a = s.execute("support.add", &json!({ "model": m, "point": [10, 10, 12], "normal": [0, 0, -1] })).unwrap()["id"].clone();
    let auto = routes(&s)[0].clone();
    assert_eq!(auto.body.len(), 3);

    // Moving the knee keeps the head where the search put it, and the foot drops below the knee.
    let knee = Vec3::new(10.0, 15.0, 4.0);
    let depth = s.undo_depth();
    s.execute("support.place_joints", &json!({ "joints": [{ "support": a, "joint": 1, "position": knee }] })).unwrap();
    assert_eq!(s.undo_depth(), depth + 1);
    let r = routes(&s)[0].clone();
    assert_eq!(r.body, vec![auto.body[0], knee, Vec3::new(10.0, 15.0, 0.0)]);
    assert!(!r.fault, "{r:?}");
    let listed = s.execute("support.list", &json!({})).unwrap();
    assert_eq!((listed["supports"][0]["edited"].clone(), listed["supports"][0]["fault"].clone()), (json!(true), json!(false)));

    // Moved, the model carries the corners: the support keeps its shape.
    let up = Vec3::new(3.0, -2.0, 1.0);
    s.execute("model.move", &json!({ "ids": [m], "by": up })).unwrap();
    let moved = routes(&s)[0].clone();
    assert!(moved.body[1].dist(knee + up) < 1e-9 && moved.head.dist(auto.body[0] + up) < 1e-9, "{moved:?}");
    assert!((moved.contact.z - 13.0).abs() < 1e-9 && !moved.fault);
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(routes(&s)[0].body, r.body);

    // Turned or scaled, it leaves them where they were put, and the tip stretches to them.
    s.execute("model.rotate", &json!({ "ids": [m], "degrees": [0, 0, 10] })).unwrap();
    assert_eq!(routes(&s)[0].body[1], knee);
    s.execute("edit.undo", &json!({})).unwrap();
    s.execute("model.place", &json!({ "models": [{ "id": m, "position": [10, 10, 8], "scale": [1, 1, 1.2] }] })).unwrap();
    let r2 = routes(&s)[0].clone();
    assert!(r2.body[1] == knee && r2.head == auto.body[0], "{r2:?}");
    s.execute("edit.undo", &json!({})).unwrap();

    // Moved down past the plate, the corners go under it: kept, flagged, for the user to move.
    s.execute("model.move", &json!({ "ids": [m], "by": [0, 0, -6] })).unwrap();
    let low = routes(&s)[0].clone();
    assert_eq!(low.body[1], Vec3::new(10.0, 15.0, -2.0));
    assert!((low.head.z - (auto.body[0].z - 6.0)).abs() < 1e-9);
    assert!(low.fault, "{low:?}");
    s.execute("edit.undo", &json!({})).unwrap();

    // A knee pulled out sideways leans past the limit: kept, flagged.
    s.execute("support.place_joints", &json!({ "joints": [{ "support": a, "joint": 1, "position": [10, 30, 7] }] })).unwrap();
    assert!(routes(&s)[0].fault);
    // Into the pillar: flagged too.
    s.execute("support.place_joints", &json!({ "joints": [{ "support": a, "joint": 1, "position": [10, 10, 6] }] })).unwrap();
    assert!(routes(&s)[0].fault);

    for bad in [
        json!([{ "support": a, "joint": 2, "position": [0, 0, 1] }]),
        json!([{ "support": a, "joint": 0, "position": [0, 0, -1] }]),
        json!([{ "support": 99, "joint": 0, "position": [0, 0, 1] }]),
    ] {
        assert!(matches!(s.execute("support.place_joints", &json!({ "joints": bad })), Err(EngineError::BadParams(_))));
    }

    // Reset: the search's route again; by default for the supports with something selected.
    s.execute("support.select", &json!({ "parts": [{ "support": a, "segment": 0 }] })).unwrap();
    assert_eq!(s.execute("support.reset_route", &json!({})).unwrap()["reset"], json!([a]));
    assert_eq!(routes(&s)[0], auto);
    assert_eq!(s.undo_command(), Some("support.reset_route"));

    // A support with no way down gets its head as its one corner, and a new place gives it a way.
    let low = s.scene.add("low".into(), boxes(&[([40.0, 0.0, 2.0], [50.0, 10.0, 4.0])]));
    let b = s.execute("support.add", &json!({ "model": low, "point": [49.5, 5, 2], "normal": [0, 0, -1] })).unwrap()["id"].clone();
    assert!(!routes(&s)[1].reachable());
    s.execute("support.place_joints", &json!({ "joints": [{ "support": b, "joint": 0, "position": [52, 5, 1.5] }] })).unwrap();
    let r = &routes(&s)[1];
    assert!(r.reachable() && r.body.len() == 2);
}

#[test]
fn a_tip_on_a_side_wall_leans_down_and_clears_it() {
    let mut s = Session::new();
    let m = s.scene.add("block".into(), boxes(&[([0.0, 0.0, 5.0], [10.0, 10.0, 25.0])]));
    s.execute("support.add", &json!({ "model": m, "point": [10, 5, 15], "normal": [1, 0, 0] })).unwrap();
    let r = &routes(&s)[0];
    assert!(r.reachable());
    // Medium: a 2 mm tip leaning 40° from straight down ends 2 sin 40° = 1.286 mm off the wall,
    // past the body's 0.5 mm radius and 0.5 mm clearance, so the body goes straight down.
    assert!((r.head.x - 10.0 - 2.0 * 40f64.to_radians().sin()).abs() < 1e-9);
    assert_eq!(r.body.len(), 2);
    check_route(&s, r);
}

#[test]
fn a_support_bends_round_what_is_below_it() {
    let mut s = Session::new();
    // A slab over a pillar standing on the plate, right under the tip.
    let m = s.scene.add("arch".into(), boxes(&[([-10.0, -10.0, 12.0], [30.0, 30.0, 14.0]), ([8.0, 8.0, 0.0], [12.0, 12.0, 4.0])]));
    s.execute("support.add", &json!({ "model": m, "point": [10, 10, 12], "normal": [0, 0, -1] })).unwrap();
    let r = &routes(&s)[0];
    assert_eq!(r.body.len(), 3, "one knee: {:?}", r.body);
    check_route(&s, r);
    // No longer than it must be: the vertical drop beside the pillar is at most a few steps out.
    let length: f64 = r.body.windows(2).map(|w| w[0].dist(w[1])).sum();
    assert!(length < 14.0, "{length}");

    // Straight down only: nowhere to go. A settings change, so no undo step.
    let undo = s.undo_depth();
    s.execute("support.configure", &json!({ "body": { "max_angle": 0 } })).unwrap();
    assert!(!routes(&s)[0].reachable());
    assert_eq!(s.execute("support.list", &json!({})).unwrap()["supports"][0]["reachable"], false);
    assert_eq!(s.undo_depth(), undo);

    // More clearance than the gap allows near the pillar moves the knee further out.
    s.execute("support.configure", &json!({ "body": { "max_angle": 40, "clearance": 1.5 } })).unwrap();
    let wide = &routes(&s)[0];
    check_route(&s, wide);
    let out = |r: &crate::Route| Vec3::new(r.body[2].x - 10.0, r.body[2].y - 10.0, 0.0).len();
    assert!(out(wide) > out(r));
}

#[test]
fn a_tip_with_no_way_down_is_kept_but_unreachable() {
    let mut s = Session::new();
    // The inner ceiling of a closed hollow box, and a tip too near the plate for a base.
    let m = s.scene.add("box".into(), boxes(&[([0.0, 0.0, 0.0], [20.0, 20.0, 20.0]), ([2.0, 2.0, 2.0], [18.0, 18.0, 18.0])]));
    let low = s.scene.add("low".into(), boxes(&[([40.0, 0.0, 2.0], [50.0, 10.0, 4.0])]));
    s.execute("support.add", &json!({ "model": m, "point": [10, 10, 18], "normal": [0, 0, -1] })).unwrap();
    s.execute("support.add", &json!({ "model": low, "point": [45, 5, 2], "normal": [0, 0, -1] })).unwrap();
    s.execute("support.add", &json!({ "model": low, "point": [45, 5, 4], "normal": [0, 0, 1] })).unwrap();
    assert!(routes(&s).iter().all(|r| !r.reachable()));
    assert_eq!(s.scene.supports().len(), 3);
    let listed = s.execute("support.list", &json!({})).unwrap();
    assert_eq!(listed["supports"][0]["path"].as_array().map(Vec::len), Some(2), "the tip alone");
}

#[test]
fn moving_a_model_moves_its_supports_routes() {
    let mut s = Session::new();
    let m = s.scene.add("slab".into(), boxes(&[([0.0, 0.0, 10.0], [20.0, 20.0, 12.0])]));
    let other = s.scene.add("pillar".into(), boxes(&[([30.0, 30.0, 0.0], [34.0, 34.0, 3.0])]));
    s.execute("support.add", &json!({ "model": m, "point": [10, 10, 10], "normal": [0, 0, -1] })).unwrap();
    assert_eq!(routes(&s)[0].body.len(), 2);
    // The other model's pillar moved under the tip blocks the straight path.
    s.execute("model.move", &json!({ "ids": [other], "by": [-22, -22, 0] })).unwrap();
    let r = &routes(&s)[0];
    assert_eq!(r.body.len(), 3);
    check_route(&s, r);
    // Lifting the slab carries the tip and lengthens the support.
    s.execute("model.move", &json!({ "ids": [m], "by": [0, 0, 5] })).unwrap();
    assert_eq!(routes(&s)[0].head.z, 13.0);
}

/// Two cubes (the first selected, moved off the second) and a selected tip on the first.
struct Fixture {
    session: Session,
    path: std::path::PathBuf,
    /// A project file holding the fixture's scene.
    project: std::path::PathBuf,
    a: u64,
    b: u64,
    tip: u64,
}

fn fixture(dir: &std::path::Path) -> Fixture {
    let path = write_cube_stl(dir);
    let mut s = Session::new();
    let a = s.execute("file.import_stl", &json!({ "path": path })).unwrap()["id"].as_u64().unwrap();
    let b = s.execute("file.import_stl", &json!({ "path": path })).unwrap()["id"].as_u64().unwrap();
    s.execute("model.move", &json!({ "ids": [b], "by": [30, 0, 0] })).unwrap();
    let tip = s.execute("support.add", &json!({ "model": a, "point": [5, 5, 0] })).unwrap()["id"].as_u64().unwrap();
    s.execute("model.select", &json!({ "ids": [a] })).unwrap();
    s.execute("support.select", &json!({ "ids": [tip] })).unwrap();
    let project = dir.join("fixture.rsup");
    std::fs::write(&project, resin_io::write_project(&s.scene.project()).unwrap()).unwrap();
    Fixture { session: s, path, project, a, b, tip }
}

/// Valid parameters for every command, run on the fixture. `None`: undo and redo, the history
/// itself, which `undo_and_redo_walk_the_history` covers, and opening a project, which starts a
/// fresh one (`opening_starts_a_fresh_history`). A new command needs a row here.
fn sample(id: &str, f: &Fixture) -> Option<Option<Value>> {
    let (a, b, tip) = (f.a, f.b, f.tip);
    Some(match id {
        "edit.redo" | "edit.undo" => None,
        "file.import_stl" => Some(json!({ "path": f.path, "fit": false })),
        "file.open" => None,
        "file.save" => Some(json!({ "path": f.project.with_file_name("saved.rsup") })),
        "model.delete" => Some(json!({ "id": a })),
        "model.lift" => Some(json!({ "lifted": true })),
        "model.list" | "support.list" | "scene.clear" => Some(json!({})),
        "scene.lift_height" => Some(json!({ "height": 8 })),
        "model.move" => Some(json!({ "by": [1, 2, 3] })),
        "model.place" => Some(json!({ "models": [{ "id": b, "position": [0, 40, 5], "rotation": [0, 0, 30], "scale": [2, 1, 1] }] })),
        "model.rotate" => Some(json!({ "degrees": [0, 90, 0] })),
        "model.scale" => Some(json!({ "factors": 1.5 })),
        "model.select" => Some(json!({ "ids": [b] })),
        "support.add" => Some(json!({ "model": b, "point": [35, 5, 0] })),
        "support.delete" => Some(json!({ "ids": [tip] })),
        "support.configure" => {
            Some(json!({ "preset": "light", "active": "heavy", "tip": { "contact_diameter": 0.3 }, "raft": { "enabled": false } }))
        }
        "support.reset_preset" => Some(json!({ "preset": "medium" })),
        "support.place_joints" => Some(json!({ "joints": [{ "support": tip, "joint": 0, "position": [6, 5, 3] }] })),
        "support.reset_route" => Some(json!({ "ids": [tip] })),
        "support.select" => Some(json!({ "parts": [{ "support": tip, "joint": 0 }], "mode": "add" })),
        "support.settings" => Some(json!({})),
        "view.fit" => Some(json!({ "aspect": 1.5 })),
        "view.projection" => Some(json!({ "mode": "toggle" })),
        "view.standard" => Some(json!({ "view": "top" })),
        "view.turn" => Some(json!({ "direction": "left" })),
        _ => return None,
    })
}

/// Every command that changes the scene is one undo step that puts back exactly what was there,
/// selection included, and redo makes it again; one that changes only the selection or the view
/// makes no step. A command without a `sample` row fails here.
#[test]
fn every_scene_change_is_undoable() {
    let dir = temp_dir("undoable");
    let f0 = fixture(&dir);
    for spec in commands() {
        let row = sample(spec.id, &f0).unwrap_or_else(|| panic!("{}: add a row to `sample` in crates/engine/src/tests.rs", spec.id));
        let Some(params) = row else {
            assert!(!spec.undoable, "{} runs the history, so it can't be a step of it", spec.id);
            continue;
        };
        let mut f = fixture(&dir);
        let s = &mut f.session;
        let (before, depth) = (fingerprint(s), s.undo_depth());
        let changed_from = content(s);
        s.execute(spec.id, &params).unwrap_or_else(|e| panic!("{} {params}: {e}", spec.id));
        let after = fingerprint(s);
        if content(s) == changed_from {
            assert_eq!(s.undo_depth(), depth, "{} changed nothing in the scene but made an undo step", spec.id);
            continue;
        }
        assert!(spec.undoable, "{} changes the scene but is noundo", spec.id);
        assert_eq!(s.undo_depth(), depth + 1, "{} changed the scene without one undo step (does its Scene method bump the revision?)", spec.id);
        assert_eq!(s.undo_command(), Some(spec.id));
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(fingerprint(s), before, "{}: undo did not put the scene back", spec.id);
        s.execute("edit.redo", &json!({})).unwrap();
        assert_eq!(fingerprint(s), after, "{}: redo did not make it again", spec.id);
    }
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn undo_and_redo_walk_the_history() {
    let dir = temp_dir("history");
    let path = write_cube_stl(&dir);
    let mut s = Session::new();
    assert!(matches!(s.execute("edit.undo", &json!({})), Err(EngineError::Failed(_))));
    assert!(matches!(s.execute("edit.redo", &json!({})), Err(EngineError::Failed(_))));

    let a = s.execute("file.import_stl", &json!({ "path": path })).unwrap()["id"].as_u64().unwrap();
    s.execute("model.move", &json!({ "ids": [a], "by": [10, 0, 0] })).unwrap();
    // Selecting, listing and the view make no step; the camera is not part of one.
    s.execute("model.select", &json!({ "ids": [a] })).unwrap();
    s.execute("model.list", &json!({})).unwrap();
    s.execute("view.standard", &json!({ "view": "top" })).unwrap();
    let cam = s.camera;
    assert_eq!(s.undo_depth(), 2);
    // A delete that removes nothing is no step either, nor placing a model where it is.
    s.execute("support.delete", &json!({})).unwrap();
    s.execute("model.place", &json!({ "models": [{ "id": a }] })).unwrap();
    assert_eq!(s.undo_depth(), 2);

    let rev = s.scene.revision();
    assert_eq!(s.execute("edit.undo", &json!({})).unwrap(), json!({ "undone": "Move", "command": "model.move" }));
    assert_eq!(s.scene.models()[0].bounds.min, Vec3::ZERO);
    assert_eq!(s.camera, cam);
    assert!(s.scene.revision() > rev, "a restored scene must not reuse a revision a cache has seen");
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(s.scene.models().is_empty());
    assert!(matches!(s.execute("edit.undo", &json!({})), Err(EngineError::Failed(_))));
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(s.redo_command(), Some("model.move"));

    // A new change drops what could be redone; an id is never given out twice.
    let b = s.execute("file.import_stl", &json!({ "path": path })).unwrap()["id"].as_u64().unwrap();
    assert_eq!(s.redo_command(), None);
    assert_ne!(a, b);
    s.execute("edit.undo", &json!({})).unwrap();
    s.execute("edit.undo", &json!({})).unwrap();
    let c = s.execute("file.import_stl", &json!({ "path": path })).unwrap()["id"].as_u64().unwrap();
    assert!(c > b);
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn a_step_groups_commands_and_a_tool_can_forget_its_own() {
    let dir = temp_dir("steps");
    let f = fixture(&dir);
    let (mut s, a, b) = (f.session, f.a, f.b);
    let depth = s.undo_depth();
    let before = fingerprint(&s);

    // A drag: one step, however many frames.
    s.begin_step();
    for x in 1..=5 {
        s.execute("model.place", &json!({ "models": [{ "id": a, "position": [x, 0, 0] }] })).unwrap();
    }
    s.end_step();
    assert_eq!(s.undo_depth(), depth + 1);
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(fingerprint(&s), before);

    // A tool's Cancel: it puts the placements back, then its steps go.
    let mark = s.history_mark();
    let start: Vec<Value> = s.scene.models().iter().map(|m| json!({ "id": m.id, "position": m.placement.position })).collect();
    s.execute("model.place", &json!({ "models": [{ "id": a, "position": [1, 1, 1] }] })).unwrap();
    s.execute("model.place", &json!({ "models": [{ "id": b, "position": [2, 2, 2] }] })).unwrap();
    s.execute("model.place", &json!({ "models": start })).unwrap();
    assert!(s.forget_since(mark, "model.place"));
    assert_eq!(s.undo_depth(), depth);
    assert_eq!(fingerprint(&s), before);
    // Another command's step since the mark keeps them all.
    let mark = s.history_mark();
    s.execute("model.place", &json!({ "models": [{ "id": a, "position": [3, 3, 3] }] })).unwrap();
    s.execute("model.delete", &json!({ "id": b })).unwrap();
    assert!(!s.forget_since(mark, "model.place"));
    assert_eq!(s.undo_depth(), depth + 2);
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn the_history_keeps_the_last_max_undo_steps() {
    let dir = temp_dir("max_undo");
    let f = fixture(&dir);
    let mut s = f.session;
    let mark = s.history_mark();
    for i in 0..crate::session::MAX_UNDO + 5 {
        s.execute("model.place", &json!({ "models": [{ "id": f.a, "position": [i, 0, 0] }] })).unwrap();
    }
    assert_eq!(s.undo_depth(), crate::session::MAX_UNDO);
    // The marks still find the steps made since, though the oldest have gone.
    assert!(s.forget_since(mark, "model.place"));
    assert_eq!(s.undo_depth(), 0);
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn support_settings_merge_validate_and_round_trip() {
    use crate::{BodyShape, PresetId, SupportSettings};
    let mut s = Session::new();
    let all = s.execute("support.settings", &json!({})).unwrap();
    assert_eq!(all["active"], "medium");
    assert_eq!(all["light"]["tip"]["contact_shape"], "cone");
    // What the command lists is what storage reads back, unchanged.
    let (back, problems) = SupportSettings::from_stored(&all);
    assert_eq!((back, problems), (*s.support_settings(), vec![]));

    // A partial merge into the active preset touches only what it names.
    s.execute("support.configure", &json!({ "tip": { "length": 2.5 }, "body": { "shape": "prism", "segments": 6 } })).unwrap();
    let m = s.support_settings().medium;
    assert_eq!((m.tip.length, m.body.shape, m.body.segments), (2.5, BodyShape::Prism, 6));
    assert_eq!(m.tip.contact_diameter, PresetId::Medium.defaults().tip.contact_diameter);
    assert_eq!(s.support_settings().light, PresetId::Light.defaults());
    // `preset` defaults to the new active one.
    s.execute("support.configure", &json!({ "active": "light", "raft": { "margin": 3 } })).unwrap();
    assert_eq!((s.support_settings().active, s.support_settings().light.raft.margin), (PresetId::Light, 3.0));

    // Rejections change nothing: bad types, ranges, names, and sizes that don't fit together.
    let before = *s.support_settings();
    for bad in [
        json!({ "tip": { "contact_diameter": -1 } }),
        json!({ "tip": { "contact_diameter": 1.0 } }),
        json!({ "tip": { "penetration": 0.8, "length": 0.5 } }),
        json!({ "body": { "diameter": 9.0 } }),
        json!({ "body": { "segments": 2 } }),
        json!({ "body": { "segments": 6.5 } }),
        json!({ "body": { "shape": "cube" } }),
        json!({ "tip": { "lenght": 2 } }),
        json!({ "raft": { "enabled": 1 } }),
        json!({ "raft": "on" }),
        json!({ "preset": "huge" }),
        json!({ "elevation": 5 }),
        json!([1]),
    ] {
        assert!(matches!(s.execute("support.configure", &bad), Err(EngineError::BadParams(_))), "{bad}");
        assert_eq!(*s.support_settings(), before, "{bad}");
    }
    assert!(s.set_support_settings(SupportSettings { light: PresetId::Heavy.defaults(), ..before }).is_ok());
    let mut broken = before;
    broken.heavy.tip.length = f64::NAN;
    assert!(s.set_support_settings(broken).is_err());

    // Restore defaults: one preset only.
    s.execute("support.configure", &json!({ "preset": "heavy", "base": { "height": 2 } })).unwrap();
    s.execute("support.reset_preset", &json!({ "preset": "heavy" })).unwrap();
    assert_eq!(s.support_settings().heavy, PresetId::Heavy.defaults());
    assert_eq!(s.support_settings().medium.tip.length, 2.5);
    assert!(matches!(s.execute("support.reset_preset", &json!({ "preset": 3 })), Err(EngineError::BadParams(_))));
}

#[test]
fn stored_support_settings_fall_back_field_by_field() {
    use crate::{PresetId, SupportSettings};
    let stored = json!({
        "active": "heavy",
        "light": { "tip": { "length": 3.0, "contact_diameter": "big", "from_the_future": 1 }, "body": { "segments": 1000 } },
        "medium": { "tip": { "contact_diameter": 0.9 } },
        "heavy": 7,
        "extra": true,
    });
    let (s, mut problems) = SupportSettings::from_stored(&stored);
    problems.sort();
    assert_eq!(problems, ["heavy", "light.body.segments", "light.tip.contact_diameter", "medium"]);
    assert_eq!(s.active, PresetId::Heavy);
    assert_eq!(s.light.tip.length, 3.0);
    assert_eq!(s.light.tip.contact_diameter, PresetId::Light.defaults().tip.contact_diameter);
    assert_eq!(s.medium, PresetId::Medium.defaults(), "fields that read but don't fit together: the whole preset");
    assert!(s.validate().is_ok());
    // Through serde too, which is how preferences read it; nothing makes it fail.
    let s2: SupportSettings = serde_json::from_value(stored).unwrap();
    assert_eq!(s2, s);
    assert_eq!(serde_json::from_value::<SupportSettings>(json!("junk")).unwrap(), SupportSettings::default());
}

#[test]
fn support_settings_are_never_an_undo_step() {
    let dir = temp_dir("settings_undo");
    let f = fixture(&dir);
    let mut s = f.session;
    s.execute("support.add", &json!({ "model": f.a, "point": [2, 2, 0] })).unwrap();
    s.execute("edit.undo", &json!({})).unwrap();
    let (undo, redo) = (s.undo_depth(), s.redo_depth());
    s.execute("support.configure", &json!({ "tip": { "length": 2.2 } })).unwrap();
    s.execute("support.reset_preset", &json!({ "preset": "light" })).unwrap();
    assert_eq!((s.undo_depth(), s.redo_depth()), (undo, redo), "no step, and redo is kept");
    let settings = *s.support_settings();
    s.execute("edit.redo", &json!({})).unwrap();
    s.execute("edit.undo", &json!({})).unwrap();
    s.execute("support.delete", &json!({ "ids": [f.tip] })).unwrap();
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(*s.support_settings(), settings, "undoing support edits leaves the settings alone");
    // A failing command leaves them as they were.
    assert!(s.execute("support.configure", &json!({ "tip": { "length": 2.4 }, "body": { "segments": 0 } })).is_err());
    assert_eq!(*s.support_settings(), settings);
    std::fs::remove_dir_all(dir).ok();
}

/// A lifted model's lowest point stays at the lift height through import, rotation, scaling,
/// moves across the plate and a new height; moving it up or down lets it go, and the flag and
/// height come back with undo.
#[test]
fn lifted_models_keep_to_the_lift_height() {
    let dir = temp_dir("lift");
    let path = write_cube_stl(&dir);
    let mut s = Session::new();
    let a = s.execute("file.import_stl", &json!({ "path": path, "lifted": true })).unwrap()["id"].as_u64().unwrap();
    let b = s.execute("file.import_stl", &json!({ "path": path })).unwrap()["id"].as_u64().unwrap();
    let model = |s: &Session, id: u64| s.scene.models().iter().find(|m| m.id == id).unwrap().clone();
    assert_eq!(model(&s, a).bounds.min.z, crate::DEFAULT_LIFT_HEIGHT);
    assert!(model(&s, a).lifted);
    assert_eq!(model(&s, b).bounds.min.z, 0.0, "imports stay as they are unless asked");

    // Turned onto an edge, the cube's lowest corner is back at the lift height.
    s.execute("model.rotate", &json!({ "ids": [a], "degrees": [45, 0, 0] })).unwrap();
    assert!((model(&s, a).bounds.min.z - 5.0).abs() < 1e-9);
    s.execute("model.scale", &json!({ "ids": [a], "factors": 2 })).unwrap();
    assert!((model(&s, a).bounds.min.z - 5.0).abs() < 1e-9);
    s.execute("model.move", &json!({ "ids": [a], "by": [10, 3, 0] })).unwrap();
    assert!(model(&s, a).lifted);
    // The gizmo and panel send placements: X and Y alone keep it lifted, and a turn re-lifts.
    let pl = model(&s, a).placement;
    let at = |x: f64, z: f64| json!({ "models": [{ "id": a, "position": [x, pl.position.y, z] }] });
    s.execute("model.place", &at(pl.position.x + 1.0, pl.position.z)).unwrap();
    assert!(model(&s, a).lifted);
    s.execute("model.place", &json!({ "models": [{ "id": a, "rotation": [0, 0, 0], "position": [0, 0, 50] }] })).unwrap();
    assert!(model(&s, a).lifted);
    assert!((model(&s, a).bounds.min.z - 5.0).abs() < 1e-9);

    s.execute("scene.lift_height", &json!({ "height": 2.5 })).unwrap();
    assert!((model(&s, a).bounds.min.z - 2.5).abs() < 1e-9);
    assert_eq!(model(&s, b).bounds.min.z, 0.0, "a model that isn't lifted stays");
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.scene.lift_height(), 5.0);
    assert!((model(&s, a).bounds.min.z - 5.0).abs() < 1e-9);
    s.execute("edit.redo", &json!({})).unwrap();

    // Up or down is the user's height: the model stays where it is put.
    s.execute("model.move", &json!({ "ids": [a], "by": [0, 0, 4] })).unwrap();
    assert!(!model(&s, a).lifted);
    assert!((model(&s, a).bounds.min.z - 6.5).abs() < 1e-9);
    s.execute("model.rotate", &json!({ "ids": [a], "degrees": [0, 0, 90] })).unwrap();
    assert!((model(&s, a).bounds.min.z - 6.5).abs() < 1e-9);
    s.execute("edit.undo", &json!({})).unwrap();
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(model(&s, a).lifted);

    // A Z typed in the panel lets it go too, and `lifted` given wins over that.
    let z = model(&s, a).placement.position.z;
    s.execute("model.place", &json!({ "models": [{ "id": a, "position": [0, 0, z + 1.0] }] })).unwrap();
    assert!(!model(&s, a).lifted);
    s.execute("model.place", &json!({ "models": [{ "id": a, "position": [0, 0, z + 2.0], "lifted": true }] })).unwrap();
    assert!(model(&s, a).lifted);
    assert!((model(&s, a).bounds.min.z - 2.5).abs() < 1e-9);

    s.execute("model.lift", &json!({ "ids": [b], "lifted": true })).unwrap();
    assert!((model(&s, b).bounds.min.z - 2.5).abs() < 1e-9);
    s.execute("model.lift", &json!({ "ids": [b], "lifted": false })).unwrap();
    assert!(!model(&s, b).lifted);
    assert!((model(&s, b).bounds.min.z - 2.5).abs() < 1e-9, "let go, it stays where it was");
    let depth = s.undo_depth();
    s.execute("model.lift", &json!({ "ids": [b], "lifted": false })).unwrap();
    s.execute("scene.lift_height", &json!({ "height": 2.5 })).unwrap();
    assert_eq!(s.undo_depth(), depth, "no change, no step");
    for bad in [json!({ "height": -1 }), json!({ "height": 501 }), json!({ "height": "5" })] {
        assert!(matches!(s.execute("scene.lift_height", &bad), Err(EngineError::BadParams(_))), "{bad}");
    }
    assert_eq!(s.scene.lift_height(), 2.5);
    std::fs::remove_dir_all(dir).ok();
}

/// Supports as `support.list` describes them, routes included, without ids or the selection.
fn supports_seen(s: &mut Session) -> Vec<Value> {
    let mut list = s.execute("support.list", &json!({})).unwrap()["supports"].as_array().unwrap().clone();
    for t in &mut list {
        let t = t.as_object_mut().unwrap();
        for key in ["id", "model", "selected", "selected_joints", "selected_segments"] {
            t.remove(key);
        }
    }
    list
}

#[test]
fn a_saved_project_opens_as_it_was_saved_unlifted() {
    let dir = temp_dir("project-round-trip");
    let Fixture { session: mut s, a, b, tip, .. } = fixture(&dir);
    s.execute("support.add", &json!({ "model": b, "point": [35, 5, 0] })).unwrap();
    s.execute("model.place", &json!({ "models": [{ "id": b, "rotation": [0, 0, 30], "scale": [2, 1, 0.5] }] })).unwrap();
    s.execute("model.lift", &json!({ "ids": [a], "lifted": true })).unwrap();
    s.execute("support.place_joints", &json!({ "joints": [{ "support": tip, "joint": 0, "position": [6, 4, 3] }] })).unwrap();
    let path = dir.join("round.rsup");
    assert!(s.modified());
    let saved = s.execute("file.save", &json!({ "path": path })).unwrap();
    assert!(saved["bytes"].as_u64().unwrap() > 0);
    assert_eq!(s.file(), path.to_str());
    assert!(!s.modified());

    let mut o = Session::new();
    let r = o.execute("file.open", &json!({ "path": path })).unwrap();
    assert_eq!((r["models"].as_u64(), r["supports"].as_u64()), (Some(2), Some(2)));
    assert_eq!(o.scene.project(), s.scene.project());
    let placements = |s: &Session| s.scene.models().iter().map(|m| (m.name.clone(), m.placement, m.bounds)).collect::<Vec<_>>();
    assert_eq!(placements(&o), placements(&s));
    assert!(o.scene.models().iter().all(|m| !m.lifted), "the lift is a model setting: not in the file");
    assert_eq!(supports_seen(&mut o), supports_seen(&mut s));
    assert!(!o.modified());
    assert_eq!(o.file(), path.to_str());
    assert!(!dir.join("round.rsup.bak").exists(), "nothing to back up on a first save");
    s.execute("file.save", &json!({})).unwrap();
    assert!(dir.join("round.rsup.bak").exists());
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn opening_starts_a_fresh_history() {
    let dir = temp_dir("project-history");
    let Fixture { session: mut s, project, .. } = fixture(&dir);
    s.execute("model.move", &json!({ "by": [1, 0, 0] })).unwrap();
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(s.undo_depth() > 0 && s.redo_depth() > 0);
    let mark = s.history_mark();
    s.execute("file.open", &json!({ "path": project })).unwrap();
    assert_eq!((s.undo_depth(), s.redo_depth()), (0, 0), "undo never brings back the scene the project replaced");
    assert!(s.execute("edit.undo", &json!({})).is_err());
    assert_eq!(s.file(), project.to_str());
    assert!(!s.modified());
    s.execute("model.move", &json!({ "by": [1, 0, 0], "ids": [s.scene.models()[0].id] })).unwrap();
    assert!(s.modified());
    assert_eq!(s.undo_command(), Some("model.move"));
    assert!(s.history_mark() > mark, "marks taken before the open never name a later step");

    // A failed open keeps the history it found.
    let depth = s.undo_depth();
    let bad = dir.join("bad.rsup");
    std::fs::write(&bad, b"PK\x03\x04 broken").unwrap();
    assert!(s.execute("file.open", &json!({ "path": bad })).is_err());
    assert_eq!((s.undo_depth(), s.undo_command()), (depth, Some("model.move")));
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn saving_refuses_other_file_kinds_and_opening_refuses_broken_files() {
    let dir = temp_dir("project-refused");
    let Fixture { session: mut s, path: stl, .. } = fixture(&dir);
    let stl_bytes = std::fs::read(&stl).unwrap();
    assert!(matches!(s.execute("file.save", &json!({ "path": stl })), Err(EngineError::BadParams(_))));
    assert_eq!(std::fs::read(&stl).unwrap(), stl_bytes, "the STL is untouched");
    let (before, depth) = (fingerprint(&s), s.undo_depth());
    for bytes in [&b""[..], b"PK\x03\x04 broken", &stl_bytes] {
        let bad = dir.join("bad.rsup");
        std::fs::write(&bad, bytes).unwrap();
        assert!(s.execute("file.open", &json!({ "path": bad })).is_err());
        assert_eq!(fingerprint(&s), before);
    }
    assert!(s.execute("file.open", &json!({ "path": dir.join("missing.rsup") })).is_err());
    assert_eq!(s.undo_depth(), depth);
    std::fs::remove_dir_all(dir).ok();
}
