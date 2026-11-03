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
    let m = &s.scene.models[0];
    assert!((m.mesh.signed_volume() - 1000.0).abs() < 1e-6);
    assert_eq!(s.camera.target, m.bounds.center());

    let list = s.execute("model.list", &json!({})).unwrap();
    assert_eq!(list["models"].as_array().map(Vec::len), Some(1));
    let id = r["id"].clone();
    s.execute("model.delete", &json!({ "id": id })).unwrap();
    assert!(s.scene.models.is_empty());
    assert!(matches!(s.execute("model.delete", &json!({ "id": id })), Err(EngineError::BadParams(_))));
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn a_failed_import_leaves_the_session_unchanged() {
    let mut s = Session::new();
    let cam = s.camera;
    assert!(s.execute("file.import_stl", &json!({ "path": "/definitely/not/here.stl" })).is_err());
    assert!(s.scene.models.is_empty());
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
    assert_eq!(s.scene.supports[0].normal, -Vec3::Z);
    assert!(matches!(s.execute("support.add", &json!({ "model": model, "point": [50, 2, 0] })), Err(EngineError::BadParams(_))));
    assert!(matches!(s.execute("support.add", &json!({ "model": 99, "point": [2, 2, 0] })), Err(EngineError::BadParams(_))));

    // The tip sphere is nearer than the face it sits on.
    let (picked, t) = s.scene.support_hit(Vec3::new(5.0, 5.0, -50.0), Vec3::Z).unwrap();
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
    assert!(s.scene.supports.is_empty() && s.scene.selected.is_empty());
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
    let m = s.scene.models[0].clone();
    assert_eq!((m.bounds.min, m.bounds.max), (Vec3::new(10.0, 0.0, 5.0), Vec3::new(20.0, 10.0, 15.0)));
    assert_eq!(s.scene.supports[0].tip, Vec3::new(15.0, 5.0, 5.0));
    assert_eq!(s.scene.models[1].bounds.min, Vec3::ZERO);
    // Picking follows the moved mesh.
    assert_eq!(json!(s.scene.ray_hit(Vec3::new(15.0, 5.0, -50.0), Vec3::Z).map(|h| h.0)), a);

    // A quarter turn about X keeps a cube's box and turns its tip's normal with it.
    s.execute("support.select", &json!({ "ids": [tip] })).unwrap();
    s.execute("model.rotate", &json!({ "degrees": [90, 0, 0] })).unwrap();
    let m = &s.scene.models[0];
    assert!(m.bounds.min.dist(Vec3::new(10.0, 0.0, 5.0)) < 1e-4 && m.bounds.max.dist(Vec3::new(20.0, 10.0, 15.0)) < 1e-4);
    assert!(s.scene.supports[0].normal.dist(Vec3::Y) < 1e-9);
    assert!((m.volume() - 1000.0).abs() < 1e-2);
    assert!(m.placement.rotation.dist(Vec3::new(90.0, 0.0, 0.0)) < 1e-9);

    // Along the model's own axes: turned a quarter about X, its Y is the scene's Z.
    s.execute("model.scale", &json!({ "factors": [2, 1, 0.5] })).unwrap();
    let size = s.scene.models[0].bounds.size();
    assert!(size.dist(Vec3::new(20.0, 5.0, 10.0)) < 1e-4);
    assert!(s.scene.models[0].scaled_size().dist(Vec3::new(20.0, 10.0, 5.0)) < 1e-4);
    assert!(s.scene.models[0].bounds.center().dist(Vec3::new(15.0, 5.0, 10.0)) < 1e-4);
    s.execute("model.scale", &json!({ "factors": 0.5, "ids": [b] })).unwrap();
    assert!(s.scene.models[1].bounds.size().dist(Vec3::new(5.0, 5.0, 5.0)) < 1e-4);

    let before = s.scene.models[0].bounds;
    for bad in [json!({ "factors": 0 }), json!({ "factors": [1, -1, 1] }), json!({ "factors": 1e9 }), json!({ "factors": "2" })] {
        assert!(matches!(s.execute("model.scale", &bad), Err(EngineError::BadParams(_))), "{bad}");
    }
    assert!(matches!(s.execute("model.move", &json!({ "by": [1e300, 0, 0] })), Err(EngineError::BadParams(_))));
    assert_eq!(s.scene.models[0].bounds, before);

    // Placing is absolute, measured from the import: the tip comes back where it was placed.
    s.execute("model.place", &json!({ "models": [{ "id": a, "position": [5, 5, 5], "rotation": [0, 0, 0], "scale": [1, 1, 1] }] })).unwrap();
    assert!(s.scene.models[0].bounds.min.dist(Vec3::ZERO) < 1e-9);
    assert!(s.scene.supports[0].tip.dist(Vec3::new(5.0, 5.0, 0.0)) < 1e-9);
    assert!(s.scene.supports[0].normal.dist(-Vec3::Z) < 1e-9);
    // Picking goes through the placement: a stretched, turned model is hit where it is drawn.
    s.execute("model.place", &json!({ "models": [{ "id": a, "position": [100, 0, 5], "rotation": [0, 0, 45], "scale": [3, 1, 1] }] })).unwrap();
    let (hit, h) = s.scene.ray_hit(Vec3::new(100.0, 0.0, 50.0), -Vec3::Z).unwrap();
    assert_eq!(json!(hit), a);
    assert!((h.t - 40.0).abs() < 1e-6 && (h.point.z - 10.0).abs() < 1e-6);
    let m = s.scene.models[0].clone();
    assert!((m.volume() - 3000.0).abs() < 1e-2);
    assert!(m.face_normal(h.triangle).is_some_and(|n| n.dist(Vec3::Z) < 1e-6));
    // A tip added to the placed model stays on it through the next placement.
    let t2 = s.execute("support.add", &json!({ "model": a, "point": h.point, "normal": [0, 0, 1] })).unwrap()["id"].clone();
    s.execute("model.move", &json!({ "ids": [a], "by": [0, 0, 7] })).unwrap();
    let tip2 = s.scene.supports.iter().find(|t| json!(t.id) == t2).unwrap().tip;
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
