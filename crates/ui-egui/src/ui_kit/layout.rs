// Ported from SolidCraft (MIT OR Apache-2.0), crates/ui-kit/src/layout.rs (`scope`, `row`,
// `sized_row`, `status_line`, `caption`, `warning_caption`) and crates/ui-kit/src/metrics.rs
// (`label_column`, `label_gap`) and crates/ui-kit/src/text.rs (`paint_text`) at a34756830; see
// ATTRIBUTION.md. Reduced: no fixed or growing
// label grids, preferences search, help icons, label scrubbing, indented labels or scenario
// handles. `value_row`, `action_row` and `hint` are ResinSupport's.

//! A card body's form: a label column and a control column that every row of the card shares.

use egui::{FontId, Rect, Response, RichText, Stroke, Ui, UiBuilder, UiStackInfo, pos2, vec2};

use crate::theme::Tokens;
use crate::theme::metrics::*;

/// Tags a card body, so that its rows share one label column.
const FORM: &str = "resin_form";

/// Widest a label column grows, as a multiple of its basis.
const LABEL_COLUMN_MAX: f32 = 1.6;

/// Form styling for a card's body: label-size text, 28 pt controls, the chrome's control fills.
/// Its rows share one label column.
pub fn scope<R>(ui: &mut Ui, t: &Tokens, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.scope_builder(UiBuilder::new().ui_stack_info(UiStackInfo::default().with_tag(FORM)), |ui| {
        ui.spacing_mut().item_spacing = vec2(SPACE_4, FORM_GAP_Y);
        ui.spacing_mut().interact_size.y = CONTROL_HEIGHT;
        ui.spacing_mut().button_padding = vec2(SPACE_8, SPACE_4);
        ui.style_mut().text_styles.insert(egui::TextStyle::Body, FontId::proportional(FONT_LABEL));
        ui.style_mut().text_styles.insert(egui::TextStyle::Button, FontId::proportional(FONT_LABEL));
        ui.style_mut().text_styles.insert(egui::TextStyle::Monospace, FontId::monospace(FONT_BODY));
        let v = ui.visuals_mut();
        v.weak_text_color = Some(t.form_dim);
        v.override_text_color = Some(t.form_ink);
        v.selection.stroke = Stroke::new(1.0, t.form_primary);
        v.selection.bg_fill = t.selected_bg;
        v.extreme_bg_color = t.field;
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, t.dialog_rule);
        for state in [&mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
            state.bg_fill = t.control_fill;
            state.weak_bg_fill = t.control_fill;
            state.bg_stroke = Stroke::new(1.0, t.control_border);
            state.corner_radius = egui::CornerRadius::same(CONTROL_RADIUS);
            state.expansion = 0.0;
            state.fg_stroke.color = t.form_ink;
        }
        v.widgets.hovered.bg_stroke = Stroke::new(1.0, t.form_primary);
        v.widgets.active.bg_stroke = Stroke::new(1.0, t.form_primary);
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
        add(ui)
    })
    .inner
}

/// A label's width in the row label's font.
fn label_width(ui: &Ui, text: &str) -> f32 {
    ui.painter().layout_no_wrap(text.to_owned(), FontId::proportional(FONT_LABEL), egui::Color32::WHITE).size().x.ceil()
}

/// The label column of a row `row_width` wide: 70 pt (104 pt in a wide card), widened for the
/// form's longest label up to 1.6 times that, never past half the row. The longest label of the
/// last pass sizes it, so it settles within two frames and narrows again when the long rows go.
fn label_column(ui: &Ui, text: &str, row_width: f32) -> f32 {
    let wide = row_width >= FORM_CARD_WIDTH + SPACE_24 * 2.0;
    let base = if wide { FORM_LABEL_WIDTH + SPACE_24 + SPACE_10 } else { FORM_LABEL_WIDTH };
    let natural = label_width(ui, text);
    let widest = match ui.stack().iter().find(|node| node.tags().contains(FORM)).map(|node| node.id) {
        Some(form) => {
            let key = form.with("label_column");
            let pass = ui.ctx().cumulative_pass_nr();
            let (seen, current, previous) = ui.ctx().data(|d| d.get_temp::<(u64, f32, f32)>(key)).unwrap_or((pass, 0.0, 0.0));
            let (current, previous) = if seen == pass {
                (current, previous)
            } else if seen + 1 == pass {
                (0.0, current)
            } else {
                (0.0, 0.0)
            };
            let current = current.max(natural);
            ui.ctx().data_mut(|d| d.insert_temp(key, (pass, current, previous)));
            current.max(previous)
        }
        None => natural,
    };
    (widest + SPACE_4).clamp(base, base * LABEL_COLUMN_MAX).min(row_width * 0.5)
}

