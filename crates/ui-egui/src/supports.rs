//! Placing support tips by hand, in Support mode: the pointer over a model shows where a tip
//! would go, a left press places it, a left press on a placed tip selects it (Shift toggles), Delete removes the
//! selection. Every change goes through the `support.*` commands.

use egui::Pos2;
use resin_engine::Support;
use resin_engine::geom::{Mesh, Vec3};
use resin_engine::render::Camera;
use serde_json::json;

use crate::ResinApp;
use crate::navigation::Proj;

/// What is under the pointer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Hover {
    /// A model's surface, where a tip would go.
    Surface { model: u64, point: Vec3, normal: Vec3 },
    /// A placed tip.
    Support(u64),
}

/// The pointer's pick, kept until the pointer, the camera or the scene changes: a ray cast per
/// frame while nothing moves would be wasted.
#[derive(Default)]
pub struct SupportTool {
    pub hover: Option<Hover>,
    key: Option<(Pos2, Camera, egui::Rect, u64)>,
}

impl SupportTool {
    /// Forget the pick, as when leaving Support mode.
    pub fn clear(&mut self) {
        self.hover = None;
        self.key = None;
    }
}

/// The pick and the clicks for one frame over the viewport. Runs after `navigation::handle`, so
/// a navigating press is known.
pub fn handle(app: &mut ResinApp, ui: &egui::Ui, proj: &Proj, resp: &egui::Response) {
    // A held right button may still become a click: the preview stays until it orbits.
    let busy = ui.input(|i| i.pointer.primary_down() || i.pointer.middle_down() || i.pointer.is_decidedly_dragging() || i.key_down(egui::Key::Space));
    let pointer = resp.hover_pos().filter(|_| !busy && ui.is_enabled());
    match pointer {
        Some(p) => {
            let key = Some((p, proj.cam, proj.rect, app.session.scene.revision()));
            if app.supports.key != key {
                app.supports.hover = pick(app, proj, p);
                app.supports.key = key;
            }
        }
        None => app.supports.clear(),
    }
    if matches!(app.supports.hover, Some(Hover::Support(_))) {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    let Some((at, mods)) = picking_press(app, ui, resp) else { return };
    match pick(app, proj, at) {
        Some(Hover::Surface { model, point, normal }) => {
            if !mods.shift && !app.session.scene.selected.is_empty() {
                app.run("support.select", json!({ "ids": [] }));
            }
            app.run("support.add", json!({ "model": model, "point": point, "normal": normal }));
        }
        Some(Hover::Support(id)) => {
            app.run("support.select", json!({ "ids": [id], "mode": if mods.shift { "toggle" } else { "replace" } }));
        }
        _ if !mods.shift && !app.session.scene.selected.is_empty() => {
            app.run("support.select", json!({ "ids": [] }));
        }
        _ => {}
    }
    app.supports.key = None;
}

/// A left press over the viewport that picks rather than navigates: where it happened and the
/// modifiers held. Acting on the press rather than egui's click: a hand still moving when it
/// clicks travels past `max_click_dist` before the release, and egui then calls it a drag, so
/// the click was lost. The press is picked where it happened, not where the last hover pick was.
pub(crate) fn picking_press(app: &ResinApp, ui: &egui::Ui, resp: &egui::Response) -> Option<(Pos2, egui::Modifiers)> {
    let (pressed_at, mods, space) =
        ui.input(|i| (i.pointer.press_origin().filter(|_| i.pointer.primary_pressed()), i.modifiers, i.key_down(egui::Key::Space)));
    let at = pressed_at.filter(|_| resp.contains_pointer() && ui.is_enabled() && !app.gizmo_grab)?;
    let navigates = app.nav.primary_navigates() || space || mods.ctrl || mods.command || mods.alt;
    (!navigates).then_some((at, mods))
}

/// A placed tip wins over the surface it sits in when the ray reaches it first, or within its
/// radius past the surface (it is half buried).
fn pick(app: &ResinApp, proj: &Proj, p: Pos2) -> Option<Hover> {
    let (o, d) = proj.ray(p);
    let scene = &app.session.scene;
    let surface = scene.ray_hit(o, d);
    let tip = scene.support_hit(o, d);
    let radius = Support::TIP_DIAMETER / 2.0;
    match (surface, tip) {
        (Some((_, h)), Some((id, t))) if t <= h.t + radius => Some(Hover::Support(id)),
        (None, Some((id, _))) => Some(Hover::Support(id)),
        (Some((model, h)), _) => {
            let tri = scene.model(model).and_then(|m| m.mesh.triangles.get(h.triangle));
            let normal = tri.and_then(Mesh::face_normal).unwrap_or(-d);
            Some(Hover::Surface { model, point: h.point, normal })
        }
        (None, None) => None,
    }
}
