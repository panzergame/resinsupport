//! Selecting models, in Object mode: a left press on a model selects it (Shift toggles it),
//! elsewhere clears the selection and may start a box (`box_select`). In Slice mode one model at
//! a time, for its own slicing settings: a press selects the model under it alone, or none.
//! Through `model.select`.

use serde_json::json;

use crate::navigation::Proj;
use crate::supports::picking_press;
use crate::{EditMode, ResinApp};

pub fn handle(app: &mut ResinApp, ui: &egui::Ui, proj: &Proj, resp: &egui::Response) {
    let Some((at, mods)) = picking_press(app, ui, resp) else { return };
    let (o, d) = proj.ray(at);
    if app.mode == EditMode::Slice {
        pick_one(app, app.session.scene.ray_hit(o, d).map(|(id, _)| id));
        return;
    }
    match app.session.scene.ray_hit(o, d) {
        Some((id, _)) => {
            app.run("model.select", json!({ "ids": [id], "mode": if mods.shift { "toggle" } else { "replace" } }));
        }
        None => {
            if !mods.shift && !app.session.scene.selected_models.is_empty() {
                app.run("model.select", json!({ "ids": [] }));
            }
            crate::box_select::press(app, at, mods);
        }
    }
}

/// Select `id` alone, or nothing; selecting what is already selected runs nothing.
pub fn pick_one(app: &mut ResinApp, id: Option<u64>) {
    let selected = &app.session.scene.selected_models;
    let ids: Vec<u64> = id.into_iter().collect();
    if selected.len() != ids.len() || !ids.iter().all(|i| selected.contains(i)) {
        app.run("model.select", json!({ "ids": ids }));
    }
}
