// Ported from SolidCraft (MIT OR Apache-2.0), crates/ui-egui/src/ui_kit/panel.rs (`header`,
// `Panel::feature`) and ui_kit/actions.rs (`footer`, `action_button`, `icon_button_at`) at
// 4bdffef0; the card's scrolling body (`Panel::feature_impl`) and `page_list` from
// crates/ui-kit/src/panel.rs at a34756830; see ATTRIBUTION.md. Reduced: no docking, top buttons,
// compute status, page groups or scenario handles.

//! SolidCraft's UI kit, ported reduced: the card every dialog uses (a 40 pt header with icon,
//! title and close, a padded body of form rows that scrolls when the viewport is too short, and a
//! 48 pt footer with OK and Cancel, or with Restore defaults and Close; a permanent panel has
//! neither footer nor close), and every control a card, a menu or a list holds. Feature modules
//! draw their controls with these, never with egui's own (`raw_widgets_stay_in_the_kit`).
//! Colours and sizes come from [`crate::theme`], words from [`crate::i18n`].

mod checks;
mod choice;
mod confirm;
mod fields;
mod figure;
mod layout;
mod list;
mod menu;
mod preset_row;
mod section;
mod status;

pub use checks::{check_row, checkbox, checkbox_indeterminate, mixed_check_row};
pub use choice::segmented;
pub use confirm::Confirm;
pub use fields::{
    choice_row, choice_row_help, dropdown, dropdown_item, dropdown_menu, multiline_text_field, number_field, number_fields, parse_number, slider,
    text_field,
};
pub use figure::{FigureField, FigureRow, FigureSlot, figure_height, figure_rows};
pub use layout::{StatusTone, action_row, caption, hint, row, scope, sized_row, status_line, value_row, warning_caption};
pub use list::{ListItem, list_item};
pub use menu::{MenuItem, menu_list, menu_separator};
pub use preset_row::{PresetAction, PresetRow, PresetRowState, PresetRows};
pub use section::section;
pub use status::progress;

use egui::{Align2, CornerRadius, FontId, Pos2, Rect, Response, Sense, Stroke, Ui, pos2, vec2};

use crate::theme::Tokens;
use crate::theme::metrics::*;
use crate::{i18n, icons};

/// What the user did with a card this frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    None,
    Ok,
    Cancel,
    /// Restore defaults.
    Reset,
}

/// The footer's buttons.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Buttons {
    /// OK (primary, `enabled` when the inputs make sense) and Cancel. Enter is OK.
    OkCancel { ok_enabled: bool },
    /// Restore defaults on the left, Close (primary, [`Action::Ok`]) on the right, for settings
    /// that apply as they change.
    ResetClose,
    /// A permanent panel, shown while its mode lasts: no footer, no close button, and Esc leaves
    /// it alone. Its edits apply as they are made.
    None,
    /// A panel the user opens and closes: no footer, the header's close button
    /// ([`Action::Cancel`]), and Esc leaves it alone, as for [`Buttons::None`].
    CloseOnly,
    /// A confirmation: the action (primary, a catalog id, [`Action::Ok`]) and Cancel.
    Confirm { ok: &'static str },
    /// A report: Close alone (primary, [`Action::Ok`]); Enter is Close too, Esc and the header's
    /// close button Cancel.
    Close,
}

pub struct Card<'a> {
    pub id: egui::Id,
    pub title: &'a str,
    /// An icon from [`crate::icons`]; empty for the accent dot.
    pub icon: &'a str,
    /// Where the card first opens (its top left).
    pub at: Pos2,
    /// What it stays inside (the viewport), so it never covers the ribbon.
    pub bounds: Option<Rect>,
    /// The card's width ([`FORM_CARD_WIDTH`] for a tool).
    pub width: f32,
    pub buttons: Buttons,
}

