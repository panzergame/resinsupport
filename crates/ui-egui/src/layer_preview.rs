//! Browsing a sliced job's layers in Slice edition: which layers are shown (the slider at the
//! viewport's right edge, Page Up and Page Down, Home and End), the shown layer decoded for the
//! GPU, and the card that describes it. What is shown is the UI's choice, as the camera is: no
//! command, no undo step.

use std::sync::Arc;

use egui::{Align2, FontId, Pos2, Rect, Sense, Stroke, pos2, vec2};
use resin_engine::Job;
use resin_engine::slice::{ExposureKind, Stamp};

use crate::layer_image::LayerImage;
use crate::theme::Tokens;
use crate::theme::metrics::*;
use crate::ui_kit::{self, Buttons, Card};
use crate::{EditMode, ResinApp, i18n};

/// The slider's column, from the viewport's right edge, and its handles.
const SLIDER_WIDTH: f32 = 28.0;
const TRACK_WIDTH: f32 = 4.0;
const HANDLE_SIZE: egui::Vec2 = vec2(18.0, 10.0);
/// Clear of the view caption in the viewport's bottom-right corner.
const SLIDER_BOTTOM_GAP: f32 = 40.0;
/// Page Up and Page Down with Shift.
const BIG_STEP: isize = 10;
/// Clear of the axis gizmo in the viewport's bottom-left corner.
const CARD_LEFT: f32 = 108.0;
const CARD_WIDTH: f32 = 240.0;

/// A slider handle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Handle {
    Top,
    Bottom,
}

/// The 2D layer view's framing: the image point at the view's centre, in millimetres from the
/// image's top-left corner, and points on screen per millimetre (0 until first fitted).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct View2d {
    pub center: [f64; 2],
    pub scale: f64,
}

/// The pixel under the pointer in the layer view: its column and row, where it is on the plate
/// (mm) and its grey level.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PixelProbe {
    pub x: u32,
    pub y: u32,
    pub plate: (f64, f64),
    pub grey: u8,
}

pub struct LayerPreview {
    /// The highest layer shown (the one the 2D view and the cut show), and the lowest.
    pub(crate) top: usize,
    pub(crate) bottom: usize,
    /// The job's layer count and stamp when it was adopted.
    len: usize,
    stamp: Option<Stamp>,
    /// The 2D layer view instead of the 3D one.
    pub(crate) flat: bool,
    image: Option<Arc<LayerImage>>,
    pub(crate) view: View2d,
    /// The 2D view asked to be fitted on its next frame.
    pub(crate) fit: bool,
    pub(crate) probe: Option<PixelProbe>,
    /// Wheel motion over the slider not yet a whole layer, points.
    wheel: f32,
    grabbed: Option<Handle>,
}

impl Default for LayerPreview {
    fn default() -> Self {
        // Past any job's end: the first job adopted shows its top layer.
        LayerPreview {
            top: usize::MAX,
            bottom: 0,
            len: 0,
            stamp: None,
            flat: false,
            image: None,
            view: View2d::default(),
            fit: true,
            probe: None,
            wheel: 0.0,
            grabbed: None,
        }
    }
}

impl LayerPreview {
    /// Follow the session's job: a job newly adopted sets the range.
    pub fn sync(&mut self, job: Option<&Job>) {
        let stamp = job.map(|j| j.sliced.stamp);
        if stamp != self.stamp {
            self.stamp = stamp;
            self.adopt(job.map_or(0, |j| j.sliced.layers.len()));
        }
    }

    /// A job of `len` layers: the shown layer stays if it is still in range, otherwise the top
    /// one is shown.
    pub fn adopt(&mut self, len: usize) {
        if len != self.len && self.len == 0 {
            self.fit = true;
        }
        self.len = len;
        if self.top >= len {
            self.top = len.saturating_sub(1);
        }
        self.bottom = self.bottom.min(self.top);
    }

    pub fn set_top(&mut self, layer: usize) {
        self.top = layer.min(self.len.saturating_sub(1)).max(self.bottom);
    }

    pub fn set_bottom(&mut self, layer: usize) {
        self.bottom = layer.min(self.top);
    }

