// Ported from SolidCraft (MIT OR Apache-2.0), crates/ui-egui/src/ui_kit/panel.rs (`header`,
// `Panel::feature`), ui_kit/actions.rs (`footer`, `action_button`) and ui_kit/layout.rs (`scope`,
// `row`) at 4bdffef0; see ATTRIBUTION.md. Reduced: no docking, top buttons, compute status or
// scenario handles.

//! The card every dialog uses: a 40 pt header (icon, title, close), a padded body of form rows,
//! and a 48 pt footer with OK and Cancel, or with Restore defaults and Close; a permanent panel has
//! neither footer nor close. Colours and sizes come from [`crate::theme`], words from
//! [`crate::i18n`].

mod choice;
mod confirm;
mod figure;

pub use choice::segmented;
pub use confirm::Confirm;
pub use figure::{FigureField, FigureRow, FigureSlot, figure_height, figure_rows};

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
    /// A confirmation: the action (primary, a catalog id, [`Action::Ok`]) and Cancel.
    Confirm { ok: &'static str },
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
        let typing = ctx.text_edit_focused();
        let frame = card_frame(t);
        let permanent = self.buttons == Buttons::None;
        let mut action = Action::None;
        let mut window = egui::Window::new(self.title);
        if let Some(b) = self.bounds {
            window = window.constrain_to(b);
        }
        window.id(self.id).title_bar(false).resizable(false).collapsible(false).frame(frame).default_pos(self.at).show(ctx, |ui| {
            ui.set_width(self.width);
            ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
            if header(ui, t, self.icon, self.title, !permanent) {
                action = Action::Cancel;
            }
            egui::Frame::NONE.inner_margin(SPACE_12).show(ui, |ui| scope(ui, t, body));
            if !permanent {
                let clicked = footer(ui, t, self.buttons);
                if clicked != Action::None {
                    action = clicked;
                }
            }
        });
        // Read after the body: an Enter that ends typing in a field has already set its value.
        let enter_ok = matches!(self.buttons, Buttons::OkCancel { ok_enabled: true });
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

/// The 40 pt header: 12 pt inset, 19 pt icon, 8 pt gap, the title, a 26 pt close target (when
/// `closable`) and a rule under it. Whether close was clicked.
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
    ui.painter().text(pos2(title_left, rect.center().y), Align2::LEFT_CENTER, title, FontId::proportional(FONT_HEADING), t.form_ink);
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
        Buttons::ResetClose => (vec![("ui.close", Action::Ok, true)], true),
        Buttons::Confirm { ok } => (vec![(ok, Action::Ok, true), ("ui.cancel", Action::Cancel, false)], true),
        Buttons::None => (Vec::new(), true),
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

/// Form styling for a card's body: label-size text, 28 pt controls, the chrome's control fills.
pub fn scope<R>(ui: &mut Ui, t: &Tokens, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing = vec2(SPACE_4, FORM_GAP_Y);
        ui.spacing_mut().interact_size.y = CONTROL_HEIGHT;
        ui.spacing_mut().button_padding = vec2(SPACE_8, SPACE_4);
        ui.style_mut().text_styles.insert(egui::TextStyle::Body, FontId::proportional(FONT_LABEL));
        ui.style_mut().text_styles.insert(egui::TextStyle::Button, FontId::proportional(FONT_LABEL));
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
            state.corner_radius = CornerRadius::same(CONTROL_RADIUS);
        }
        v.widgets.hovered.bg_stroke = Stroke::new(1.0, t.form_primary);
        v.widgets.active.bg_stroke = Stroke::new(1.0, t.form_primary);
        add(ui)
    })
    .inner
}

/// One form row, `CONTROL_HEIGHT` tall: `label` in the 70 pt label column, the control in the
/// rest.
pub fn row<R>(ui: &mut Ui, t: &Tokens, label: &str, control: impl FnOnce(&mut Ui) -> R) -> R {
    sized_row(ui, t, label, FORM_LABEL_WIDTH, control)
}