impl Card<'_> {
    /// Show the card; Enter is OK, Esc (outside a field being typed in) and the close button are
    /// Cancel.
    pub fn show(self, ctx: &egui::Context, t: &Tokens, body: impl FnOnce(&mut Ui)) -> Action {
        self.show_window(ctx, t, None, body)
    }

    /// [`Card::show`], pinned by its `corner` to the same corner of `bounds` (else the screen)
    /// moved by `offset`:
    /// a panel that stays put, such as one at the viewport's bottom left, whatever its height.
    pub fn show_anchored(self, ctx: &egui::Context, t: &Tokens, corner: Align2, offset: egui::Vec2, body: impl FnOnce(&mut Ui)) -> Action {
        self.show_window(ctx, t, Some((corner, offset)), body)
    }

    fn show_window(self, ctx: &egui::Context, t: &Tokens, anchor: Option<(Align2, egui::Vec2)>, body: impl FnOnce(&mut Ui)) -> Action {
        let typing = ctx.text_edit_focused();
        let frame = card_frame(t);
        let permanent = matches!(self.buttons, Buttons::None | Buttons::CloseOnly);
        let mut action = Action::None;
        let room = self.bounds.unwrap_or_else(|| ctx.content_rect());
        // A window that does not resize offers its content its last height: as tall as the room,
        // the body's scroll area can grow, while the card is still drawn at its content's height.
        let mut window = egui::Window::new(self.title).min_height(room.height());
        if let Some(b) = self.bounds {
            window = window.constrain_to(b);
        }
        if let Some((corner, offset)) = anchor {
            window = window.anchor(corner, offset);
        }
        window.id(self.id).title_bar(false).resizable(false).collapsible(false).frame(frame).default_pos(self.at).show(ctx, |ui| {
            ui.set_width(self.width);
            ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
            if header(ui, t, self.icon, self.title, self.buttons != Buttons::None) {
                action = Action::Cancel;
            }
            // The body scrolls rather than push the footer past the viewport's bottom.
            let bottom = room.bottom() - SPACE_12 - 1.0;
            let footer_height = if permanent { 0.0 } else { FORM_FOOTER_HEIGHT };
            let body_height = (bottom - ui.cursor().top() - footer_height).max(CONTROL_HEIGHT * 2.0);
            egui::ScrollArea::vertical()
                .id_salt("card_body")
                .min_scrolled_height(0.0)
                .max_height(body_height)
                .auto_shrink([false, true])
                .show(ui, |ui| egui::Frame::NONE.inner_margin(SPACE_12).show(ui, |ui| scope(ui, t, body)));
            if !permanent {
                let clicked = footer(ui, t, self.buttons);
                if clicked != Action::None {
                    action = clicked;
                }
            }
        });
        // Read after the body: an Enter that ends typing in a field has already set its value.
        let enter_ok = matches!(self.buttons, Buttons::OkCancel { ok_enabled: true } | Buttons::Close);
        ctx.input(|i| {
            if i.key_pressed(egui::Key::Enter) && enter_ok {
                action = Action::Ok;
            }
            if i.key_pressed(egui::Key::Escape) && !typing && !permanent {
                action = Action::Cancel;
            }
        });
        action
    }
}

/// A card's frame: the dialog fill, border, corners and shadow.
fn card_frame(t: &Tokens) -> egui::Frame {
    egui::Frame::NONE
        .fill(t.dialog_body)
        .stroke(Stroke::new(1.0, t.dialog_border))
        .corner_radius(RADIUS_XL)
        .shadow(t.shell_shadow())
        .inner_margin(0.0)
}

/// The 40 pt header: 12 pt inset, 19 pt icon, 8 pt gap, the title (cut short to fit, whole in a
/// tooltip), a 26 pt close target (when `closable`) and a rule under it. Whether close was clicked.
fn header(ui: &mut Ui, t: &Tokens, icon: &str, title: &str, closable: bool) -> bool {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), FORM_HEADER_HEIGHT), Sense::hover());
    let icon_rect =
        Rect::from_center_size(pos2(rect.left() + SPACE_12 + FORM_ICON_SIZE / 2.0, rect.center().y), vec2(FORM_ICON_SIZE, FORM_ICON_SIZE));
    if icon.is_empty() {
        ui.painter().circle_filled(icon_rect.center(), SPACE_3, t.accent);
    } else {
        icons::paint(ui.painter(), icon_rect, icon, t.dark, 1.0);
    }
    let title_left = icon_rect.right() + SPACE_8;
    let title_right = if closable { rect.right() - SPACE_8 - FORM_CLOSE_SIZE - SPACE_8 } else { rect.right() - SPACE_12 };
    let mut job = egui::text::LayoutJob::simple_singleline(title.to_owned(), FontId::proportional(FONT_HEADING), t.form_ink);
    job.wrap = egui::text::TextWrapping::truncate_at_width((title_right - title_left).max(0.0));
    let galley = ui.painter().layout_job(job);
    let title_rect = Rect::from_min_size(pos2(title_left, rect.center().y - galley.size().y / 2.0), galley.size());
    let elided = galley.elided;
    ui.painter().galley(title_rect.min, galley, t.form_ink);
    if elided {
        ui.interact(title_rect, ui.id().with("title"), Sense::hover()).on_hover_text(title);
    }
    ui.painter().hline(rect.x_range(), rect.bottom() - 0.5, Stroke::new(1.0, t.dialog_rule));
    closable && close_button(ui, t, pos2(rect.right() - SPACE_8 - FORM_CLOSE_SIZE / 2.0, rect.center().y))
}

