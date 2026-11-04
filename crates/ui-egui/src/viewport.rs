//! The 3D view: models, origin grid and axes, the axis gizmo, and camera input.

use std::sync::Arc;

use egui::{Align2, FontId, Pos2, Rect, Sense, Stroke, pos2, vec2};
use resin_engine::geom::Vec3;
use resin_engine::render::Overlay;

use crate::gpu::{ModelDraw, Sphere, ViewFrame, ViewportCallback};
use crate::support_routes::SupportMesh;
use crate::supports::Hover;
use crate::theme::{Tokens, linear};
use crate::{EditMode, ResinApp, gizmo, i18n, navigation, objects, supports, transform};

pub fn show(app: &mut ResinApp, ui: &mut egui::Ui, tokens: &Tokens) {
    let rect = ui.available_rect_before_wrap();
    let resp = ui.allocate_rect(rect, Sense::click_and_drag());
    app.viewport_rect = Some(rect);
    app.step_view_animation(ui.ctx());
    // The models are drawn into this slot once the gizmo has moved them, and the gizmo over them.
    let painter = ui.painter_at(rect);
    let scene_slot = painter.add(egui::Shape::Noop);
    let gizmo_id = egui::Id::new("transform_gizmo");
    // A press on a gizmo handle belongs to it: neither the camera nor picking may take it.
    let pressed_triad = triad(app);
    let pressed_on = ui.input(|i| i.pointer.press_origin().filter(|_| i.pointer.primary_pressed()));
    let proj = navigation::Proj { cam: app.session.camera, rect };
    app.gizmo_grab = gizmo::dragging(ui.ctx(), gizmo_id)
        || pressed_triad.zip(pressed_on).is_some_and(|(t, p)| resp.contains_pointer() && gizmo::contains_pointer(&proj, &t, p));
    navigation::handle(app, ui, rect, &resp);
    let proj = navigation::Proj { cam: app.session.camera, rect };
    if let Some(t) = triad(app) {
        let free = ui.input(|i| i.modifiers.ctrl || i.modifiers.command || i.modifiers.alt);
        let step = gizmo::snap_step(app.session.camera.half_height());
        if let Some(g) = gizmo::show(ui, &painter, &proj, gizmo_id, &t, step, free, tokens) {
            match app.mode {
                EditMode::Object => transform::drag(app, g),
                EditMode::Support => supports::drag(app, g),
            }
        }
    } else {
        gizmo::cancel(ui.ctx(), gizmo_id);
    }
    match app.mode {
        EditMode::Object => {
            app.supports.clear();
            objects::handle(app, ui, &proj, &resp);
        }
        EditMode::Support => supports::handle(app, ui, &proj, &resp),
    }

    let cam = app.session.camera;
    let aspect = f64::from(rect.width().max(1.0)) / f64::from(rect.height().max(1.0));
    let overlay = Overlay::build(&cam, aspect, &tokens.overlay());
    let r = overlay.radius;
    let preset = *app.session.support_settings().current();
    if app.routes.update(&app.session.scene, &preset) {
        ui.ctx().request_repaint();
    }
    let support_mesh = app.routes.mesh(&app.session.scene, &preset);
    let colors = [tokens.support, tokens.support_selected, tokens.support_unreachable].map(linear);
    let spheres = support_spheres(app, &support_mesh, &colors, tokens);
    let handles = joint_handles(app, &support_mesh, tokens);
    let mut bounds = app.session.scene.bounds().union(&support_mesh.bounds);
    for s in spheres.iter().chain(&handles) {
        let r = f64::from(s.radius);
        bounds.add(s.center - Vec3::new(r, r, r));
        bounds.add(s.center + Vec3::new(r, r, r));
    }
    bounds.add(overlay.center + Vec3::new(-r, -r, 0.0));
    bounds.add(overlay.center + Vec3::new(r, r, overlay.step * 10.0));
    let (right, up, back) = cam.basis();
    // A key light from the viewer's upper left, so faces read apart; a fill from the eye.
    let light = (right * -0.45 + up * 0.65 + back * 0.6).normalized().unwrap_or(back);
    let frame = ViewFrame {
        view_proj: cam.view_proj_bounds(aspect, &bounds).to_f32(),
        light,
        back,
        bg_top: linear(tokens.viewport_top),
        bg_bottom: linear(tokens.viewport_bottom),
        grid_center: overlay.center,
        grid_radius: overlay.radius,
        lines: overlay.lines,
        spheres,
        handles,
        models: model_draws(app, tokens),
        supports: Some(support_mesh),
        support_colors: colors,
    };
    painter.set(scene_slot, egui_wgpu::Callback::new_paint_callback(rect, ViewportCallback(frame)));

    axis_gizmo(&painter, rect, right, up, back, tokens);
    painter.text(
        rect.right_bottom() + vec2(-10.0, -8.0),
        Align2::RIGHT_BOTTOM,
        i18n::format(
            "viewport.caption",
            &[
                ("step", &fmt_step(overlay.step)),
                ("projection", &i18n::tr(if cam.fov > 0.0 { "viewport.perspective" } else { "viewport.orthographic" })),
            ],
        ),
        FontId::proportional(12.0),
        tokens.text_dim,
    );
    if app.session.scene.models().is_empty() {
        painter.text(rect.center(), Align2::CENTER_CENTER, i18n::tr("viewport.empty"), FontId::proportional(16.0), tokens.text_dim);
    }
    if ui.input(|i| !i.raw.hovered_files.is_empty()) {
        painter.rect_stroke(rect.shrink(3.0), 4.0, Stroke::new(3.0, tokens.axis_z), egui::StrokeKind::Inside);
    }
}

