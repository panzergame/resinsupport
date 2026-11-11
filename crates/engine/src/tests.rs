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

/// The default plate's footprint, Elegoo Saturn 2 sized.
fn plate_of(s: &Session) -> resin_geom::Rect {
    let p = &s.print_config().printer;
    resin_geom::Rect::centred(p.display_x, p.display_y)
}

fn footprint_of(s: &Session, id: u64) -> resin_geom::Rect {
    let b = s.scene.model(id).unwrap().bounds;
    resin_geom::Rect { min: [b.min.x, b.min.y], max: [b.max.x, b.max.y] }
}

fn on(r: &resin_geom::Rect, plate: &resin_geom::Rect) -> bool {
    r.min[0] >= plate.min[0] && r.min[1] >= plate.min[1] && r.max[0] <= plate.max[0] && r.max[1] <= plate.max[1]
}

#[test]
fn imports_are_arranged_on_the_plate_apart_and_put_down() {
    let dir = temp_dir("import-arranged");
    let path = write_cube_stl(&dir);
    let mut s = Session::new();
    let mut ids = Vec::new();
    for lifted in [false, false, true] {
        let r = s.execute("file.import_stl", &json!({ "path": path, "arrange": true, "lifted": lifted })).unwrap();
        assert_eq!(r["on_plate"], true);
        ids.push(r["id"].as_u64().unwrap());
    }
    let first = s.scene.model(ids[0]).unwrap().bounds;
    assert!(first.center().x.abs() < 1e-6 && first.center().y.abs() < 1e-6, "the first on the plate's centre: {first:?}");
    let plate = plate_of(&s);
    for (k, id) in ids.iter().enumerate() {
        let (m, f) = (s.scene.model(*id).unwrap(), footprint_of(&s, *id));
        assert!(on(&f, &plate), "{f:?} on the plate");
        let want = if m.lifted { s.scene.lift_height() } else { 0.0 };
        assert!((m.bounds.min.z - want).abs() < 1e-6, "model {id} put down at {want}: {:?}", m.bounds);
        for o in &ids[..k] {
            let g = footprint_of(&s, *o);
            assert!(f.min[0] >= g.max[0] || g.min[0] >= f.max[0] || f.min[1] >= g.max[1] || g.min[1] >= f.max[1], "{f:?} apart from {g:?}");
        }
    }
    // Arranging is part of the import: one undo step takes the model away.
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.scene.models().len(), 2);

    // Without `arrange`, the file's coordinates are kept.
    let r = s.execute("file.import_stl", &json!({ "path": path })).unwrap();
    assert!(r.get("on_plate").is_none());
    let b = s.scene.model(r["id"].as_u64().unwrap()).unwrap().bounds;
    assert_eq!((b.min, b.max), (Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0)));
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn arranging_lays_a_long_side_along_the_plate_unless_supported() {
    let mut s = Session::new();
    let long = s.scene.add("long".into(), boxes(&[([100.0, 50.0, 7.0], [110.0, 110.0, 12.0])]));
    let r = s.execute("model.arrange", &json!({})).unwrap();
    assert_eq!(r["beside"], json!([]));
    let m = s.scene.model(long).unwrap();
    assert_eq!(m.placement.rotation.z.abs(), 90.0);
    let (b, size) = (m.bounds, m.bounds.size());
    assert!((size.x - 60.0).abs() < 1e-6 && (size.y - 10.0).abs() < 1e-6, "turned: {b:?}");
    assert!(b.center().x.abs() < 1e-6 && b.center().y.abs() < 1e-6 && b.min.z.abs() < 1e-6, "centred and put down: {b:?}");
    // Arranged already: a second run changes nothing and makes no step.
    let depth = s.undo_depth();
    s.execute("model.arrange", &json!({})).unwrap();
    assert_eq!(s.undo_depth(), depth);

    // Off-centre on its pivot and turned so the quarter turn back is the shorter one: the bounds
    // still land on the cell.
    let mut s = Session::new();
    let l = s.scene.add("l".into(), boxes(&[([0.0, 0.0, 0.0], [30.0, 10.0, 5.0]), ([0.0, 0.0, 0.0], [10.0, 60.0, 5.0])]));
    s.execute("model.rotate", &json!({ "ids": [l], "degrees": [0, 0, 30] })).unwrap();
    s.execute("model.arrange", &json!({})).unwrap();
    let m = s.scene.model(l).unwrap();
    assert!((m.placement.rotation.z - -60.0).abs() < 1e-9, "{:?}", m.placement);
    let c = m.bounds.center();
    assert!(c.x.abs() < 1e-6 && c.y.abs() < 1e-6, "on the cell: {:?}", m.bounds);

    let mut s = Session::new();
    let held = s.scene.add("held".into(), boxes(&[([0.0, 0.0, 7.0], [10.0, 60.0, 12.0])]));
    s.execute("support.add", &json!({ "model": held, "point": [5, 30, 7] })).unwrap();
    s.execute("model.arrange", &json!({ "ids": [held] })).unwrap();
    let m = s.scene.model(held).unwrap();
    assert_eq!(m.placement.rotation.z, 0.0, "a model with supports is only moved");
    assert!(on(&footprint_of(&s, held), &plate_of(&s)));
}

