// Ported from SolidCraft (MIT OR Apache-2.0), crates/ui-kit/src/fields.rs (`text_input`,
// `parse_number`, `number_field`, `number_fields`, `text_field`, `multiline_text_field`,
// `dropdown`, `dropdown_menu`, `slider`) at a34756830; see ATTRIBUTION.md. Reduced: no expression
// fields, completion, label scrubbing, search pickers, text context menu, bevelled themes,
// preferences search or scenario handles; a number field takes its decimals; a text field takes
// its length limit; the dropdown's arrow is the section's chevron. `dropdown_item`, `choice_row`
// and `choice_row_help` are ResinSupport's.

//! The value controls of a card's rows: numbers, text, drop-downs and the slider.

use std::ops::RangeInclusive;

use egui::emath::Numeric;
use egui::{Response, Ui, vec2};

use super::layout::{row, sized_row};
use super::section::paint_chevron;
use crate::i18n;
use crate::theme::Tokens;
use crate::theme::metrics::*;

/// One alignment for every kit text input: the text centred down the field, `FORM_INSET` from
/// its sides.
pub(super) fn text_input(edit: egui::TextEdit<'_>) -> egui::TextEdit<'_> {
    edit.vertical_align(egui::Align::Center).margin(vec2(FORM_INSET, 0.0)).min_size(vec2(0.0, CONTROL_HEIGHT))
}

/// A plain number typed into a field, with a decimal point or a decimal comma: "2.5" and "2,5"
/// are both 2.5, as a numeric keypad types either depending on the locale. Spaces are ignored and
/// "−" is a minus. A comma beside a point ("1.234,5") or a second comma is not guessed at.
pub fn parse_number(text: &str) -> Option<f64> {
    let t: String = text.chars().filter(|c| !c.is_whitespace()).map(|c| if c == '\u{2212}' { '-' } else { c }).collect();
    let t = if !t.contains('.') && t.matches(',').count() == 1 { t.replacen(',', ".", 1) } else { t };
    t.parse::<f64>().ok().filter(|v| v.is_finite())
}

/// A number within `range`, dragged `speed` a point, shown with `suffix` and at most `decimals`
/// places, across the control column.
pub fn number_field<T: Numeric>(ui: &mut Ui, value: &mut T, range: RangeInclusive<T>, speed: f64, suffix: &str, decimals: usize) -> Response {
    let mut drag = egui::DragValue::new(value).range(range).speed(speed).suffix(suffix).custom_parser(parse_number);
    drag = if T::INTEGRAL { drag.fixed_decimals(0) } else { drag.max_decimals(decimals) };
    ui.add_sized([ui.available_width().max(0.0), CONTROL_HEIGHT], drag)
}

/// Equal number fields sharing the control column, gaps included; each field's response in order.
pub fn number_fields<T: Numeric>(
    ui: &mut Ui,
    values: &mut [T],
    range: RangeInclusive<T>,
    speed: f64,
    suffix: &str,
    decimals: usize,
) -> Vec<Response> {
    let gap = ui.spacing().item_spacing.x;
    let width = (ui.available_width() - gap * values.len().saturating_sub(1) as f32).max(0.0) / values.len().max(1) as f32;
    values
        .iter_mut()
        .enumerate()
        .map(|(index, value)| {
            ui.push_id(index, |ui| {
                let layout = egui::Layout::top_down_justified(egui::Align::LEFT);
                ui.allocate_ui_with_layout(vec2(width, CONTROL_HEIGHT), layout, |ui| number_field(ui, value, range.clone(), speed, suffix, decimals))
                    .inner
            })
            .inner
        })
        .collect()
}

/// A one-line text field across the control column, at most `limit` characters.
pub fn text_field(ui: &mut Ui, t: &Tokens, value: &mut String, hint: &str, limit: usize) -> Response {
    let edit = text_input(egui::TextEdit::singleline(value)).hint_text(hint).char_limit(limit).background_color(t.field).clip_text(true);
    ui.add_sized([ui.available_width().max(0.0), CONTROL_HEIGHT], edit)
}

/// A three-line text row: `label` in the label column, the field in the control column.
pub fn multiline_text_field(ui: &mut Ui, t: &Tokens, label: &str, value: &mut String) -> Response {
    let height = CONTROL_HEIGHT * 3.0;
    sized_row(ui, t, label, height, |ui| {
        let edit = egui::TextEdit::multiline(value).margin(vec2(FORM_INSET, SPACE_6)).desired_rows(3).background_color(t.field);
        ui.add_sized([ui.available_width().max(0.0), height], edit)
    })
}

/// The kit's drop-down arrow: the section's chevron, dimmed.
fn dropdown_arrow(t: Tokens) -> impl FnOnce(&Ui, egui::Rect, &egui::style::WidgetVisuals, bool) + 'static {
    move |ui, rect, _, _| {
        let size = rect.height().min(SPACE_12);
        paint_chevron(ui.painter(), egui::Rect::from_center_size(rect.center(), vec2(size, size)), true, t.form_dim);
    }
}

