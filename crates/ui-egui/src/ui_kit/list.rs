// Ported from SolidCraft (MIT OR Apache-2.0), crates/ui-kit/src/tree.rs (`tree_row`: a compact
// scene row's selection and hover fills, its inset, its faded icon and the eye at its end) and
// crates/ui-egui/src/browser.rs (`inline_rename`: the name edited in its row) at a34756830; see
// ATTRIBUTION.md. Reduced to one level: no folding, radio, badges, drag, eye drag or status.

//! A row of a list of things (the models): its icon, its name, selected or not, and its eye;
//! and the same row while its name is edited.

use egui::{FontId, Pos2, Rect, Response, Sense, Ui, pos2, vec2};

use super::fields::text_input;
use super::layout::paint_text;
use crate::theme::Tokens;
use crate::theme::metrics::*;
use crate::{i18n, icons};

/// A hidden row's icon and eye, as faint as its name.
const DIM_ICON_OPACITY: f32 = 0.4;
/// A shown row's eye, a little quieter than its name.
const EYE_OPACITY: f32 = 0.85;

/// What a [`browser_row`] shows.
pub struct BrowserRow<'a> {
    /// An icon from [`crate::icons`] before the name.
    pub icon: &'a str,
    /// Cut short to fit: the caller's tooltip on [`BrowserRowResponse::row`] names it whole.
    pub label: &'a str,
    pub selected: bool,
    /// Its eye open; a hidden row is faded and its eye struck through.
    pub visible: bool,
}

/// What was done to a [`browser_row`].
pub struct BrowserRowResponse {
    /// The row itself, for its tooltip and context menu.
    pub row: Response,
    /// Clicked away from the eye.
    pub clicked: bool,
    /// Double-clicked away from the eye.
    pub double_clicked: bool,
    /// The eye was clicked.
    pub eye: bool,
}

/// The row's eye: a target as tall as the row at its end.
fn eye_rect(r: Rect) -> Rect {
    let glyph = SPACE_12 + SPACE_3;
    Rect::from_center_size(pos2(r.right() - SPACE_6 - glyph / 2.0, r.center().y), vec2(SPACE_16 + SPACE_3, r.height()))
}

/// The row's icon, `SPACE_16` square after a `SPACE_6` inset.
fn icon_rect(r: Rect) -> Rect {
    Rect::from_min_size(pos2(r.left() + SPACE_6, r.center().y - SPACE_16 / 2.0), vec2(SPACE_16, SPACE_16))
}

/// A `TREE_ROW_HEIGHT` row: `icon`, the name, filled when selected or hovered, and the eye at its
/// end (its tooltip says what a click does).
pub fn browser_row(ui: &mut Ui, t: &Tokens, id: impl std::hash::Hash + std::fmt::Debug, row: &BrowserRow) -> BrowserRowResponse {
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width().max(0.0), TREE_ROW_HEIGHT), Sense::hover());
    let response = ui.interact(r, ui.id().with(("browser_row", &id)), Sense::click());
    super::mark(ui.layer_id(), "browser_row", r, row.label);
    if row.selected {
        ui.painter().rect_filled(r, RADIUS_SM.saturating_sub(1), t.accent_soft);
    } else if response.hovered() {
        ui.painter().rect_filled(r, RADIUS_SM.saturating_sub(1), t.hover);
    }
    let eye = eye_rect(r);
    let on_eye = |p: Option<Pos2>| p.is_some_and(|p| eye.contains(p));
    let eye_hovered = on_eye(response.hover_pos());
    let faded = if row.visible { 1.0 } else { DIM_ICON_OPACITY };
    icons::paint(ui.painter(), icon_rect(r), row.icon, t.dark, faded);
    let glyph = Rect::from_center_size(eye.center(), vec2(SPACE_12 + SPACE_3, SPACE_12 + SPACE_3));
    let eye_opacity = if eye_hovered {
        1.0
    } else if row.visible {
        EYE_OPACITY
    } else {
        DIM_ICON_OPACITY
    };
    icons::paint(ui.painter(), glyph, if row.visible { "eye" } else { "suppress" }, t.dark, eye_opacity);
    let ink = if row.selected {
        t.selected_ink
    } else if row.visible {
        t.form_ink
    } else {
        t.form_dim
    };
    let text = Rect::from_min_max(pos2(icon_rect(r).right() + SPACE_6, r.top()), pos2(eye.left() - SPACE_4, r.bottom()));
    paint_text(ui, text, row.label, FontId::proportional(FONT_LABEL), ink);
    if eye_hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, ui.is_enabled(), row.selected, row.label));
    let pressed_at = response.interact_pointer_pos();
    let eye_clicked = response.clicked() && on_eye(pressed_at);
    let out = BrowserRowResponse {
        clicked: response.clicked() && !on_eye(pressed_at),
        double_clicked: response.double_clicked() && !on_eye(pressed_at),
        eye: eye_clicked,
        row: response.clone(),
    };
    if eye_hovered {
        let mut at_eye = response;
        (at_eye.rect, at_eye.interact_rect) = (eye, eye);
        at_eye.on_hover_text(i18n::tr(if row.visible { "ui.hide" } else { "ui.show" }));
    }
    out
}

