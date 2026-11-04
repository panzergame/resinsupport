// Ported from SolidCraft (MIT OR Apache-2.0), crates/ui-egui/src/ui_kit/choice.rs (`segmented`) at
// 4bdffef0; see ATTRIBUTION.md. Reduced: equal-width text segments only, no preferences search,
// scenario handles, dropdown fallback, icons, mixed states or per-segment disabling.

use egui::{Align2, CornerRadius, FontId, Rect, Sense, Stroke, Ui, pos2, vec2};

use crate::theme::Tokens;
use crate::theme::metrics::*;

/// A row of equal segments on the control track, one of them chosen: the in-card choice for a
/// few options (a card's tabs, a preset). `options` are shown as given (already translated).
/// Whether a segment was clicked this frame.
pub fn segmented(ui: &mut Ui, t: &Tokens, selected: &mut usize, options: &[&str]) -> bool {
    // Segments are keyed on the strip, so two strips in one card keep apart.
    let (rect, strip) = ui.allocate_exact_size(vec2(ui.available_width(), CONTROL_HEIGHT), Sense::hover());
    ui.painter().rect_filled(rect, CONTROL_RADIUS, t.control_fill);
    let width = rect.width() / options.len().max(1) as f32;
    let font = FontId::proportional(FONT_LABEL);
    let mut clicked = false;
    for (index, text) in options.iter().enumerate() {
        let segment = Rect::from_min_size(pos2(rect.left() + width * index as f32, rect.top()), vec2(width, rect.height()));
        let response = ui.interact(segment, strip.id.with(index), Sense::click());
        let on = *selected == index;
        let first = index == 0;
        let last = index + 1 == options.len();
        let rounding = CornerRadius {
            nw: if first { CONTROL_RADIUS } else { 0 },
            sw: if first { CONTROL_RADIUS } else { 0 },
            ne: if last { CONTROL_RADIUS } else { 0 },
            se: if last { CONTROL_RADIUS } else { 0 },
        };
        if on || response.hovered() {
            ui.painter().rect_filled(segment, rounding, if on { t.selected_bg } else { t.hover });
        }
        if !first {
            ui.painter().vline(segment.left(), segment.y_range(), Stroke::new(1.0, t.control_border));
        }
        let ink = if on { t.selected_ink } else { t.form_ink };
        ui.painter().with_clip_rect(segment.shrink2(vec2(SPACE_4, 0.0))).text(segment.center(), Align2::CENTER_CENTER, *text, font.clone(), ink);
        response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, on, *text));
        if response.on_hover_text(*text).clicked() {
            *selected = index;
            clicked = true;
        }
    }
    ui.painter().rect_stroke(rect, CONTROL_RADIUS, Stroke::new(1.0, t.control_border), egui::StrokeKind::Inside);
    clicked
}