/// A row with the wide (manager) label column, for settings whose names are longer.
pub fn wide_row<R>(ui: &mut Ui, t: &Tokens, label: &str, control: impl FnOnce(&mut Ui) -> R) -> R {
    sized_row(ui, t, label, FORM_WIDE_LABEL_WIDTH, control)
}

/// A checkbox setting: the box and its label across the row, `help` on hover.
pub fn check_row(ui: &mut Ui, t: &Tokens, label: &str, help: &str, value: &mut bool) -> Response {
    mixed_check_row(ui, t, label, help, value, false)
}

/// [`check_row`] for a setting of several things that may differ: `mixed` shows the box neither
/// ticked nor clear.
pub fn mixed_check_row(ui: &mut Ui, _t: &Tokens, label: &str, help: &str, value: &mut bool, mixed: bool) -> Response {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), CONTROL_HEIGHT), Sense::hover());
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
        ui.add(egui::Checkbox::new(value, label).indeterminate(mixed))
    })
    .inner
    .on_hover_text(help)
}

/// A setting chosen from `options` (stored value, catalog id of its name), in a wide row.
pub fn choice_row(ui: &mut Ui, t: &Tokens, label: &str, id: &str, value: &mut String, options: &[(&str, &'static str)]) {
    let shown = options.iter().find(|(v, _)| *v == value.as_str()).map_or(value.clone(), |(_, name)| i18n::tr(name).to_string());
    wide_row(ui, t, label, |ui| {
        egui::ComboBox::from_id_salt(id).selected_text(shown).width(ui.available_width()).show_ui(ui, |ui| {
            for (v, name) in options {
                ui.selectable_value(value, (*v).to_string(), i18n::tr(name));
            }
        });
    });
}

fn sized_row<R>(ui: &mut Ui, t: &Tokens, label: &str, label_width: f32, control: impl FnOnce(&mut Ui) -> R) -> R {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), CONTROL_HEIGHT), Sense::hover());
    let label_rect = Rect::from_min_size(rect.min, vec2(label_width, rect.height()));
    ui.painter().text(label_rect.left_center(), Align2::LEFT_CENTER, label, FontId::proportional(FONT_LABEL), t.form_ink);
    let control_rect = Rect::from_min_max(pos2(label_rect.right(), rect.top()), rect.max);
    ui.scope_builder(egui::UiBuilder::new().max_rect(control_rect).layout(egui::Layout::left_to_right(egui::Align::Center)), control).inner
}

/// A full-width line of dim text (a hint under the rows).
pub fn hint(ui: &mut Ui, t: &Tokens, text: &str) {
    ui.add(egui::Label::new(egui::RichText::new(text).size(FONT_CAPTION).color(t.form_dim)).wrap());
}

/// A text button in the form's style, `CONTROL_HEIGHT` tall.
pub fn button(ui: &mut Ui, label: &str) -> Response {
    ui.add(egui::Button::new(label).min_size(vec2(0.0, CONTROL_HEIGHT)))
}

/// One entry of a settings window's page list: its icon and title, filled when selected. Whether
/// it was clicked.
pub fn page_tab(ui: &mut Ui, t: &Tokens, icon: &str, title: &str, selected: bool) -> bool {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), CONTROL_HEIGHT), Sense::click());
    if selected {
        ui.painter().rect_filled(rect, CONTROL_RADIUS, t.selected_bg);
    } else if resp.hovered() {
        ui.painter().rect_filled(rect, CONTROL_RADIUS, t.hover);
    }
    let icon_rect = Rect::from_center_size(pos2(rect.left() + SPACE_8 + 8.0, rect.center().y), vec2(16.0, 16.0));
    icons::paint(ui.painter(), icon_rect, icon, t.dark, 1.0);
    let ink = if selected { t.selected_ink } else { t.form_ink };
    ui.painter().text(pos2(icon_rect.right() + SPACE_8, rect.center().y), Align2::LEFT_CENTER, title, FontId::proportional(FONT_LABEL), ink);
    resp.clicked()
}