/// The gizmo of the edit mode: the transform tool's, or the arrows on selected support joints.
fn triad(app: &ResinApp) -> Option<gizmo::Triad> {
    match app.mode {
        EditMode::Object => transform::triad(app),
        EditMode::Support => supports::triad(app),
    }
}

/// Every model, the selected ones in their own colour while in Object mode.
fn model_draws(app: &ResinApp, tokens: &Tokens) -> Vec<ModelDraw> {
    let scene = &app.session.scene;
    let (plain, selected) = (linear(tokens.model), linear(tokens.model_selected));
    scene
        .models()
        .iter()
        .map(|m| {
            let picked = app.mode == EditMode::Object && scene.selected_models.contains(&m.id);
            ModelDraw { id: m.id, mesh: Arc::clone(&m.mesh), transform: m.transform, color: if picked { selected } else { plain } }
        })
        .collect()
}

/// The supports' round joints in their look's colour, and where a tip would go under the
/// pointer.
fn support_spheres(app: &ResinApp, mesh: &SupportMesh, colors: &[[f32; 4]; 3], tokens: &Tokens) -> Vec<Sphere> {
    let mut out: Vec<Sphere> =
        mesh.joints.iter().filter_map(|&(center, radius, look)| Some(Sphere { center, radius, color: *colors.get(look.index())? })).collect();
    if let Some(Hover::Surface { point, .. }) = app.supports.hover {
        out.push(Sphere { center: point, radius: supports::tip_radius(app) as f32, color: linear(tokens.support_preview) });
    }
    out
}

/// The selected joints' handles, in Support edition only: the joints are what it edits.
fn joint_handles(app: &ResinApp, mesh: &SupportMesh, tokens: &Tokens) -> Vec<Sphere> {
    if app.mode != EditMode::Support {
        return Vec::new();
    }
    let color = linear(tokens.support_selected);
    mesh.handles.iter().filter(|h| h.selected).map(|h| Sphere { center: h.center, radius: h.radius, color }).collect()
}

fn fmt_step(step: f64) -> String {
    if step >= 1.0 { format!("{step:.0}") } else { format!("{step}") }
}

/// The world axes as seen from the camera, in the view's lower-left corner; nearer axes drawn
/// last.
fn axis_gizmo(painter: &egui::Painter, rect: Rect, right: Vec3, up: Vec3, back: Vec3, t: &Tokens) {
    let size = 36.0;
    let centre = pos2(rect.left() + size + 18.0, rect.bottom() - size - 18.0);
    let mut axes = [(Vec3::X, t.axis_x, "X"), (Vec3::Y, t.axis_y, "Y"), (Vec3::Z, t.axis_z, "Z")];
    axes.sort_by(|a, b| a.0.dot(back).total_cmp(&b.0.dot(back)));
    for (axis, colour, label) in axes {
        let tip: Pos2 = centre + vec2(axis.dot(right) as f32, -(axis.dot(up) as f32)) * size;
        painter.line_segment([centre, tip], Stroke::new(2.5, colour));
        painter.circle_filled(tip, 8.0, colour);
        painter.text(tip, Align2::CENTER_CENTER, label, FontId::proportional(11.0), egui::Color32::WHITE);
    }
}