/// A drop-down across the control column showing `current`; `add` lays out its entries (with
/// [`dropdown_item`]). Esc closes an open list without reaching the card.
pub fn dropdown_menu(ui: &mut Ui, t: &Tokens, id: impl std::hash::Hash + std::fmt::Debug, current: &str, add: impl FnOnce(&mut Ui)) -> Response {
    let width = ui.available_width().max(0.0);
    let popup = egui::ComboBox::from_id_salt(id)
        .width(width)
        .height(SPACE_24 * 14.0)
        .selected_text(current)
        .truncate()
        .icon(dropdown_arrow(*t))
        .show_ui(ui, add);
    let response = popup.response;
    if popup.inner.is_some() && ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
        response.request_focus();
    }
    response
}

/// One entry of a [`dropdown_menu`], highlighted when `selected`.
pub fn dropdown_item(ui: &mut Ui, selected: bool, label: &str) -> Response {
    ui.selectable_label(selected, label)
}

/// A drop-down of `options` (already translated), `selected` their index.
pub fn dropdown(ui: &mut Ui, t: &Tokens, id: impl std::hash::Hash + std::fmt::Debug, selected: &mut usize, options: &[&str]) -> Response {
    let current = options.get(*selected).copied().unwrap_or("");
    let mut picked = None;
    let mut response = dropdown_menu(ui, t, id, current, |ui| {
        for (index, option) in options.iter().enumerate() {
            if dropdown_item(ui, index == *selected, option).clicked() {
                picked = Some(index);
            }
        }
    });
    if let Some(index) = picked.filter(|i| i != selected) {
        *selected = index;
        response.mark_changed();
    }
    response
}

