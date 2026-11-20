// Ported from SolidCraft (MIT OR Apache-2.0), crates/ui-kit/src/status.rs (`progress`) at
// a34756830; see ATTRIBUTION.md. Reduced: no scenario handles.

//! A running operation's progress.

use egui::{Response, Ui, vec2};

use crate::theme::Tokens;
use crate::theme::metrics::*;

/// A determinate progress bar across the row, `SPACE_6` tall, under a [`super::status_line`]
/// naming the step; `fraction` is clamped and a non-finite one counts as none.
pub fn progress(ui: &mut Ui, t: &Tokens, fraction: f32) -> Response {
    let fraction = if fraction.is_finite() { fraction.clamp(0.0, 1.0) } else { 0.0 };
    let (bar, response) = ui.allocate_exact_size(vec2(ui.available_width().max(0.0), SPACE_6), egui::Sense::hover());
    super::mark(ui.layer_id(), "progress", bar, "");
    ui.painter().rect_filled(bar, SPACE_3, t.dialog_rule);
    if fraction > 0.0 {
        ui.painter().rect_filled(egui::Rect::from_min_size(bar.min, vec2(bar.width() * fraction, bar.height())), SPACE_3, t.form_primary);
    }
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::ProgressIndicator, ui.is_enabled(), format!("{:.0}%", fraction * 100.0)));
    response
}
