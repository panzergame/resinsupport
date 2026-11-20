// Ported from SolidCraft (MIT OR Apache-2.0), crates/ui-kit/src/tree.rs (`tree_row`: a compact
// scene row's selection and hover fills, its inset and its trailing quiet action) at a34756830;
// see ATTRIBUTION.md. Reduced to one level: no folding, eye, radio, badges, drag or rename.

//! A row of a list of things (the models): its name, selected or not, and one action at its end.

use egui::{FontId, Rect, Response, Sense, Ui, pos2, vec2};

use super::layout::paint_text;
use crate::icons;
use crate::theme::Tokens;
use crate::theme::metrics::*;

/// What was done to a [`list_item`].
pub struct ListItem {
    /// The row itself, for its click and tooltip.
    pub row: Response,
    /// Whether its action was clicked.
    pub action: bool,
}

/// A `CONTROL_HEIGHT` row: `label` (cut short to fit: the caller's tooltip on [`ListItem::row`]
/// names it whole), filled when `selected` or hovered, and the quiet `icon` action at its end,
/// shown while the row is hovered or selected, `tip` on it.
pub fn list_item(
    ui: &mut Ui,
    t: &Tokens,
    id: impl std::hash::Hash + std::fmt::Debug,
    label: &str,
    selected: bool,
    icon: &str,
    tip: &str,
) -> ListItem {
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width().max(0.0), CONTROL_HEIGHT), Sense::hover());
    let row = ui.interact(r, ui.id().with(("list_item", &id)), Sense::click());
    super::mark(ui.layer_id(), "list_item", r, label);
    if selected {
        ui.painter().rect_filled(r, RADIUS_SM.saturating_sub(1), t.accent_soft);
    } else if row.hovered() {
        ui.painter().rect_filled(r, RADIUS_SM.saturating_sub(1), t.hover);
    }
    let action_rect = Rect::from_center_size(pos2(r.right() - SPACE_8 - SPACE_16 / 2.0, r.center().y), vec2(SPACE_16 + SPACE_4, SPACE_24));
    let hovered = ui.rect_contains_pointer(r);
    let action = ui.interact(action_rect, ui.id().with(("list_item_action", &id)), Sense::click());
    if hovered || selected {
        if action.hovered() {
            ui.painter().rect_filled(action_rect.shrink2(vec2(0.0, SPACE_3)), RADIUS_SM.saturating_sub(1), t.hover);
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        let glyph = Rect::from_center_size(action_rect.center(), vec2(SPACE_16, SPACE_16));
        icons::paint(ui.painter(), glyph, icon, t.dark, if action.hovered() { 1.0 } else { 0.6 });
    }
    let ink = if selected { t.selected_ink } else { t.text };
    let text = Rect::from_min_max(pos2(r.left() + SPACE_8, r.top()), pos2(action_rect.left() - SPACE_4, r.bottom()));
    paint_text(ui, text, label, FontId::proportional(FONT_LABEL), ink);
    row.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, ui.is_enabled(), selected, label));
    action.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), tip));
    let clicked = (hovered || selected) && action.on_hover_text(tip).clicked();
    ListItem { row, action: clicked }
}
