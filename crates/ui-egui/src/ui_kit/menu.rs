// Ported from SolidCraft (MIT OR Apache-2.0), crates/ui-kit/src/menu.rs (`MenuItem::show`,
// `MenuItem::paint`, `Menu::separator`, `Menu::show`, `LIST_W`) at a34756830; see ATTRIBUTION.md. Reduced: no
// swatches, submenus, keyboard navigation or scenario handles, and a danger row's icon keeps its colours.

//! The rows of a popup menu: the application menu, a ribbon group's menu, a model's menu.

use egui::{FontId, Rect, Response, Sense, Stroke, Ui, pos2, vec2};

use super::checks::paint_check_glyph;
use crate::icons;
use crate::theme::Tokens;
use crate::theme::metrics::*;

/// A list menu's width.
const MENU_WIDTH: f32 = 250.0;

/// A popup's entries, `add`'s [`MenuItem`]s and [`menu_separator`]s, at the list menu's width.
pub fn menu_list<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    // The popup's own node (egui's area widget, `<layer>.with("move")`): a menu, named by the button
    // that opened it.
    ui.ctx().accesskit_node_builder(ui.layer_id().id.with("move"), |node| node.set_role(egui::accesskit::Role::Menu));
    ui.set_width(MENU_WIDTH - SPACE_6 * 2.0 - 2.0);
    ui.spacing_mut().item_spacing.y = 0.0;
    let inner = add(ui);
    super::mark(ui.layer_id(), "menu", ui.min_rect(), "");
    inner
}

/// One menu entry.
pub struct MenuItem<'a> {
    pub label: &'a str,
    /// The keys that do the same, right-aligned; empty for none.
    pub shortcut: &'a str,
    pub enabled: bool,
    /// `Some(true)` ticks the entry: the current one of a choice.
    pub checked: Option<bool>,
    /// An icon from [`crate::icons`] before the label; empty for none.
    pub icon: &'a str,
    /// It deletes something: its label and keys in the error colour.
    pub danger: bool,
}

impl<'a> MenuItem<'a> {
    pub fn new(label: &'a str) -> Self {
        Self { label, shortcut: "", enabled: true, checked: None, icon: "", danger: false }
    }

    pub fn shortcut(self, shortcut: &'a str) -> Self {
        Self { shortcut, ..self }
    }

    pub fn enabled(self, enabled: bool) -> Self {
        Self { enabled, ..self }
    }

    pub fn icon(self, icon: &'a str) -> Self {
        Self { icon, ..self }
    }

    pub fn danger(self) -> Self {
        Self { danger: true, ..self }
    }

    /// The entry's row, `CONTROL_HEIGHT + SPACE_2` tall across the menu; inert and faded when not
    /// enabled.
    pub fn show(&self, ui: &mut Ui, t: &Tokens) -> Response {
        // A popup first measures what it holds: filling its provisional width there would make
        // it as wide as the screen.
        let width = if ui.is_sizing_pass() {
            let label = ui.painter().layout_no_wrap(self.label.to_owned(), FontId::proportional(FONT_LABEL), t.form_ink).size().x;
            let shortcut = ui.painter().layout_no_wrap(self.shortcut.to_owned(), FontId::monospace(FONT_SMALL), t.form_dim).size().x;
            (CONTROL_HEIGHT + SPACE_2 + label + shortcut + SPACE_24).min(ui.available_width())
        } else {
            ui.available_width()
        };
        let (r, response) = ui.allocate_exact_size(vec2(width, CONTROL_HEIGHT + SPACE_2), if self.enabled { Sense::click() } else { Sense::hover() });
        super::mark(ui.layer_id(), "menu_item", r, self.label);
        self.paint(ui, t, r, &response);
        response
    }

    fn paint(&self, ui: &Ui, t: &Tokens, r: Rect, response: &Response) {
        let enabled = self.enabled && ui.is_enabled();
        let hot = enabled && (response.hovered() || response.has_focus());
        let (ink, dim) = if self.danger {
            (t.error, t.error)
        } else if hot {
            (t.form_primary, t.form_primary)
        } else {
            (t.form_ink, t.form_dim)
        };
        let mut painter = ui.painter().with_clip_rect(ui.clip_rect().intersect(r));
        if !enabled {
            painter.multiply_opacity(0.4);
        }
        if hot {
            painter.rect_filled(r, CONTROL_RADIUS, t.accent_soft);
        }
        let icon = Rect::from_center_size(pos2(r.left() + CONTROL_HEIGHT / 2.0, r.center().y), vec2(SPACE_16, SPACE_16));
        if self.checked == Some(true) {
            paint_check_glyph(&painter, icon, false, t.form_primary);
        } else if !self.icon.is_empty() {
            icons::paint(&painter, icon, self.icon, t.dark, 1.0);
        }
        let mut right = r.right() - SPACE_8;
        if !self.shortcut.is_empty() {
            let key = painter.layout_no_wrap(self.shortcut.to_owned(), FontId::monospace(FONT_SMALL), dim);
            painter.galley(pos2(right - key.size().x, r.center().y - key.size().y / 2.0), key.clone(), dim);
            right -= key.size().x + SPACE_8;
        }
        let left = r.left() + CONTROL_HEIGHT + SPACE_2;
        let mut job = egui::text::LayoutJob::simple_singleline(self.label.to_owned(), FontId::proportional(FONT_LABEL), ink);
        job.wrap.max_width = (right - left).max(0.0);
        job.wrap.max_rows = 1;
        let galley = painter.layout_job(job);
        painter.galley(pos2(left, r.center().y - galley.size().y / 2.0), galley, ink);
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, self.label));
        if hot && response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
    }
}

/// A rule between groups of menu entries.
pub fn menu_separator(ui: &mut Ui, t: &Tokens) {
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), SPACE_10 + 1.0), Sense::hover());
    super::mark(ui.layer_id(), "menu_separator", r, "");
    ui.painter().hline(r.x_range(), r.center().y, Stroke::new(1.0, t.dialog_rule));
}