    /// Move the shown layer by `by` layers, staying in range.
    pub fn step(&mut self, by: isize) {
        self.set_top(self.top.saturating_add_signed(by));
    }

    /// The shown layer of `job` decoded, decoding it only when it changed: dragging through many
    /// layers decodes one per frame, the one under the handle.
    pub fn image(&mut self, job: &Job, limit: u32) -> Option<Arc<LayerImage>> {
        let key = (job.sliced.stamp, self.top);
        if self.image.as_ref().is_none_or(|i| i.key != key) {
            let layer = job.sliced.layers.get(self.top)?;
            self.image = Some(Arc::new(LayerImage::decode(key, layer, &job.sliced.profile, limit)));
        }
        self.image.clone()
    }

    /// Drop the decoded layer (out of Slice edition, it only holds memory).
    pub fn release(&mut self) {
        self.image = None;
        self.probe = None;
    }

    fn handle_layer(&self, h: Handle) -> usize {
        match h {
            Handle::Top => self.top,
            Handle::Bottom => self.bottom,
        }
    }
}

/// Whether the layers are shown: in Slice edition, once a job was kept.
pub fn active(app: &ResinApp) -> bool {
    app.mode == EditMode::Slice && app.session.job().is_some_and(|j| !j.sliced.layers.is_empty())
}

/// Whether what is shown is an older job than the scene and settings: a slice is running, or the
/// scene changed since.
pub fn dimmed(app: &ResinApp) -> bool {
    !app.session.job_is_current() || app.slicer.is_running()
}

/// Page Up and Page Down step one layer (Shift: ten), Home and End jump to the ends. Whether
/// the key was used.
pub fn key(app: &mut ResinApp, key: egui::Key, mods: egui::Modifiers) -> bool {
    if !active(app) || mods.ctrl || mods.command || mods.alt {
        return false;
    }
    let step = if mods.shift { BIG_STEP } else { 1 };
    let p = &mut app.preview;
    match key {
        egui::Key::PageUp => p.step(step),
        egui::Key::PageDown => p.step(-step),
        egui::Key::Home => p.set_top(0),
        egui::Key::End => p.set_top(usize::MAX),
        _ => return false,
    }
    true
}

/// The slider's column inside the viewport `rect`.
fn column(rect: Rect) -> Rect {
    Rect::from_min_max(
        pos2(rect.right() - SPACE_8 - SLIDER_WIDTH, rect.top() + SPACE_24),
        pos2(rect.right() - SPACE_8, rect.bottom() - SLIDER_BOTTOM_GAP),
    )
}

