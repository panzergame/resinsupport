//! Selecting models, in Object mode: a left press on a model selects it (Shift toggles it),
//! elsewhere clears the selection and may start a box (`box_select`). Through `model.select`.

use serde_json::json;

use crate::ResinApp;
use crate::navigation::Proj;
use crate::supports::picking_press;

pub fn handle(app: &mut ResinApp, ui: &egui::Ui, proj: &Proj, resp: &egui::Response) {
    let Some((at, mods)) = picking_press(app, ui, resp) else { return };
    let (o, d) = proj.ray(at);
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