/// The 26 pt close target centred on `at`. Whether it was clicked.
fn close_button(ui: &mut Ui, t: &Tokens, at: Pos2) -> bool {
    let close_rect = Rect::from_center_size(at, vec2(FORM_CLOSE_SIZE, FORM_CLOSE_SIZE));
    let close = ui.interact(close_rect, ui.id().with("close"), Sense::click());
    if close.hovered() {
        ui.painter().rect_filled(close_rect, RADIUS_SM, t.hover);
    }
    icons::paint(ui.painter(), close_rect.shrink(6.0), "close", t.dark, 1.0);
    close.on_hover_text(i18n::tr("ui.close")).clicked()
}

/// The 48 pt footer band: its own fill, a rule on top, the primary action and Cancel
/// right-aligned with 10 pt above and below and 12 pt at the sides; Restore defaults on the left.
fn footer(ui: &mut Ui, t: &Tokens, buttons: Buttons) -> Action {
    let (band, _) = ui.allocate_exact_size(vec2(ui.available_width(), FORM_FOOTER_HEIGHT), Sense::hover());
    let r = RADIUS_XL.saturating_sub(1);
    ui.painter().rect_filled(band, CornerRadius { nw: 0, ne: 0, sw: r, se: r }, t.dialog_footer);
    ui.painter().hline(band.x_range(), band.top() + 0.5, Stroke::new(1.0, t.dialog_rule));
    let inner = band.shrink2(vec2(SPACE_12, SPACE_10));
    let mut right = inner.right();
    let mut action = Action::None;
    let (row, ok_enabled): (Vec<(&'static str, Action, bool)>, bool) = match buttons {
        Buttons::OkCancel { ok_enabled } => (vec![("ui.ok", Action::Ok, true), ("ui.cancel", Action::Cancel, false)], ok_enabled),
        Buttons::ResetClose | Buttons::Close => (vec![("ui.close", Action::Ok, true)], true),
        Buttons::Confirm { ok } => (vec![(ok, Action::Ok, true), ("ui.cancel", Action::Cancel, false)], true),
        Buttons::None | Buttons::CloseOnly => (Vec::new(), true),
    };
    let width_of = |ui: &Ui, label: &str, primary: bool| {
        let text = ui.painter().layout_no_wrap(label.into(), FontId::proportional(FONT_LABEL), t.form_ink).size().x;
        (text + 2.0 * if primary { SPACE_16 } else { SPACE_12 }).max(if primary { SPACE_24 * 3.0 } else { 0.0 })
    };
    let button_rect = |left: f32, right: f32| {
        Rect::from_min_max(pos2(left, inner.center().y - FORM_BUTTON_HEIGHT / 2.0), pos2(right, inner.center().y + FORM_BUTTON_HEIGHT / 2.0))
    };
    for &(id, kind, primary) in &row {
        let label = i18n::tr(id);
        let rect = button_rect(right - width_of(ui, label, primary), right);
        right = rect.left() - SPACE_6;
        let enabled = !primary || ok_enabled;
        if action_button(ui, t, rect, label, primary, enabled).clicked() && enabled {
            action = kind;
        }
    }
    if buttons == Buttons::ResetClose {
        let label = i18n::tr("ui.restore_defaults");
        let rect = button_rect(inner.left(), inner.left() + width_of(ui, label, false));
        if action_button(ui, t, rect, label, false, true).clicked() {
            action = Action::Reset;
        }
    }
    action
}

/// One OK or Cancel button: OK filled with the accent, Cancel outlined.
fn action_button(ui: &mut Ui, t: &Tokens, rect: Rect, label: &str, primary: bool, enabled: bool) -> Response {
    let response = ui.interact(rect, ui.id().with(("action", label)), if enabled { Sense::click() } else { Sense::hover() });
    let hot = enabled && response.hovered();
    let (fill, stroke, ink) = if primary && enabled {
        (if hot { t.form_primary.gamma_multiply(0.85) } else { t.form_primary }, Stroke::NONE, t.on_accent)
    } else {
        (if hot { t.hover } else { t.control_fill }, Stroke::new(1.0, t.control_border), if enabled { t.form_ink } else { t.form_dim })
    };
    ui.painter().rect(rect, CONTROL_RADIUS, fill, stroke, egui::StrokeKind::Inside);
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, label, FontId::proportional(FONT_LABEL), ink);
    if hot {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    response
}

/// A text button in the form's style, `CONTROL_HEIGHT` tall.
pub fn button(ui: &mut Ui, label: &str) -> Response {
    ui.add(egui::Button::new(label).min_size(vec2(0.0, CONTROL_HEIGHT)))
}

/// An icon action, `CONTROL_HEIGHT` square, beside a row's control: filled when hovered, `tip` on
/// hover, dimmed and inert when not `enabled`.
pub fn icon_button(ui: &mut Ui, t: &Tokens, icon: &str, tip: &str, enabled: bool) -> Response {
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, response) = ui.allocate_exact_size(vec2(CONTROL_HEIGHT, CONTROL_HEIGHT), sense);
    if enabled && response.hovered() {
        ui.painter().rect_filled(rect, CONTROL_RADIUS, t.hover);
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    icons::paint(ui.painter(), rect.shrink(SPACE_6), icon, t.dark, if enabled { 1.0 } else { 0.35 });
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, tip));
    response.on_hover_text(tip)
}

