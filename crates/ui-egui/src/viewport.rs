//! The 3D view: models, origin grid and axes, the plate's outline, the axis gizmo, and camera
//! input. In Slice edition with a job, the layer slider over it, the solids cut at the shown
//! layer, or the 2D layer view in its place.

use std::sync::Arc;

use egui::{Align2, FontId, Pos2, Rect, Sense, Stroke, pos2, vec2};
use resin_engine::geom::Vec3;
use resin_engine::render::{Line, Overlay};

use crate::gpu::{Cap, Clip, ModelDraw, Sphere, ViewFrame, ViewportCallback};
use crate::support_routes::SupportMesh;
use crate::supports::Hover;
use crate::theme::{Tokens, linear};
use crate::{EditMode, ResinApp, box_select, gizmo, i18n, layer_preview, layer_view, navigation, objects, supports, transform};

pub fn show(app: &mut ResinApp, ui: &mut egui::Ui, tokens: &Tokens) {
    let rect = ui.available_rect_before_wrap();
    let resp = ui.allocate_rect(rect, Sense::click_and_drag());
    app.viewport_rect = Some(rect);
    app.step_view_animation(ui.ctx());
    if !layer_preview::active(app) {
        app.preview.release();
    }
    layer_preview::slider(app, ui.ctx(), rect, tokens);
    if layer_preview::active(app) && app.preview.flat {
        layer_view::show(app, ui, rect, &resp, tokens);
        return;
    }
    app.preview.probe = None;
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
                EditMode::Slice => {}
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
        // Nothing is picked: the view only navigates.
        EditMode::Slice => app.supports.clear(),
    }
    box_select::track(app, ui, &proj);

    let (frame, step) = frame(app, ui.ctx(), rect, tokens);
    let (right, up, back) = app.session.camera.basis();
    let cam = app.session.camera;
    painter.set(scene_slot, egui_wgpu::Callback::new_paint_callback(rect, ViewportCallback(frame)));

    axis_gizmo(&painter, rect, right, up, back, tokens);
    box_select::paint(app, &painter, tokens);
    painter.text(
        rect.right_bottom() + vec2(-10.0, -8.0),
        Align2::RIGHT_BOTTOM,
        i18n::format(
            "viewport.caption",
            &[("step", &fmt_step(step)), ("projection", &i18n::tr(if cam.fov > 0.0 { "viewport.perspective" } else { "viewport.orthographic" }))],
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

/// This frame of the 3D view over `rect`, and the grid's step.
pub(crate) fn frame(app: &mut ResinApp, ctx: &egui::Context, rect: Rect, tokens: &Tokens) -> (ViewFrame, f64) {
    let cam = app.session.camera;
    let aspect = f64::from(rect.width().max(1.0)) / f64::from(rect.height().max(1.0));
    let mut overlay = Overlay::build(&cam, aspect, &tokens.overlay());
    let printer = &app.session.print_config().printer;
    let plate = plate_outline(printer.display_x, printer.display_y, linear(tokens.grid_major));
    overlay.lines.extend(plate);
    let r = overlay.radius;
    // The axis an amount is typed along, across the grid.
    let typed_axis = transform::typed_axis(app).map(|(c, dir, axis)| {
        let color = linear(tokens.axes().get(axis).copied().unwrap_or(tokens.axis_x));
        Line { a: c - dir * r, b: c + dir * r, color }
    });
    overlay.lines.extend(typed_axis);
    let preset = *app.session.support_settings().current();
    if app.routes.update(&app.session.scene, &preset) {
        ctx.request_repaint();
    }
    let support_mesh = app.routes.mesh(&app.session.scene, &preset, app.mode != EditMode::Slice);
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
    for l in plate.iter().chain(&typed_axis) {
        bounds.add(l.a);
        bounds.add(l.b);
    }
    let (right, up, back) = cam.basis();
    // A key light from the viewer's upper left, so faces read apart; a fill from the eye.
    let light = (right * -0.45 + up * 0.65 + back * 0.6).normalized().unwrap_or(back);
    let clip = clip_plane(app);
    let dim = layer_preview::dimmed(app);
    let cap = clip.and_then(|c| {
        let job = app.session.job()?;
        let image = app.preview.image(job, app.max_texture)?;
        Some(Cap { image, z: c.above, color: linear(tokens.model), edge: linear(tokens.cap_edge), dim })
    });
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
        clip,
        cap,
    };
    (frame, overlay.step)
}

/// What is kept of the solids: in Slice edition with a job, up to the shown layer's top, and from
/// the lowest shown layer's bottom once that is raised.
pub(crate) fn clip_plane(app: &ResinApp) -> Option<Clip> {
    if !layer_preview::active(app) {
        return None;
    }
    let layers = &app.session.job()?.sliced.layers;
    let above = layers.get(app.preview.top)?.z;
    let below = match app.preview.bottom.checked_sub(1) {
        Some(i) => layers.get(i)?.z,
        None => f32::MIN,
    };
    Some(Clip { below, above })
}

/// The gizmo of the edit mode: the transform tool's, or the arrows on selected support joints.
fn triad(app: &ResinApp) -> Option<gizmo::Triad> {
    match app.mode {
        EditMode::Object => transform::triad(app),
        EditMode::Support => supports::triad(app),
        EditMode::Slice => None,
    }
}

/// The printer's display, `x` by `y` mm centred on the origin, on the ground: what fits on the
/// plate is inside it.
fn plate_outline(x: f64, y: f64, color: [f32; 4]) -> [Line; 4] {
    let (hx, hy) = (x / 2.0, y / 2.0);
    let c = [Vec3::new(-hx, -hy, 0.0), Vec3::new(hx, -hy, 0.0), Vec3::new(hx, hy, 0.0), Vec3::new(-hx, hy, 0.0)];
    [0, 1, 2, 3].map(|i| Line { a: c[i], b: c[(i + 1) % 4], color })
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prefs::Prefs;
    use serde_json::json;

    /// A cube of side `a` on the plate, as ASCII STL.
    pub(crate) fn cube_stl(a: f32) -> String {
        let quads = [
            [(0., 0., 0.), (0., 1., 0.), (1., 1., 0.), (1., 0., 0.)],
            [(0., 0., 1.), (1., 0., 1.), (1., 1., 1.), (0., 1., 1.)],
            [(0., 0., 0.), (1., 0., 0.), (1., 0., 1.), (0., 0., 1.)],
            [(0., 1., 0.), (0., 1., 1.), (1., 1., 1.), (1., 1., 0.)],
            [(0., 0., 0.), (0., 0., 1.), (0., 1., 1.), (0., 1., 0.)],
            [(1., 0., 0.), (1., 1., 0.), (1., 1., 1.), (1., 0., 1.)],
        ];
        let v = |p: (f32, f32, f32)| format!("vertex {} {} {}", p.0 * a, p.1 * a, p.2 * a);
        let mut text = String::from("solid cube\n");
        for [a, b, c, d] in quads {
            for [p, q, r] in [[a, b, c], [a, c, d]] {
                text += &format!("facet normal 0 0 0\nouter loop\n{}\n{}\n{}\nendloop\nendfacet\n", v(p), v(q), v(r));
            }
        }
        text + "endsolid cube\n"
    }

    #[test]
    fn the_cut_follows_the_shown_layer_in_slice_edition_only() {
        let mut app = ResinApp::build(Prefs::default(), &[]);
        let ctx = egui::Context::default();
        let rect = Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0));
        let tokens = Tokens::for_dark(true);
        let dir = std::env::temp_dir().join(format!("resin-ui-cut-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let stl = dir.join("cube.stl");
        std::fs::write(&stl, cube_stl(10.0)).unwrap();
        app.run("file.import_stl", json!({ "path": stl, "fit": false })).unwrap();
        std::fs::remove_dir_all(dir).ok();
        app.run("print.slice", json!({})).unwrap();
        assert!(app.session.job().is_some(), "sliced");
        app.preview.sync(app.session.job());

        let (f, _) = frame(&mut app, &ctx, rect, &tokens);
        assert!(f.clip.is_none() && f.cap.is_none(), "Object edition cuts nothing");

        app.mode = EditMode::Slice;
        app.preview.set_top(50);
        let (f, _) = frame(&mut app, &ctx, rect, &tokens);
        let z = app.session.job().unwrap().sliced.layers[50].z;
        assert_eq!(f.clip.map(|c| (c.above, c.below)), Some((z, f32::MIN)));
        let cap = f.cap.expect("capped");
        assert_eq!(cap.z, z);
        assert_eq!(cap.image.key.1, 50);

        app.preview.set_bottom(10);
        let below = app.session.job().unwrap().sliced.layers[9].z;
        assert_eq!(clip_plane(&app).map(|c| c.below), Some(below), "the lowest shown layer's bottom");

        app.mode = EditMode::Support;
        assert!(frame(&mut app, &ctx, rect, &tokens).0.clip.is_none());
    }
}