/// The gap between the label and control columns.
fn label_gap(row_width: f32) -> f32 {
    if row_width >= FORM_CARD_WIDTH + SPACE_24 * 2.0 { SPACE_10 } else { SPACE_8 }
}

/// Where a row of `ui`'s width labelled `text` puts its control: the left of its control column.
fn control_left(ui: &Ui, text: &str, row: Rect) -> f32 {
    (row.left() + label_column(ui, text, row.width()) + label_gap(row.width())).min(row.right())
}

/// One form row, `CONTROL_HEIGHT` tall: `label` in the label column (cut short to fit, whole on
/// hover), the control in the rest. An empty label keeps the column, so the control lines up.
pub fn row<R>(ui: &mut Ui, t: &Tokens, label: &str, control: impl FnOnce(&mut Ui) -> R) -> R {
    sized_row(ui, t, label, CONTROL_HEIGHT, control)
}

/// [`row`], `height` tall, its label centred on the row.
pub fn sized_row<R>(ui: &mut Ui, t: &Tokens, label: &str, height: f32, control: impl FnOnce(&mut Ui) -> R) -> R {
    let width = ui.available_width().max(0.0);
    let (rect, _) = ui.allocate_exact_size(vec2(width, height.max(0.0)), egui::Sense::hover());
    let left = control_left(ui, label, rect);
    let layout = egui::Layout::left_to_right(egui::Align::Center);
    // Children, not scopes: a scope allocates what it holds, and a control shorter than the row
    // would move the cursor back up into it.
    if !label.is_empty() {
        let label_rect = Rect::from_min_max(rect.min, pos2((left - label_gap(width)).max(rect.left()), rect.bottom()));
        let mut child = ui.new_child(UiBuilder::new().id_salt((label, "label")).max_rect(label_rect).layout(layout));
        child.add(egui::Label::new(RichText::new(label).size(FONT_LABEL).color(t.dialog_label)).truncate());
    }
    let control_rect = Rect::from_min_max(pos2(left, rect.top()), rect.max);
    let mut child = ui.new_child(UiBuilder::new().id_salt(label).max_rect(control_rect).layout(layout));
    child.set_clip_rect(child.clip_rect().intersect(control_rect));
    control(&mut child)
}

/// A row of read-only text: the value wraps in the control column and the row grows to hold every
/// line, so a long value is never cut. One line keeps the 28 pt row.
pub fn value_row(ui: &mut Ui, t: &Tokens, label: &str, value: &str) -> Response {
    let width = ui.available_width().max(0.0);
    let column = label_column(ui, label, width) + label_gap(width);
    let room = (width - column).max(0.0);
    let lines = ui.painter().layout(value.to_owned(), FontId::proportional(FONT_LABEL), t.form_ink, room).size().y;
    sized_row(ui, t, label, CONTROL_HEIGHT.max(lines + SPACE_8), |ui| {
        ui.add(egui::Label::new(RichText::new(value).size(FONT_LABEL).color(t.form_ink)).wrap())
    })
}

/// A one-shot action in a card: a button across the control column, under the other rows'
/// controls. Its response, for its tooltip and click.
pub fn action_row(ui: &mut Ui, t: &Tokens, label: &str) -> Response {
    row(ui, t, "", |ui| ui.add_sized([ui.available_width(), CONTROL_HEIGHT], egui::Button::new(label)))
}

/// What a [`status_line`] says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatusTone {
    Dim,
    Warning,
    Error,
}

