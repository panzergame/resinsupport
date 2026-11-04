// Ported from SolidCraft (MIT OR Apache-2.0), crates/ui-egui/src/ui_kit/confirm.rs (`Confirm`) at
// 4bdffef0; see ATTRIBUTION.md. Reduced: one tone (warning), no left or secondary action, no
// detail list; on ResinSupport's card frame and footer.

use egui::{Align2, FontId, Pos2, Sense, pos2, vec2};

use super::{Action, Buttons, card_frame, close_button, footer, scope};
use crate::icons;
use crate::theme::Tokens;
use crate::theme::metrics::*;

/// A modal question over everything else: a warning disc with `icon`, the title and message, and
/// the action and Cancel. Nothing behind it takes input while it is open.
pub struct Confirm<'a> {
    pub id: egui::Id,
    pub icon: &'a str,
    pub title: &'a str,
    pub message: &'a str,
    /// Catalog id of the action's label ("Restore").
    pub ok: &'static str,
}

impl Confirm<'_> {
    /// [`Action::Ok`] for the action (its button, or Enter); [`Action::Cancel`] for Cancel, the
    /// close button, Esc or a click outside; else [`Action::None`].
    pub fn show(self, ctx: &egui::Context, t: &Tokens) -> Action {
        let response = egui::Modal::new(self.id).frame(card_frame(t)).backdrop_color(t.backdrop).show(ctx, |ui| {
            ui.set_width(FORM_CARD_WIDTH);
            ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
            let mut action = Action::None;
            egui::Frame::NONE.inner_margin(SPACE_16).show(ui, |ui| {
                scope(ui, t, |ui| {
                    ui.spacing_mut().item_spacing = vec2(SPACE_12, SPACE_6);
                    ui.horizontal_top(|ui| {
                        let diameter = CONTROL_HEIGHT + SPACE_8;
                        let (disc, _) = ui.allocate_exact_size(vec2(diameter, diameter), Sense::hover());
                        ui.painter().circle_filled(disc.center(), diameter / 2.0, t.warning.gamma_multiply(0.2));
                        icons::paint(ui.painter(), disc.shrink(SPACE_8 + 1.0), self.icon, t.dark, 1.0);
                        ui.vertical(|ui| {
                            let (heading, _) = ui.allocate_exact_size(vec2(ui.available_width(), FORM_CLOSE_SIZE), Sense::hover());
                            ui.painter().text(heading.left_center(), Align2::LEFT_CENTER, self.title, FontId::proportional(FONT_HEADING), t.form_ink);
                            let at: Pos2 = pos2(heading.right() - FORM_CLOSE_SIZE / 2.0, heading.center().y);
                            if close_button(ui, t, at) {
                                action = Action::Cancel;
                            }
                            ui.add(egui::Label::new(self.message).wrap());
                        });
                    });
                });
            });
            let clicked = footer(ui, t, Buttons::Confirm { ok: self.ok });
            if clicked != Action::None {
                action = clicked;
            }
            action
        });
        let mut action = response.inner;
        if action == Action::None && response.should_close() {
            action = Action::Cancel;
        }
        if action == Action::None && response.is_top_modal && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)) {
            action = Action::Ok;
        }
        action
    }
}