/// How a [`rename_row`] ended this frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenameEnd {
    /// Still being typed in.
    Editing,
    /// Enter, or a click elsewhere: keep the name.
    Commit,
    /// Esc: put the name back.
    Cancel,
}

/// A [`browser_row`], selected, whose name is a field: `text` all selected on its `fresh` first
/// frame, so typing replaces it. At most `limit` characters.
pub fn rename_row(
    ui: &mut Ui,
    t: &Tokens,
    id: impl std::hash::Hash + std::fmt::Debug,
    row: &BrowserRow,
    text: &mut String,
    fresh: &mut bool,
    limit: usize,
) -> RenameEnd {
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width().max(0.0), TREE_ROW_HEIGHT), Sense::hover());
    super::mark(ui.layer_id(), "browser_row", r, row.label);
    ui.painter().rect_filled(r, RADIUS_SM.saturating_sub(1), t.accent_soft);
    let faded = if row.visible { 1.0 } else { DIM_ICON_OPACITY };
    icons::paint(ui.painter(), icon_rect(r), row.icon, t.dark, faded);
    let eye = eye_rect(r);
    let glyph = Rect::from_center_size(eye.center(), vec2(SPACE_12 + SPACE_3, SPACE_12 + SPACE_3));
    icons::paint(ui.painter(), glyph, if row.visible { "eye" } else { "suppress" }, t.dark, if row.visible { EYE_OPACITY } else { faded });
    let field = Rect::from_min_max(pos2(icon_rect(r).right() + SPACE_3, r.top() + SPACE_2), pos2(eye.left() - SPACE_4, r.bottom() - SPACE_2));
    let edit_id = ui.id().with(("browser_rename", &id));
    // A child the row's layout never sees: the row has taken its height already.
    let mut field_ui = ui.new_child(egui::UiBuilder::new().max_rect(field));
    let mut output = text_input(egui::TextEdit::singleline(text))
        .id(edit_id)
        .char_limit(limit)
        .margin(vec2(SPACE_3, 0.0))
        .min_size(field.size())
        .desired_width(field.width())
        .background_color(t.field)
        .clip_text(true)
        .show(&mut field_ui);
    super::mark(ui.layer_id(), "text_field", field, row.label);
    if *fresh {
        *fresh = false;
        output.response.request_focus();
        output.state.cursor.set_char_range(Some(egui::text::CCursorRange::select_all(&output.galley)));
        output.state.store(ui.ctx(), edit_id);
        return RenameEnd::Editing;
    }
    if !output.response.lost_focus() {
        return RenameEnd::Editing;
    }
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) { RenameEnd::Cancel } else { RenameEnd::Commit }
}