/// A setting chosen from `options` (stored value, catalog id of its name) in a row.
pub fn choice_row(ui: &mut Ui, t: &Tokens, label: &str, id: &str, value: &mut String, options: &[(&str, &'static str)]) {
    choice_row_help(ui, t, label, None, id, value, options);
}

/// [`choice_row`], with `help` shown on hovering the choice.
pub fn choice_row_help(ui: &mut Ui, t: &Tokens, label: &str, help: Option<&str>, id: &str, value: &mut String, options: &[(&str, &'static str)]) {
    let names: Vec<&str> = options.iter().map(|(_, name)| i18n::tr(name)).collect();
    let found = options.iter().position(|(v, _)| *v == value.as_str());
    let mut index = found.unwrap_or(usize::MAX);
    row(ui, t, label, |ui| {
        let response = if found.is_some() {
            dropdown(ui, t, id, &mut index, &names)
        } else {
            // A stored value no option names: shown as it is, until one is picked.
            let mut picked = None;
            let response = dropdown_menu(ui, t, id, value, |ui| {
                for (i, name) in names.iter().enumerate() {
                    if dropdown_item(ui, false, name).clicked() {
                        picked = Some(i);
                    }
                }
            });
            index = picked.unwrap_or(usize::MAX);
            response
        };
        if let Some(help) = help {
            response.on_hover_text(help);
        }
    });
    if let Some((v, _)) = options.get(index) {
        *value = (*v).to_string();
    }
}

/// The one slider: egui's input, keyboard and value editor over a token-painted track and thumb.
pub fn slider<T: Numeric>(ui: &mut Ui, t: &Tokens, value: &mut T, range: RangeInclusive<T>) -> Response {
    ui.scope(|ui| {
        let limits = (range.start().to_f64(), range.end().to_f64());
        let font = egui::TextStyle::Monospace.resolve(ui.style());
        ui.style_mut().drag_value_text_style = egui::TextStyle::Monospace;
        // The value editor keeps the width of its widest value, so the track does not move as it is dragged.
        let text_width = [limits.0, limits.1, value.to_f64()]
            .into_iter()
            .map(|v| ui.painter().layout_no_wrap(format!("{v:.2}"), font.clone(), t.form_ink).size().x.ceil())
            .fold(0.0, f32::max);
        let editor = (text_width + ui.spacing().button_padding.x * 2.0).max(ui.spacing().interact_size.x);
        let reserved = (editor + ui.spacing().item_spacing.x).max(SPACE_24 * 2.0 + SPACE_4);
        ui.spacing_mut().slider_width = (ui.available_width() - reserved).max(0.0);
        let widget = egui::Slider::new(value, range).handle_shape(egui::style::HandleShape::Circle).trailing_fill(false);
        let layer = ui.layer_id();
        let paint_start = ui.ctx().graphics(|g| g.get(layer).map_or(egui::layers::ShapeIdx(0), |list| list.next_idx()));
        let response = ui.add(widget);
        let slider = egui::Rect::from_min_size(response.rect.min, vec2(ui.spacing().slider_width, response.rect.height()));
        // egui 0.36.2 paints the rail, then the round handle, then the value editor: only that
        // pair, positively identified, gives way to the painted track and thumb. Anything else
        // (an invisible or clipped slider) keeps egui's own.
        ui.ctx().graphics_mut(|g| {
            let Some(list) = g.get_mut(layer) else { return };
            if list.next_idx().0.saturating_sub(paint_start.0) < 2 {
                return;
            }
            let handle_idx = egui::layers::ShapeIdx(paint_start.0 + 1);
            let rail_height = ui.spacing().slider_rail_height.max(0.0);
            let native_radius = slider.height() / 2.5;
            let native_range = slider.x_range().shrink(native_radius);
            let (mut rail_matches, mut handle_matches) = (false, false);
            list.mutate_shape(paint_start, |entry| {
                if let egui::Shape::Rect(rail) = &entry.shape {
                    let expected = egui::Rect::from_center_size(slider.center(), vec2(slider.width(), rail_height));
                    rail_matches = (rail.rect.min - expected.min).length_sq() < 0.0001
                        && (rail.rect.max - expected.max).length_sq() < 0.0001
                        && rail.stroke == egui::Stroke::NONE;
                }
            });
            list.mutate_shape(handle_idx, |entry| {
                if let egui::Shape::Circle(handle) = &entry.shape {
                    handle_matches = (handle.center.y - slider.center().y).abs() < 0.01
                        && handle.center.x >= native_range.min.min(native_range.max) - 0.01
                        && handle.center.x <= native_range.min.max(native_range.max) + 0.01
                        && (handle.radius - native_radius).abs() < 0.01;
                }
            });
            if rail_matches && handle_matches {
                list.mutate_shape(paint_start, |entry| entry.shape = egui::Shape::Noop);
                list.mutate_shape(handle_idx, |entry| entry.shape = egui::Shape::Noop);
            }
        });
        // egui's input range (handle radius = height / 2.5), so the thumb stays under the pointer.
        let input_radius = ui.text_style_height(&egui::TextStyle::Body).max(ui.spacing().interact_size.y) / 2.5;
        let track = egui::Rect::from_center_size(slider.center(), vec2((slider.width() - input_radius * 2.0).max(0.0), SPACE_4));
        let fraction = if limits.1 != limits.0 { ((value.to_f64() - limits.0) / (limits.1 - limits.0)).clamp(0.0, 1.0) as f32 } else { 0.0 };
        let center = egui::pos2(egui::lerp(track.x_range(), fraction), track.center().y);
        ui.painter().rect_filled(track, CONTROL_RADIUS, t.dialog_rule);
        ui.painter().rect_filled(egui::Rect::from_min_max(track.min, egui::pos2(center.x, track.bottom())), CONTROL_RADIUS, t.form_primary);
        ui.painter().circle(center, SPACE_8, t.control_fill, egui::Stroke::new(SPACE_2, t.form_primary));
        response
    })
    .inner
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_number_takes_a_decimal_point_or_comma() {
        assert_eq!(parse_number("2.5"), Some(2.5));
        assert_eq!(parse_number(" 2,5 "), Some(2.5));
        assert_eq!(parse_number("\u{2212}3"), Some(-3.0));
        assert_eq!(parse_number("1.234,5"), None);
        assert_eq!(parse_number("1,2,3"), None);
        assert_eq!(parse_number("inf"), None);
    }
}