/// Full-width, wrapping status or refusal text. Put it directly in the form, outside a [`row`]:
/// rows hold one line.
pub fn status_line(ui: &mut Ui, t: &Tokens, text: &str, tone: StatusTone) -> Response {
    let ink = match tone {
        StatusTone::Dim => t.text_dim,
        StatusTone::Warning => t.warning,
        StatusTone::Error => t.error,
    };
    ui.add(egui::Label::new(RichText::new(text).size(FONT_LABEL).color(ink)).wrap())
}

/// A full-width line of dim, smaller text: a hint under the rows.
pub fn hint(ui: &mut Ui, t: &Tokens, text: &str) {
    ui.add(egui::Label::new(RichText::new(text).size(FONT_CAPTION).color(t.form_dim)).wrap());
}

/// A dim one-line caption in the control column of the row labelled `aligned_to`, under its
/// control: says in words what a colour or an icon shows.
pub fn caption(ui: &mut Ui, t: &Tokens, aligned_to: &str, text: &str) {
    caption_in(ui, aligned_to, text, t.form_dim);
}

/// [`caption`] in the warning colour, for what is wrong with the row's value.
pub fn warning_caption(ui: &mut Ui, t: &Tokens, aligned_to: &str, text: &str) {
    caption_in(ui, aligned_to, text, t.warning);
}

fn caption_in(ui: &mut Ui, aligned_to: &str, text: &str, color: egui::Color32) {
    let width = ui.available_width().max(0.0);
    let (rect, _) = ui.allocate_exact_size(vec2(width, FORM_ICON_SIZE), egui::Sense::hover());
    let text_rect = Rect::from_min_max(pos2(control_left(ui, aligned_to, rect), rect.top()), rect.max);
    paint_text(ui, text_rect, text, FontId::proportional(FONT_SMALL), color);
}

/// One line of `text` in `rect`, left aligned and centred down it, cut short with an ellipsis to
/// fit, on whole device pixels (a fractional origin blurs every glyph).
pub(super) fn paint_text(ui: &Ui, rect: Rect, text: &str, font: FontId, ink: egui::Color32) {
    let mut job = egui::text::LayoutJob::single_section(text.to_owned(), egui::TextFormat::simple(font, ink));
    job.wrap.max_width = rect.width().max(0.0);
    job.wrap.max_rows = 1;
    let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
    let ppp = ui.ctx().pixels_per_point().max(0.5);
    let snap = |v: f32| (v * ppp).round() / ppp;
    let at = pos2(snap(rect.left()), snap(rect.center().y - galley.size().y / 2.0));
    ui.painter().with_clip_rect(ui.clip_rect().intersect(rect)).galley(at, galley, ink);
}

#[cfg(test)]
mod tests {
    use egui::{Pos2, Rect, vec2};

    use super::*;

    /// The control column's left edge of each row, after two frames (the column settles in two).
    fn columns(width: f32, labels: &[&str]) -> Vec<f32> {
        let ctx = egui::Context::default();
        let t = Tokens::for_dark(false);
        let mut lefts = Vec::new();
        for _ in 0..3 {
            lefts.clear();
            let raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1000.0, 600.0))), ..Default::default() };
            let mut out = ctx.run_ui(raw, |ui| {
                ui.set_max_width(width);
                scope(ui, &t, |ui| {
                    for label in labels {
                        lefts.push(row(ui, &t, label, |ui| ui.max_rect().left()));
                    }
                });
            });
            out.textures_delta.clear();
        }
        lefts
    }

    #[test]
    fn rows_share_one_label_column_widened_for_the_longest_label() {
        let short = columns(240.0, &["X", "Y"]);
        assert_eq!(short, [FORM_LABEL_WIDTH + SPACE_8; 2], "the 70 pt basis and the 8 pt gap");
        let long = columns(240.0, &["X", "A rather long label"]);
        assert_eq!(long[0], long[1], "one column for the card");
        assert!(long[0] > short[0] && long[0] <= FORM_LABEL_WIDTH * LABEL_COLUMN_MAX + SPACE_8, "widened, at most 1.6 times");
        let wide = columns(456.0, &["X"]);
        assert_eq!(wide, [FORM_LABEL_WIDTH + SPACE_24 + SPACE_10 + SPACE_10], "a wide card's 104 pt basis and 10 pt gap");
    }
}
