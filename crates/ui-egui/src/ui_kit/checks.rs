// Ported from SolidCraft (MIT OR Apache-2.0), crates/ui-kit/src/checks.rs (`box_at`,
// `paint_check_glyph`, `paint_box`, `checkbox`, `checkbox_indeterminate`) at a34756830; see
// ATTRIBUTION.md. Reduced: no icon variant, bevelled themes, wrapping labels or preferences search.
// `check_row` and `mixed_check_row` are ResinSupport's.

//! The only checkbox: one box and one tick for inline boxes, rows and menu entries.

use egui::{FontId, Rect, Response, Sense, Stroke, Ui, pos2, vec2};

use super::layout::paint_text;
use crate::theme::Tokens;
use crate::theme::metrics::*;

/// The box, `SPACE_14` square on whole device pixels, centred on `center`.
fn box_at(ui: &Ui, center: egui::Pos2) -> Rect {
    let ppp = ui.ctx().pixels_per_point();
    let min = center - vec2(SPACE_14, SPACE_14) / 2.0;
    Rect::from_min_size(pos2((min.x * ppp).round() / ppp, (min.y * ppp).round() / ppp), vec2(SPACE_14, SPACE_14))
}

/// The tick in `square`, or a bar for a mixed value.
pub(super) fn paint_check_glyph(painter: &egui::Painter, square: Rect, mixed: bool, ink: egui::Color32) {
    let mark = square.shrink(SPACE_3);
    let points = if mixed { vec![mark.left_center(), mark.right_center()] } else { vec![mark.left_center(), mark.center_bottom(), mark.right_top()] };
    painter.add(egui::Shape::line(points, Stroke::new(SPACE_2, ink)));
}

/// The box in every state; a disabled box takes egui's disabled opacity.
fn paint_box(ui: &Ui, t: &Tokens, square: Rect, response: &Response, on: bool, mixed: bool) {
    let selected = on || mixed;
    let lit = selected || response.hovered() || response.has_focus();
    let fill = if selected {
        t.form_primary_fill
    } else if response.hovered() {
        t.hover
    } else {
        t.control_fill
    };
    let border = if lit { t.form_primary } else { t.control_border };
    ui.painter().rect(square, RADIUS_SM, fill, Stroke::new(1.0, border), egui::StrokeKind::Inside);
    if selected {
        paint_check_glyph(ui.painter(), square, mixed, t.on_accent);
    }
    if response.has_focus() {
        ui.painter().rect_stroke(square.expand(SPACE_2), RADIUS_SM, Stroke::new(1.0, t.form_primary), egui::StrokeKind::Outside);
    }
}

/// A checkbox and its label, as wide as they are.
pub fn checkbox(ui: &mut Ui, t: &Tokens, label: &str, value: &mut bool) -> Response {
    checkbox_indeterminate(ui, t, label, value, false)
}

/// [`checkbox`] for a value of several things that may differ: `mixed` shows a bar, and a click
/// ticks it.
pub fn checkbox_indeterminate(ui: &mut Ui, t: &Tokens, label: &str, value: &mut bool, mixed: bool) -> Response {
    let font = FontId::proportional(FONT_LABEL);
    let gap = if label.is_empty() { 0.0 } else { SPACE_6 };
    let text = ui.painter().layout_no_wrap(label.to_owned(), font.clone(), t.form_ink).size();
    let width = (SPACE_14 + gap + text.x).max(ui.spacing().interact_size.y).min(ui.available_width().max(0.0));
    let height = ui.spacing().interact_size.y.max(SPACE_14).max(text.y);
    let (rect, mut response) = ui.allocate_exact_size(vec2(width, height), Sense::click());
    super::mark(ui.layer_id(), "checkbox", rect, label);
    if response.clicked() {
        *value = mixed || !*value;
        response.mark_changed();
    }
    let square = box_at(ui, pos2(rect.left() + SPACE_14 / 2.0, rect.center().y));
    paint_box(ui, t, square, &response, *value, mixed && !response.changed());
    let text_rect = Rect::from_min_max(pos2((square.right() + gap).min(rect.right()), rect.top()), rect.max);
    paint_text(ui, text_rect, label, font, t.form_ink);
    response.widget_info(|| {
        let mut info = egui::WidgetInfo::selected(egui::WidgetType::Checkbox, ui.is_enabled(), *value, label);
        if mixed && !response.changed() {
            info.selected = None;
        }
        info
    });
    if response.hovered() && ui.is_enabled() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    response
}

/// A checkbox setting across a row: the box and its label, `help` (unless empty) on hover.
pub fn check_row(ui: &mut Ui, t: &Tokens, label: &str, help: &str, value: &mut bool) -> Response {
    mixed_check_row(ui, t, label, help, value, false)
}

/// [`check_row`] for a setting of several things that may differ: `mixed` shows the box neither
/// ticked nor clear.
pub fn mixed_check_row(ui: &mut Ui, t: &Tokens, label: &str, help: &str, value: &mut bool, mixed: bool) -> Response {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), CONTROL_HEIGHT), Sense::hover());
    super::mark(ui.layer_id(), "check_row", rect, label);
    let layout = egui::Layout::left_to_right(egui::Align::Center);
    let response =
        ui.scope_builder(egui::UiBuilder::new().max_rect(rect).layout(layout), |ui| checkbox_indeterminate(ui, t, label, value, mixed)).inner;
    if help.is_empty() { response } else { response.on_hover_text(help).on_disabled_hover_text(help) }
}

#[cfg(test)]
mod tests {
    use egui::{Pos2, Rect};

    use super::*;

    fn click(ctx: &egui::Context, value: &mut bool, mixed: bool, events: Vec<egui::Event>) -> bool {
        let raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(300.0, 100.0))), events, ..Default::default() };
        let t = Tokens::for_dark(false);
        let mut changed = false;
        let mut out = ctx.run_ui(raw, |ui| changed = checkbox_indeterminate(ui, &t, "Lifted", value, mixed).changed());
        out.textures_delta.clear();
        changed
    }

    #[test]
    fn a_click_toggles_the_box_and_a_mixed_one_ticks() {
        let ctx = egui::Context::default();
        let at = pos2(SPACE_14 / 2.0 + 1.0, CONTROL_HEIGHT / 2.0);
        let press = |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
        let events = || vec![egui::Event::PointerMoved(at), press(true), press(false)];
        let mut value = false;
        click(&ctx, &mut value, false, vec![]);
        assert!(click(&ctx, &mut value, false, events()) && value);
        click(&ctx, &mut value, false, vec![]);
        assert!(click(&ctx, &mut value, false, events()) && !value);
        click(&ctx, &mut value, true, vec![]);
        assert!(click(&ctx, &mut value, true, events()) && value, "mixed: ticked");
    }
}
