//! The 2D layer view: the shown layer's image as the printer will expose it, grey as is, at the
//! plate's aspect ratio inside its outline. The mouse mapping's pan and zoom move it (it has no
//! orbit), F fits it, and the status bar reads the pixel under the pointer.

use egui::{Pos2, Rect, Stroke, pos2, vec2};

use crate::gpu::{LayerFrame, LayerViewCallback};
use crate::layer_preview::{self, PixelProbe, View2d};
use crate::theme::{Tokens, linear};
use crate::{ResinApp, navigation};

/// The fitted image's share of the view.
const FIT_MARGIN: f64 = 0.92;
/// The closest view: points on screen per image pixel.
const MAX_POINTS_PER_PIXEL: f64 = 96.0;

pub fn show(app: &mut ResinApp, ui: &mut egui::Ui, rect: Rect, resp: &egui::Response, t: &Tokens) {
    app.preview.probe = None;
    let dim = layer_preview::dimmed(app);
    let Some(job) = app.session.job() else { return };
    let Some(image) = app.preview.image(job, app.max_texture) else { return };
    let plate = image.plate;
    let size = (plate.display_x, plate.display_y);
    let (kx, ky) = plate.pitch();
    let fitted = fit_scale(rect, size);
    if app.preview.fit || !(app.preview.view.scale.is_finite() && app.preview.view.scale > 0.0) {
        app.preview.view = View2d { center: [size.0 / 2.0, size.1 / 2.0], scale: fitted };
        app.preview.fit = false;
    }
    let m = navigation::handle_2d(app, ui, rect, resp);
    let v = &mut app.preview.view;
    v.center[0] -= f64::from(m.pan.x) / v.scale;
    v.center[1] -= f64::from(m.pan.y) / v.scale;
    if m.zoom.is_finite() && m.zoom != 1.0 {
        let at = m.at.filter(|p| rect.contains(*p)).unwrap_or(rect.center());
        let held = to_image(v, rect, at);
        v.scale = (v.scale * m.zoom).clamp(fitted * 0.25, MAX_POINTS_PER_PIXEL / kx.min(ky).max(1e-9));
        // The point under the pointer stays under it.
        let now = to_image(v, rect, at);
        v.center[0] += held[0] - now[0];
        v.center[1] += held[1] - now[1];
    }
    if m.pan != egui::Vec2::ZERO || m.zoom != 1.0 {
        ui.ctx().request_repaint();
    }
    let v = *v;

    let frame = LayerFrame {
        image: image.clone(),
        view_proj: view_proj(&v, rect),
        bg_top: linear(t.viewport_top),
        bg_bottom: linear(t.viewport_bottom),
        grid: linear(t.layer_grid),
        dim,
    };
    let painter = ui.painter_at(rect);
    painter.add(egui_wgpu::Callback::new_paint_callback(rect, LayerViewCallback(frame)));
    let outline = Rect::from_two_pos(to_screen(&v, rect, [0.0, 0.0]), to_screen(&v, rect, [size.0, size.1]));
    painter.rect_stroke(outline, 0.0, Stroke::new(1.0, t.grid_major), egui::StrokeKind::Outside);

    if let Some(p) = ui.input(|i| i.pointer.hover_pos()).filter(|p| rect.contains(*p) && resp.contains_pointer()) {
        let mm = to_image(&v, rect, p);
        let (x, y) = ((mm[0] / kx).floor(), (mm[1] / ky).floor());
        if x >= 0.0 && y >= 0.0 {
            let (x, y) = (x as u32, y as u32);
            if let Some(grey) = image.grey(x, y) {
                let plate_mm = plate.to_plate(f64::from(x) + 0.5, f64::from(y) + 0.5);
                app.preview.probe = Some(PixelProbe { x, y, plate: plate_mm, grey });
            }
        }
    }
}

/// Points per millimetre that fit a `size` mm image in `rect`.
fn fit_scale(rect: Rect, size: (f64, f64)) -> f64 {
    let s = (f64::from(rect.width()) / size.0.max(1e-9)).min(f64::from(rect.height()) / size.1.max(1e-9)) * FIT_MARGIN;
    if s.is_finite() && s > 0.0 { s } else { 1.0 }
}

/// Image millimetres (from its top-left corner, Y down) to the screen.
fn to_screen(v: &View2d, rect: Rect, mm: [f64; 2]) -> Pos2 {
    let c = rect.center();
    pos2(c.x + ((mm[0] - v.center[0]) * v.scale) as f32, c.y + ((mm[1] - v.center[1]) * v.scale) as f32)
}

fn to_image(v: &View2d, rect: Rect, p: Pos2) -> [f64; 2] {
    let d = p - rect.center();
    [v.center[0] + f64::from(d.x) / v.scale, v.center[1] + f64::from(d.y) / v.scale]
}

/// Image millimetres to the view's clip space, column by column.
fn view_proj(v: &View2d, rect: Rect) -> [[f32; 4]; 4] {
    let half = vec2(rect.width().max(1.0), rect.height().max(1.0)) / 2.0;
    let sx = v.scale / f64::from(half.x);
    let sy = -v.scale / f64::from(half.y);
    [[sx as f32, 0.0, 0.0, 0.0], [0.0, sy as f32, 0.0, 0.0], [0.0, 0.0, 0.0, 0.0], [(-v.center[0] * sx) as f32, (-v.center[1] * sy) as f32, 0.5, 1.0]]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screen_and_image_agree() {
        let rect = Rect::from_min_size(pos2(100.0, 50.0), vec2(800.0, 400.0));
        let v = View2d { center: [100.0, 60.0], scale: 3.0 };
        let p = to_screen(&v, rect, [110.0, 50.0]);
        assert_eq!(p, pos2(530.0, 220.0));
        let back = to_image(&v, rect, p);
        assert!((back[0] - 110.0).abs() < 1e-4 && (back[1] - 50.0).abs() < 1e-4);
        // The same point in clip space: right of centre, above it.
        let m = view_proj(&v, rect);
        let x = m[0][0] * 110.0 + m[3][0];
        let y = m[1][1] * 50.0 + m[3][1];
        assert!((x - 30.0 / 400.0).abs() < 1e-6 && (y - 30.0 / 200.0).abs() < 1e-6, "{x} {y}");
    }
}