/// The layer slider over the viewport `rect`. It is its own area above the viewport, so the
/// camera never takes a press or the wheel over it.
pub fn slider(app: &mut ResinApp, ctx: &egui::Context, rect: Rect, t: &Tokens) {
    if !active(app) {
        app.preview.grabbed = None;
        return;
    }
    let Some(job) = app.session.job() else { return };
    let layers = &job.sliced.layers;
    let settings = job.sliced.settings;
    let dim = dimmed(app);
    let col = column(rect);
    if col.height() < SPACE_24 * 2.0 {
        return;
    }
    let p = &mut app.preview;
    let track = Rect::from_center_size(col.center(), vec2(TRACK_WIDTH, col.height() - HANDLE_SIZE.y));
    let last = p.len.saturating_sub(1);
    let y_of = |i: usize| if last == 0 { track.bottom() } else { track.bottom() - track.height() * i as f32 / last as f32 };
    let layer_at = |y: f32| {
        let f = ((track.bottom() - y) / track.height().max(1.0)).clamp(0.0, 1.0);
        (f * last as f32).round() as usize
    };
    let line_scroll = ctx.options(|o| o.input_options.line_scroll_speed).max(1.0);
    egui::Area::new(egui::Id::new("layer_slider")).order(egui::Order::Middle).fixed_pos(col.min).constrain(false).show(ctx, |ui| {
        let (r, resp) = ui.allocate_exact_size(col.size(), Sense::click_and_drag());
        let resp = resp.on_hover_text(i18n::tr("preview.slider.tip"));
        let nearer = |y: f32, p: &LayerPreview| if (y - y_of(p.top)).abs() <= (y - y_of(p.bottom)).abs() { Handle::Top } else { Handle::Bottom };
        if let Some(at) = resp.interact_pointer_pos() {
            if p.grabbed.is_none() {
                p.grabbed = Some(nearer(at.y, p));
            }
            match p.grabbed {
                Some(Handle::Top) => p.set_top(layer_at(at.y)),
                Some(Handle::Bottom) => p.set_bottom(layer_at(at.y)),
                None => {}
            }
        }
        if !resp.is_pointer_button_down_on() {
            p.grabbed = None;
        }
        if resp.hovered() {
            let dy = ui.input(|i| wheel_points(i, line_scroll));
            p.wheel += dy;
            let steps = (p.wheel / line_scroll).trunc();
            if steps != 0.0 {
                p.wheel -= steps * line_scroll;
                p.step(steps as isize);
            }
        } else {
            p.wheel = 0.0;
        }

        let painter = ui.painter().with_clip_rect(rect);
        let fade = |c: egui::Color32| if dim { c.gamma_multiply(0.5) } else { c };
        painter.rect_filled(track, RADIUS_SM, t.border);
        let range = Rect::from_x_y_ranges(track.x_range(), y_of(p.top)..=y_of(p.bottom));
        painter.rect_filled(range, RADIUS_SM, fade(t.accent));
        let hot = resp.hovered() || p.grabbed.is_some();
        for h in [Handle::Bottom, Handle::Top] {
            let at = pos2(track.center().x, y_of(p.handle_layer(h)));
            let rect = Rect::from_center_size(at, HANDLE_SIZE);
            let ring = if hot && p.grabbed.is_none_or(|g| g == h) { t.form_primary } else { t.control_border };
            painter.rect(rect, RADIUS_SM, t.control_fill, Stroke::new(1.5, fade(ring)), egui::StrokeKind::Inside);
        }
        let Some(top) = layers.get(p.top) else { return };
        let label = i18n::format("preview.slider_label", &[("n", &(p.top + 1)), ("z", &format!("{:.2}", top.z))]);
        let anchor = pos2(r.left() - SPACE_6, y_of(p.top));
        let shown = pill(&painter, anchor, &label, Align2::RIGHT_CENTER, t.panel, fade(t.text), t);
        let kind = kind_label(settings.exposure_kind(p.top as u32));
        pill_tag(&painter, pos2(shown.right(), shown.bottom() + SPACE_3), kind, t, dim);
        if p.bottom > 0
            && let Some(bottom) = layers.get(p.bottom)
        {
            let label = i18n::format("preview.slider_label", &[("n", &(p.bottom + 1)), ("z", &format!("{:.2}", bottom.z))]);
            pill(&painter, pos2(r.left() - SPACE_6, y_of(p.bottom)), &label, Align2::RIGHT_CENTER, t.panel, fade(t.form_dim), t);
        }
    });
}

/// This frame's vertical wheel and trackpad scroll, points (up is positive).
fn wheel_points(i: &egui::InputState, line: f32) -> f32 {
    let page = i.content_rect().height();
    i.events
        .iter()
        .filter_map(|e| match e {
            egui::Event::MouseWheel { unit, delta, phase: egui::TouchPhase::Move, .. } => Some(match unit {
                egui::MouseWheelUnit::Point => delta.y,
                egui::MouseWheelUnit::Line => delta.y * line,
                egui::MouseWheelUnit::Page => delta.y * page,
            }),
            _ => None,
        })
        .filter(|d| d.is_finite())
        .sum()
}

