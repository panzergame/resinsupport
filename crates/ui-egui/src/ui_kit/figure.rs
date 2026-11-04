// Ported from SolidCraft (MIT OR Apache-2.0), crates/ui-egui/src/ui_kit/layout.rs (`figure_rows`,
// `FigureSlot`) at 4bdffef0; see ATTRIBUTION.md. Adapted: number fields (`DragValue`) instead of
// expression fields; no scenario handles.

use std::ops::RangeInclusive;

use egui::{Align2, DragValue, FontId, Painter, Rect, Sense, Ui, pos2, vec2};

use crate::theme::Tokens;
use crate::theme::metrics::*;

/// One number a [`figure_rows`] picture dimensions.
pub struct FigureField<'a> {
    /// The caption over the field (translated).
    pub label: &'a str,
    pub value: &'a mut f64,
    pub range: RangeInclusive<f64>,
    /// `" mm"` or `"°"`.
    pub suffix: &'a str,
    pub decimals: usize,
    pub speed: f64,
    /// Off: the field shows its value dimmed and takes no input.
    pub enabled: bool,
}

/// One row of a [`figure_rows`] column: a number under its caption, or a checkbox (a part the
/// picture shows or not).
pub enum FigureRow<'a> {
    Number(FigureField<'a>),
    Check { label: &'a str, help: &'a str, value: &'a mut bool },
}

impl<'a> From<FigureField<'a>> for FigureRow<'a> {
    fn from(f: FigureField<'a>) -> Self {
        FigureRow::Number(f)
    }
}

impl FigureRow<'_> {
    fn height(&self) -> f32 {
        match self {
            FigureRow::Number(_) => SLOT,
            FigureRow::Check { .. } => CONTROL_HEIGHT,
        }
    }
}

/// Where a [`figure_rows`] row went (in the order given): its control's rect, and whether it is
/// hovered, dragged or has the keyboard (its part of the figure lights up).
pub struct FigureSlot {
    pub rect: Rect,
    pub hot: bool,
}

const SLOT: f32 = FONT_SMALL + SPACE_2 + CONTROL_HEIGHT;

/// The height of `n` captioned fields stacked: a [`figure_rows`] `min_height` that keeps one
/// figure the same size across a card's tabs.
pub fn figure_height(n: usize) -> f32 {
    n as f32 * SLOT + n.saturating_sub(1) as f32 * SPACE_4
}

/// A figure in the label column beside a column of rows (number fields, each under its own
/// caption, and checkboxes), for a picture whose parts the rows size: the caller paints the
/// figure where the rows went, with leaders to them. At least four rows and `min_height` tall.
/// Whether a row changed.
pub fn figure_rows(
    ui: &mut Ui,
    t: &Tokens,
    id: &str,
    min_height: f32,
    rows: &mut [FigureRow<'_>],
    paint: impl FnOnce(&Painter, Rect, &[FigureSlot]),
) -> bool {
    let width = ui.available_width();
    let stack = rows.iter().map(FigureRow::height).sum::<f32>() + rows.len().saturating_sub(1) as f32 * SPACE_4;
    let height = stack.max(CONTROL_HEIGHT * 4.0).max(min_height);
    let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    let figure = Rect::from_min_size(rect.min, vec2((FORM_LABEL_WIDTH * 1.4).min(width * 0.45), height));
    let left = (figure.right() + SPACE_12).min(rect.right());
    let mut changed = false;
    let mut top = rect.top() + (height - stack) / 2.0;
    let mut slots = Vec::with_capacity(rows.len());
    for (i, row) in rows.iter_mut().enumerate() {
        let control = match row {
            FigureRow::Number(f) => {
                let caption = Rect::from_min_max(pos2(left, top), pos2(rect.right(), top + FONT_SMALL));
                ui.painter().text(caption.left_center(), Align2::LEFT_CENTER, f.label, FontId::proportional(FONT_SMALL), t.form_dim);
                Rect::from_min_max(pos2(left, caption.bottom() + SPACE_2), pos2(rect.right(), top + SLOT))
            }
            FigureRow::Check { .. } => Rect::from_min_max(pos2(left, top), pos2(rect.right(), top + CONTROL_HEIGHT)),
        };
        // A child, not a scope: a scope would move this Ui's cursor back up to the row.
        let mut child =
            ui.new_child(egui::UiBuilder::new().id_salt((id, i)).max_rect(control).layout(egui::Layout::left_to_right(egui::Align::Center)));
        let response = match row {
            FigureRow::Number(f) => {
                if !f.enabled {
                    child.disable();
                }
                child.add_sized(
                    control.size(),
                    DragValue::new(f.value).range(f.range.clone()).speed(f.speed).suffix(f.suffix).max_decimals(f.decimals),
                )
            }
            FigureRow::Check { label, help, value } => child.checkbox(value, *label).on_hover_text(*help),
        };
        changed |= response.changed();
        let hot = response.hovered() || response.has_focus() || response.dragged();
        slots.push(FigureSlot { rect: control, hot });
        top += row.height() + SPACE_4;
    }
    paint(ui.painter(), figure, &slots);
    changed
}