/// Two 90 × 47 mm models imported one after the other on the 219 × 123 mm plate: the second fits
/// only turned, beside the first.
#[test]
fn a_model_that_fits_only_turned_is_turned_beside_the_first() {
    let mut s = Session::new();
    let first = s.scene.add("first".into(), boxes(&[([0.0, 0.0, 0.0], [90.0, 47.0, 70.0])]));
    s.execute("model.arrange", &json!({ "ids": [first] })).unwrap();
    let second = s.scene.add("second".into(), boxes(&[([0.0, 0.0, 0.0], [90.0, 47.0, 70.0])]));
    let r = s.execute("model.arrange", &json!({ "ids": [second] })).unwrap();
    assert_eq!(r["beside"], json!([]));
    let (a, b) = (footprint_of(&s, first), footprint_of(&s, second));
    assert_eq!(s.scene.model(second).unwrap().placement.rotation.z.abs(), 90.0);
    assert!(on(&b, &plate_of(&s)), "{b:?}");
    assert!(b.min[0] >= a.max[0] || a.min[0] >= b.max[0], "beside the first: {a:?} {b:?}");
}

#[test]
fn a_model_too_big_for_the_plate_goes_beside_it() {
    let mut s = Session::new();
    let small = s.scene.add("small".into(), boxes(&[([0.0, 0.0, 0.0], [10.0, 10.0, 10.0])]));
    let big = s.scene.add("big".into(), boxes(&[([0.0, 0.0, 0.0], [300.0, 20.0, 10.0])]));
    let r = s.execute("model.arrange", &json!({})).unwrap();
    assert_eq!(r["beside"], json!([big]));
    let plate = plate_of(&s);
    assert!(footprint_of(&s, big).min[0] > plate.max[0], "right of the plate");
    assert!(on(&footprint_of(&s, small), &plate));
    assert!(matches!(s.execute("model.arrange", &json!({ "ids": [999] })), Err(EngineError::BadParams(_))));
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
        json!({ "arrange": "yes", "ids": [1, 1, 2] }),
        json!({ "tip": { "length": [], "penetration": {} }, "base": { "diameter": 0, "height": 1e-300 }, "raft": { "chamfer_angle": "45" } }),
        json!({ "printer": { "name": 1, "resolution_x": 1e308, "display_x": -1, "mirror_x": "yes" }, "layers": [], "slicing": "aa" }),
        json!({ "printer": { "resolution_x": u64::MAX, "resolution_y": 0.5, "display_y": 1e-300 }, "exposure": { "power": 256, "normal": null } }),
        json!({ "layers": { "height": 1e-300, "bottom_count": u64::MAX, "transition_count": -1 }, "motion": { "lift_speed": 1e308 } }),
        json!({ "layers": { "bottom_count": 200, "transition_count": 200 }, "printer": { "build_height": 1 }, "estimates": { "bottle_cost": -0.0 } }),
        json!({ "slicing": { "anti_aliasing": 3 }, "estimates": { "resin_density": 1e308, "bottle_volume": "1" }, "printer": { "name": "\u{0}" } }),
        json!({ "printer": { "name": "x".repeat(10_000), "display_x": 1, "resolution_x": 32768 }, "from_the_future": {} }),
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
    // Something for `edit.paste` to paste.
    s.execute("edit.copy", &json!({ "ids": [a] })).unwrap();
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
        "edit.copy" => Some(json!({ "ids": [b] })),
        "edit.paste" => Some(json!({})),
        "edit.redo" | "edit.undo" => None,
        "file.import_stl" => Some(json!({ "path": f.path, "fit": false, "arrange": true })),
        "file.open" => None,
        "file.save" => Some(json!({ "path": f.project.with_file_name("saved.rsup") })),
        "model.arrange" => Some(json!({})),
        "model.delete" => Some(json!({ "id": a })),
        "model.lift" => Some(json!({ "lifted": true })),
        "model.list" | "support.list" | "scene.clear" => Some(json!({})),
        "scene.lift_height" => Some(json!({ "height": 8 })),
        "model.move" => Some(json!({ "by": [1, 2, 3] })),
        "model.place" => Some(json!({ "models": [{ "id": b, "position": [0, 40, 5], "rotation": [0, 0, 30], "scale": [2, 1, 1] }] })),
        "model.rotate" => Some(json!({ "degrees": [0, 90, 0] })),
        "model.scale" => Some(json!({ "factors": 1.5 })),
        "model.select" => Some(json!({ "ids": [b] })),
        "print.configure" => {
            Some(json!({ "printer": { "name": "Other", "mirror_x": true }, "layers": { "height": 0.03 }, "slicing": { "anti_aliasing": 4 } }))
        }
        "print.reset" | "print.settings" | "print.slice" => Some(json!({})),
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

/// A copied model pastes with its supports and lift, beside the others on the plate and selected,
/// as one step; still after the original is deleted, and as often as asked.
#[test]
fn copied_models_paste_with_their_supports_beside_the_others() {
    let dir = temp_dir("paste");
    let path = write_cube_stl(&dir);
    let mut s = Session::new();
    assert!(s.execute("edit.paste", &json!({})).is_err(), "nothing copied yet");
    assert!(s.execute("edit.copy", &json!({})).is_err(), "nothing selected");
    let a = s.execute("file.import_stl", &json!({ "path": path, "arrange": true })).unwrap()["id"].as_u64().unwrap();
    s.execute("model.lift", &json!({ "lifted": true, "ids": [a] })).unwrap();
    let tip_at = s.scene.model(a).unwrap().bounds.min;
    s.execute("support.add", &json!({ "model": a, "point": [tip_at.x + 5.0, tip_at.y + 5.0, tip_at.z] })).unwrap();
    s.execute("model.select", &json!({ "ids": [a] })).unwrap();
    let depth = s.undo_depth();
    assert_eq!(s.execute("edit.copy", &json!({})).unwrap()["copied"], json!(1));
    assert_eq!(s.undo_depth(), depth, "copying is no step");
    assert!(s.can_paste());

    let r = s.execute("edit.paste", &json!({})).unwrap();
    assert_eq!(s.undo_depth(), depth + 1);
    let ids: Vec<u64> = r["ids"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap()).collect();
    let [copy] = ids[..] else { panic!("{r}") };
    assert_eq!(r["beside"], json!([]));
    assert_eq!(s.scene.selected_models.iter().copied().collect::<Vec<_>>(), vec![copy]);
    let (m, orig) = (s.scene.model(copy).unwrap(), s.scene.model(a).unwrap());
    assert!(m.lifted && std::sync::Arc::ptr_eq(&m.mesh, &orig.mesh) && m.name == orig.name);
    assert!((m.bounds.min.z - s.scene.lift_height()).abs() < 1e-9, "at the lift height: {:?}", m.bounds);
    let (fa, fc, plate) = (footprint_of(&s, a), footprint_of(&s, copy), plate_of(&s));
    assert!(on(&fc, &plate), "{fc:?}");
    assert!(fc.min[0] >= fa.max[0] || fa.min[0] >= fc.max[0] || fc.min[1] >= fa.max[1] || fa.min[1] >= fc.max[1], "apart: {fa:?} {fc:?}");
    let tips: Vec<&crate::Support> = s.scene.supports().iter().filter(|t| t.model == copy).collect();
    let [t] = tips[..] else { panic!("one support on the copy: {:?}", s.scene.supports()) };
    let shift = m.placement.position - orig.placement.position;
    let want = s.scene.supports().iter().find(|t| t.model == a).unwrap().tip + shift;
    assert!((t.tip - want).len() < 1e-6, "the tip moved with the copy: {:?} {want:?}", t.tip);

    s.execute("edit.undo", &json!({})).unwrap();
    assert!(s.scene.model(copy).is_none() && s.scene.supports().len() == 1);

    // The clipboard holds the model itself: deleted, it still pastes, and again.
    s.execute("model.delete", &json!({ "id": a })).unwrap();
    let first = s.execute("edit.paste", &json!({})).unwrap()["ids"][0].as_u64().unwrap();
    let second = s.execute("edit.paste", &json!({})).unwrap()["ids"][0].as_u64().unwrap();
    assert_eq!(s.scene.models().len(), 2);
    assert_eq!(s.scene.supports().len(), 2);
    let (f1, f2) = (footprint_of(&s, first), footprint_of(&s, second));
    assert!(f1.min[0] >= f2.max[0] || f2.min[0] >= f1.max[0] || f1.min[1] >= f2.max[1] || f2.min[1] >= f1.max[1], "apart: {f1:?} {f2:?}");
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

#[test]
fn print_config_merges_validates_and_round_trips() {
    use crate::PrintConfig;
    use resin_slice::{PrintSettings, PrinterProfile};
    let mut s = Session::new();
    let all = s.execute("print.settings", &json!({})).unwrap();
    assert_eq!(all["printer"]["name"], "ELEGOO SATURN 2");
    assert_eq!(all["printer"]["resolution_x"], 7680);
    assert_eq!(all["layers"]["height"], 0.05);
    assert_eq!(all["exposure"]["bottom"], 30.0);
    // What the command lists is what storage reads back, unchanged.
    assert_eq!(PrintConfig::from_stored(&all), (s.print_config().clone(), vec![]));

    // A partial patch changes only what it names.
    s.execute("print.configure", &json!({ "exposure": { "normal": 2.5 } })).unwrap();
    let mut expected = PrintConfig::default();
    expected.settings.exposure.normal = 2.5;
    assert_eq!(*s.print_config(), expected);
    s.execute("print.configure", &json!({ "printer": { "name": "Mars 4", "mirror_y": true, "display_x": 200 }, "slicing": { "anti_aliasing": 8 } }))
        .unwrap();
    let c = s.print_config();
    assert_eq!((c.printer.name.as_str(), c.printer.mirror_y, c.printer.display_x), ("Mars 4", true, 200.0));
    assert_eq!((c.printer.mirror_x, c.printer.display_y), (false, 123.12));
    assert_eq!((c.settings.slicing.anti_aliasing, c.settings.exposure.normal), (8, 2.5));

    // A bad patch leaves everything as it was and names the field.
    let before = s.print_config().clone();
    for (bad, names) in [
        (json!({ "exposure": { "normal": 0 } }), "exposure.normal"),
        (json!({ "exposure": { "normal": 2.0, "bottom": 1e9 } }), "exposure.bottom"),
        (json!({ "exposure": { "power": 255.5 } }), "exposure.power"),
        (json!({ "layers": { "height": 0.5 } }), "layers.height"),
        (json!({ "layers": { "bottom_count": -1 } }), "layers.bottom_count"),
        (json!({ "slicing": { "anti_aliasing": 3 } }), "slicing.anti_aliasing"),
        (json!({ "estimates": { "bottle_cost": -1 } }), "estimates.bottle_cost"),
        (json!({ "estimates": { "resin_density": "1.1" } }), "estimates.resin_density"),
        (json!({ "printer": { "resolution_x": 0 } }), "printer.resolution_x"),
        (json!({ "printer": { "mirror_x": 1 } }), "printer.mirror_x"),
        (json!({ "printer": { "name": "n".repeat(65) } }), "printer.name"),
        (json!({ "printer": { "colour": "red" } }), "printer.colour"),
        (json!({ "layers": "thin" }), "layers"),
        (json!({ "elevation": 5 }), "elevation"),
        // Each fits alone; together they don't: 2 mm hold 40 layers of 0.05 mm.
        (json!({ "printer": { "build_height": 2 }, "layers": { "bottom_count": 40, "transition_count": 1 } }), "build height"),
        (json!({ "printer": { "display_x": 1, "resolution_x": 32768 } }), "µm"),
    ] {
        match s.execute("print.configure", &bad) {
            Err(EngineError::BadParams(why)) => assert!(why.contains(names), "{bad}: {why}"),
            other => panic!("{bad}: {other:?}"),
        }
        assert_eq!(*s.print_config(), before, "{bad}");
    }
    assert!(matches!(s.execute("print.configure", &json!([1])), Err(EngineError::BadParams(_))));
    assert!(
        s.set_print_config(PrintConfig { printer: PrinterProfile { build_height: f64::NAN, ..PrinterProfile::default() }, ..before.clone() })
            .is_err()
    );
    assert_eq!(*s.print_config(), before);

    // Restore defaults: the print settings only; the printer stays.
    s.execute("print.reset", &json!({})).unwrap();
    assert_eq!(s.print_config().settings, PrintSettings::default());
    assert_eq!(s.print_config().printer, before.printer);
}

#[test]
fn stored_print_config_falls_back_field_by_field() {
    use crate::PrintConfig;
    use resin_slice::{PrintSettings, PrinterProfile};
    let stored = json!({
        "printer": { "name": "Mine", "resolution_x": "many", "mirror_x": true, "from_the_future": 1 },
        "layers": { "height": 0.1, "bottom_count": 1e9 },
        "exposure": { "normal": f64::NAN, "bottom": 20 },
        "motion": 7,
        "extra": true,
    });
    let (c, mut problems) = PrintConfig::from_stored(&stored);
    problems.sort();
    assert_eq!(problems, ["exposure.normal", "layers.bottom_count", "motion", "printer.resolution_x"]);
    let (p, d) = (PrinterProfile::default(), PrintSettings::default());
    assert_eq!((c.printer.name.as_str(), c.printer.mirror_x, c.printer.resolution_x), ("Mine", true, p.resolution_x));
    assert_eq!((c.settings.layers.height, c.settings.layers.bottom_count), (0.1, d.layers.bottom_count));
    assert_eq!((c.settings.exposure.normal, c.settings.exposure.bottom), (d.exposure.normal, 20.0));
    assert_eq!(c.settings.motion, d.motion);
    assert!(c.validate().is_ok());

    // Fields that read but don't fit together: the layers' group, then the printer's.
    let crowded = json!({ "printer": { "build_height": 1 }, "layers": { "bottom_count": 100, "height": 0.01 } });
    let (c, problems) = PrintConfig::from_stored(&crowded);
    assert_eq!(problems, ["layers"]);
    assert_eq!((c.printer.build_height, c.settings.layers), (1.0, d.layers));
    let tiny = json!({ "printer": { "display_x": 1, "resolution_x": 32768, "name": "Tiny" } });
    let (c, problems) = PrintConfig::from_stored(&tiny);
    assert_eq!((problems, c.printer), (vec!["printer".to_string()], p.clone()));

    // Through serde too, which is how preferences read it; nothing makes it fail.
    let c2: PrintConfig = serde_json::from_value(stored.clone()).unwrap();
    assert_eq!(c2, PrintConfig::from_stored(&stored).0);
    assert_eq!(serde_json::from_value::<PrintConfig>(json!("junk")).unwrap(), PrintConfig::default());
    assert_eq!(PrintConfig::from_stored(&json!(3)).1, ["print_config"]);
}

#[test]
fn print_config_is_never_an_undo_step() {
    let dir = temp_dir("print_undo");
    let f = fixture(&dir);
    let mut s = f.session;
    s.execute("support.add", &json!({ "model": f.a, "point": [2, 2, 0] })).unwrap();
    s.execute("edit.undo", &json!({})).unwrap();
    let (undo, redo, revision) = (s.undo_depth(), s.redo_depth(), s.scene.revision());
    s.execute("print.configure", &json!({ "layers": { "height": 0.025 } })).unwrap();
    s.execute("print.reset", &json!({})).unwrap();
    s.execute("print.configure", &json!({ "exposure": { "normal": 4 } })).unwrap();
    assert_eq!((s.undo_depth(), s.redo_depth(), s.scene.revision()), (undo, redo, revision), "no step, redo kept, scene untouched");
    let config = s.print_config().clone();
    s.execute("edit.redo", &json!({})).unwrap();
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(*s.print_config(), config, "undo leaves the print settings alone");
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

#[test]
fn supports_reach_the_plate_in_the_slice() {
    let dir = temp_dir("slice");
    let path = write_cube_stl(&dir);
    let mut s = Session::new();
    let m = s.execute("file.import_stl", &json!({ "path": path })).unwrap()["id"].as_u64().unwrap();
    // On the plate, alone: its 10 mm.
    let alone = s.execute("print.slice", &json!({})).unwrap();
    assert_eq!(alone["layers"], 200);
    assert_eq!(alone["unreachable_supports"], 0);
    // Lifted 5 mm and held up by a support from the plate: 15 mm.
    s.execute("model.lift", &json!({ "lifted": true, "ids": [m] })).unwrap();
    let bottom = s.scene.model(m).unwrap().bounds.min;
    s.execute("support.add", &json!({ "model": m, "point": [bottom.x + 5.0, bottom.y + 5.0, bottom.z], "normal": [0, 0, -1] })).unwrap();
    let held = s.execute("print.slice", &json!({})).unwrap();
    assert!(held["layers"].as_u64().unwrap() > alone["layers"].as_u64().unwrap());
    assert_eq!(held["layers"], 300);
    assert!(held["volume_ml"].as_f64().unwrap() > alone["volume_ml"].as_f64().unwrap(), "the support is printed");
    assert_eq!((held["clipped"].as_u64(), held["open_rows"].as_u64()), (Some(0), Some(0)));
    // The first layers: empty without the support, lit with it.
    let first_lit = s.job().unwrap().sliced.layers[0].lit;
    assert!(first_lit > 0, "the support's base is on the plate");
    assert!(s.job_is_current());
    // Moved off the plate's edge, the cube is clipped.
    s.execute("model.move", &json!({ "ids": [m], "by": [200, 0, 0] })).unwrap();
    assert!(!s.job_is_current(), "a scene change makes the job stale");
    let off = s.execute("print.slice", &json!({})).unwrap();
    assert!(off["clipped"].as_u64().unwrap() >= 1);
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn a_stale_job_is_refused() {
    let dir = temp_dir("stale");
    let path = write_cube_stl(&dir);
    let mut s = Session::new();
    let m = s.execute("file.import_stl", &json!({ "path": path })).unwrap()["id"].clone();
    let (input, unreachable) = s.slice_input();
    let job = resin_slice::slice(&input, &Default::default(), &Default::default()).unwrap();
    // Each of the three a job depends on, changed while it was sliced.
    let mut moved = s.clone();
    moved.execute("model.move", &json!({ "by": [1, 0, 0], "ids": [m] })).unwrap();
    assert!(!moved.adopt_job(job.clone(), unreachable));
    let mut reprinted = s.clone();
    reprinted.execute("print.configure", &json!({ "exposure": { "normal": 4 } })).unwrap();
    assert!(!reprinted.adopt_job(job.clone(), unreachable));
    let mut resupported = s.clone();
    resupported.execute("support.configure", &json!({ "body": { "diameter": 1.2 } })).unwrap();
    assert!(!resupported.adopt_job(job.clone(), unreachable));
    assert!(moved.job().is_none() && !moved.job_is_current());
    assert!(s.adopt_job(job, unreachable));
    assert!(s.job_is_current());
    std::fs::remove_dir_all(dir).ok();
}