/// A rounded label against `at`, aligned by `align`; its rectangle.
fn pill(painter: &egui::Painter, at: Pos2, text: &str, align: Align2, fill: egui::Color32, ink: egui::Color32, t: &Tokens) -> Rect {
    let galley = painter.layout_no_wrap(text.to_string(), FontId::proportional(FONT_SMALL), ink);
    let size = galley.size() + vec2(SPACE_6 * 2.0, SPACE_3 * 2.0);
    let rect = align.anchor_size(at, size);
    painter.rect(rect, RADIUS_SM, fill, Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
    painter.galley(rect.min + vec2(SPACE_6, SPACE_3), galley, ink);
    rect
}

/// The exposure kind's tag under the shown layer's label, right-aligned to `at`.
fn pill_tag(painter: &egui::Painter, at: Pos2, text: &str, t: &Tokens, dim: bool) {
    let ink = if dim { t.selected_ink.gamma_multiply(0.6) } else { t.selected_ink };
    pill(painter, at, text, Align2::RIGHT_TOP, t.accent_soft, ink, t);
}

fn kind_label(kind: ExposureKind) -> &'static str {
    match kind {
        ExposureKind::Bottom => i18n::tr("preview.kind.bottom"),
        ExposureKind::Transition => i18n::tr("preview.kind.transition"),
        ExposureKind::Normal => i18n::tr("preview.kind.normal"),
    }
}

/// The shown layer's card at the viewport's bottom left: its number and height, exposure, lit
/// area and lit regions.
pub fn card(app: &mut ResinApp, ctx: &egui::Context) {
    if !active(app) {
        return;
    }
    let Some(rect) = app.viewport_rect else { return };
    let Some(job) = app.session.job() else { return };
    let Some(layer) = job.sliced.layers.get(app.preview.top) else { return };
    let t = Tokens::for_dark(ctx.global_style().visuals.dark_mode);
    let profile = &job.sliced.profile;
    let (px, py) = profile.pixel_pitch();
    let pixels = f64::from(profile.resolution_x) * f64::from(profile.resolution_y);
    let lit = layer.lit as f64;
    let regions = app.preview.image.as_ref().filter(|i| i.key == (job.sliced.stamp, app.preview.top)).map(|i| i.regions);
    let kind = kind_label(job.sliced.settings.exposure_kind(app.preview.top as u32));
    let rows = [
        (
            "preview.row.layer",
            i18n::format(
                "preview.layer_value",
                &[("n", &(app.preview.top + 1)), ("total", &job.sliced.layers.len()), ("z", &format!("{:.2}", layer.z))],
            ),
        ),
        ("preview.row.exposure", i18n::format("preview.exposure_value", &[("s", &format!("{:.1}", layer.exposure)), ("kind", &kind)])),
        (
            "preview.row.area",
            i18n::format(
                "preview.area_value",
                &[("area", &format!("{:.1}", lit * px * py)), ("share", &format!("{:.1}", 100.0 * lit / pixels.max(1.0)))],
            ),
        ),
        ("preview.row.regions", regions.map_or_else(|| "…".to_string(), |n| n.to_string())),
    ];
    // Anchored within `bounds`, the viewport.
    let offset = vec2(CARD_LEFT, -SPACE_12);
    let card = Card {
        id: egui::Id::new("layer_card"),
        title: i18n::tr("preview.title"),
        icon: "section",
        at: rect.left_bottom(),
        bounds: Some(rect),
        width: CARD_WIDTH,
        buttons: Buttons::None,
    };
    card.show_anchored(ctx, &t, Align2::LEFT_BOTTOM, offset, |ui| {
        for (label, value) in &rows {
            ui_kit::row(ui, &t, i18n::tr(label), |ui| ui.label(value));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_slider_stays_in_the_jobs_range() {
        let mut p = LayerPreview::default();
        p.adopt(100);
        assert_eq!((p.top, p.bottom), (99, 0), "a first job shows its top layer");
        p.set_top(40);
        p.set_bottom(10);
        p.step(-BIG_STEP * 10);
        assert_eq!(p.top, 10, "not under the bottom handle");
        p.set_bottom(50);
        assert_eq!(p.bottom, 10, "nor the bottom over the top");
        p.set_top(1000);
        assert_eq!(p.top, 99);
        p.set_top(40);
        p.adopt(60);
        assert_eq!((p.top, p.bottom), (40, 10), "still in range: kept");
        p.adopt(30);
        assert_eq!((p.top, p.bottom), (29, 10), "a shorter job: its top layer");
        p.adopt(0);
        assert_eq!((p.top, p.bottom), (0, 0));
        p.step(5);
        assert_eq!(p.top, 0);
    }
}