/// A settings window's page list: one `CONTROL_HEIGHT + SPACE_4` row per page (title, icon),
/// filled when selected or hovered, `selected` the page shown. Whether a page was clicked.
pub fn page_list(ui: &mut Ui, t: &Tokens, selected: &mut usize, pages: &[(&str, &str)]) -> bool {
    let mut clicked = false;
    ui.spacing_mut().item_spacing.y = SPACE_2;
    for (index, (title, icon)) in pages.iter().enumerate() {
        let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), CONTROL_HEIGHT + SPACE_4), Sense::click());
        if response.clicked() {
            *selected = index;
            clicked = true;
        }
        let on = *selected == index;
        if on || response.hovered() || response.has_focus() {
            ui.painter().rect_filled(rect, CONTROL_RADIUS, if on { t.selected_bg } else { t.hover });
        }
        let icon_rect = Rect::from_center_size(pos2(rect.left() + SPACE_10 + SPACE_8, rect.center().y), vec2(SPACE_16, SPACE_16));
        icons::paint(ui.painter(), icon_rect, icon, t.dark, 1.0);
        let ink = if on { t.form_primary } else { t.form_ink };
        let text = Rect::from_min_max(pos2(icon_rect.right() + SPACE_10, rect.top()), rect.right_bottom());
        layout::paint_text(ui, text, title, FontId::proportional(FONT_LABEL), ink);
        response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, ui.is_enabled(), on, *title));
        if response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
    }
    clicked
}

#[cfg(test)]
mod tests {
    use egui::{Pos2, Rect};

    use super::*;

    /// The card's height after a few frames, its body `rows` rows tall, in a `room` points tall viewport.
    fn card_height(rows: usize, room: f32) -> f32 {
        let ctx = egui::Context::default();
        let t = Tokens::for_dark(false);
        let bounds = Rect::from_min_size(Pos2::ZERO, vec2(800.0, room));
        for _ in 0..4 {
            let raw = egui::RawInput { screen_rect: Some(bounds), ..Default::default() };
            let mut out = ctx.run_ui(raw, |ui| {
                let card = Card {
                    id: egui::Id::new("tall"),
                    title: "",
                    icon: "",
                    at: Pos2::ZERO,
                    bounds: Some(bounds),
                    width: 240.0,
                    buttons: Buttons::None,
                };
                card.show(ui.ctx(), &t, |ui| {
                    for _ in 0..rows {
                        row(ui, &t, "Row", |_| {});
                    }
                });
            });
            out.textures_delta.clear();
        }
        ctx.memory(|m| m.area_rect(egui::Id::new("tall"))).map_or(0.0, |r| r.height())
    }

    #[test]
    fn a_tall_body_shows_whole_when_there_is_room_and_scrolls_when_there_is_not() {
        let rows = 20;
        let whole = FORM_HEADER_HEIGHT + 2.0 * SPACE_12 + rows as f32 * CONTROL_HEIGHT + (rows - 1) as f32 * FORM_GAP_Y;
        let roomy = card_height(rows, 1000.0);
        assert!((roomy - whole).abs() < 4.0, "whole, past egui's first 420 pt: {roomy} vs {whole}");
        let short = card_height(rows, 400.0);
        assert!(short <= 400.0 - SPACE_12 + 1.0, "within the viewport: {short}");
    }
}
