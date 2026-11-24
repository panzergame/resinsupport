//! Selecting models, in Object mode: a left press on a model selects it (Shift toggles it),
//! elsewhere clears the selection and may start a box (`box_select`). In Slice mode one model at
//! a time, for its own slicing settings: a press selects the model under it alone, or none.
//! A model, its supports and its raft are one object: a press on a support or the raft picks
//! its model, one on a brace the models of both its supports. Through `model.select`.

use resin_engine::geom::Vec3;
use serde_json::json;

use crate::navigation::Proj;
use crate::supports::picking_press;
use crate::{EditMode, ResinApp};

pub fn handle(app: &mut ResinApp, ui: &egui::Ui, proj: &Proj, resp: &egui::Response) {
    let Some((at, mods)) = picking_press(app, ui, resp) else { return };
    let (o, d) = proj.ray(at);
    if app.mode == EditMode::Slice {
        let [id] = object_hit(app, o, d, true)[..] else { return pick_one(app, None) };
        pick_one(app, Some(id));
        return;
    }
    let ids = object_hit(app, o, d, false);
    match ids.is_empty() {
        false => {
            app.run("model.select", json!({ "ids": ids, "mode": if mods.shift { "toggle" } else { "replace" } }));
        }
        true => {
            if !mods.shift && !app.session.scene.selected_models.is_empty() {
                app.run("model.select", json!({ "ids": [] }));
            }
            crate::box_select::press(app, at, mods);
        }
    }
}

/// The models the nearest object along the ray from `o` along unit `d` stands for: a model, a
/// support's or a raft's model, or the models of a brace's two supports. With `one`, a brace between two
/// models is passed through, to what lies behind it.
fn object_hit(app: &ResinApp, o: Vec3, d: Vec3, one: bool) -> Vec<u64> {
    let scene = &app.session.scene;
    let preset = *app.session.support_settings().current();
    let model_of = |id: u64| scene.supports().iter().find(|s| s.id == id).map(|s| s.model);
    let surface = scene.ray_hit(o, d).map(|(id, h)| (vec![id], h.t));
    let support = app.routes.piece_hit(scene, &preset, o, d, true).and_then(|(id, _, t)| Some((vec![model_of(id)?], t)));
    let raft = app.routes.raft_hit(scene, o, d).map(|(id, t)| (vec![id], t));
    let brace = app.routes.brace_hits(scene, &preset, o, d).filter_map(|(id, t)| {
        let b = scene.braces().iter().find(|b| b.id == id)?;
        let mut ids: Vec<u64> = [b.a, b.b].into_iter().filter_map(model_of).collect();
        ids.sort_unstable();
        ids.dedup();
        let fits = if one { ids.len() == 1 } else { !ids.is_empty() };
        fits.then_some((ids, t))
    });
    surface.into_iter().chain(support).chain(raft).chain(brace).min_by(|a, b| a.1.total_cmp(&b.1)).map(|(ids, _)| ids).unwrap_or_default()
}

/// Select `id` alone, or nothing; selecting what is already selected runs nothing.
pub fn pick_one(app: &mut ResinApp, id: Option<u64>) {
    let selected = &app.session.scene.selected_models;
    let ids: Vec<u64> = id.into_iter().collect();
    if selected.len() != ids.len() || !ids.iter().all(|i| selected.contains(i)) {
        app.run("model.select", json!({ "ids": ids }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prefs::Prefs;

    /// Two flat 10 mm triangles 12 mm apart, lifted 15 mm, a support under each joined by a brace.
    #[test]
    fn a_support_picks_its_model_and_a_brace_both() {
        let dir = std::env::temp_dir().join(format!("resin-ui-objects-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let stl = dir.join("tri.stl");
        std::fs::write(&stl, "solid t\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 10 0 0\nvertex 0 10 0\nendloop\nendfacet\nendsolid t\n")
            .unwrap();
        let mut app = ResinApp::build(Prefs::default(), &[]);
        app.run("scene.lift_height", json!({ "height": 15 })).unwrap();
        let import =
            |app: &mut ResinApp| app.run("file.import_stl", json!({ "path": stl, "fit": false, "lifted": true })).unwrap()["id"].as_u64().unwrap();
        let (a, b) = (import(&mut app), import(&mut app));
        std::fs::remove_dir_all(dir).ok();
        app.run("model.move", json!({ "ids": [b], "by": [12, 0, 0] })).unwrap();
        let add = |app: &mut ResinApp, model: u64, x: f64| {
            app.run("support.add", json!({ "model": model, "point": [x, 1, 15], "normal": [0, 0, -1] })).unwrap()["id"].as_u64().unwrap()
        };
        let supports = [add(&mut app, a, 8.0), add(&mut app, b, 13.0)];
        let made = app.run("support.add_braces", json!({ "ids": supports })).unwrap();
        assert_eq!(made["ids"].as_array().map(Vec::len), Some(1), "{made}");
        let preset = *app.session.support_settings().current();
        while app.routes.update(&app.session.scene, &preset) {}

        let along_y = |x: f64, z: f64| (Vec3::new(x, -50.0, z), Vec3::new(0.0, 1.0, 0.0));
        let (o, d) = along_y(8.0, 10.0);
        assert_eq!(object_hit(&app, o, d, false), vec![a], "the first support's body");
        let (o, d) = along_y(30.0, 10.0);
        assert!(object_hit(&app, o, d, false).is_empty(), "nothing there");

        // Between the supports, where only the brace's struts are.
        let on_brace = (10..150).map(|i| along_y(10.5, f64::from(i) / 10.0)).find(|&(o, d)| {
            let scene = &app.session.scene;
            app.routes.brace_hits(scene, &preset, o, d).next().is_some() && app.routes.piece_hit(scene, &preset, o, d, true).is_none()
        });
        let (o, d) = on_brace.unwrap();
        assert_eq!(object_hit(&app, o, d, false), vec![a, b]);
        assert!(object_hit(&app, o, d, true).is_empty(), "one model at a time: through the brace");

        // Down onto the second support's raft, beside its base.
        app.routes.wait_raft();
        let foot = app.routes.route(supports[1]).and_then(|r| r.body.last().copied()).unwrap();
        let x = foot.x + preset.base.diameter / 2.0 + preset.raft.margin / 2.0;
        let (o, d) = (Vec3::new(x, foot.y, 1.0), Vec3::new(0.0, 0.0, -1.0));
        assert!(app.routes.raft_hit(&app.session.scene, o, d).is_some(), "the raft is there");
        assert_eq!(object_hit(&app, o, d, false), vec![b], "the raft picks its model");

        // A press on the first support in Slice mode selects its model alone.
        app.mode = EditMode::Slice;
        app.run("model.select", json!({ "ids": [b] })).unwrap();
        let (o, d) = along_y(8.0, 10.0);
        let [id] = object_hit(&app, o, d, true)[..] else { panic!("one model") };
        pick_one(&mut app, Some(id));
        assert_eq!(app.session.scene.selected_models.iter().copied().collect::<Vec<_>>(), vec![a]);
    }
}
