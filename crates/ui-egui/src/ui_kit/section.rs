// Ported from SolidCraft (MIT OR Apache-2.0), crates/ui-egui/src/ui_kit/layout.rs (`section`,
// `section_with_badge`) and ui_kit/disclosure.rs (`disclosure`) at 4bdffef0; see ATTRIBUTION.md.
// Reduced: no count or method badge, compact variant, preferences search or scenario handles; the
// chevron is drawn, not an icon.

use egui::{Align2, Color32, FontId, Painter, Rect, Sense, Stroke, Ui, pos2, vec2};

use crate::theme::Tokens;
use crate::theme::metrics::*;

/// SolidCraft's `form_section_icon_size`.
const CHEVRON_SIZE: f32 = 11.0;
/// SolidCraft's `font_section_tracking`, a share of the font's size.
const TRACKING: f32 = 0.04;

/// A fold of a card's rows: a `CONTROL_HEIGHT` heading (a chevron, the title in capitals, a rule
/// to the right edge), and under it `add`'s rows while open. A click on the heading opens or
/// closes it; that is remembered by `id` in the card, across runs. `add`'s result while open.
pub fn section<R>(ui: &mut Ui, t: &Tokens, id: impl egui::AsIdSalt, title: &str, default_open: bool, add: impl FnOnce(&mut Ui) -> R) -> Option<R> {
    ui.add_space(SPACE_2);
    let id = ui.make_persistent_id(id);
    let mut open = ui.ctx().data_mut(|d| d.get_persisted::<bool>(id)).unwrap_or(default_open);
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), CONTROL_HEIGHT), Sense::click());
    super::mark(ui.layer_id(), "section", rect, title);
    if response.hovered() || response.has_focus() {
        ui.painter().rect_filled(rect.shrink2(vec2(0.0, 1.0)), RADIUS_SM, t.hover);
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let chevron = Rect::from_center_size(pos2(rect.left() + CHEVRON_SIZE / 2.0, rect.center().y), vec2(CHEVRON_SIZE, CHEVRON_SIZE));
    paint_chevron(ui.painter(), chevron, open, t.form_dim);
    let caption = title.to_uppercase();
    let font = FontId::proportional(FONT_CAPTION);
    let mut format = egui::TextFormat::simple(font, t.form_dim);
    format.extra_letter_spacing = FONT_CAPTION * TRACKING;
    let galley = ui.fonts_mut(|f| f.layout_job(egui::text::LayoutJob::single_section(caption, format)));
    let left = chevron.right() + FORM_GAP_Y;
    let at = Align2::LEFT_CENTER.anchor_size(pos2(left, rect.center().y), galley.size()).min;
    let rule_left = left + galley.size().x + SPACE_12;
    ui.painter().with_clip_rect(ui.clip_rect().intersect(rect)).galley(at, galley, t.form_dim);
    if rule_left < rect.right() {
        ui.painter().hline(rule_left..=rect.right(), rect.center().y, Stroke::new(1.0, t.dialog_rule));
    }
    response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::CollapsingHeader, ui.is_enabled(), open, title));
    if response.clicked() {
        open = !open;
    }
    ui.ctx().data_mut(|d| d.insert_persisted(id, open));
    open.then(|| ui.push_id(id, add).inner)
}

/// Pointing down when open, right when closed: the section's, and the drop-downs' arrow.
pub(super) fn paint_chevron(painter: &Painter, r: Rect, open: bool, ink: Color32) {
    let (c, h) = (r.center(), r.width() * 0.25);
    let points = if open {
        [pos2(c.x - h * 1.6, c.y - h * 0.8), c + vec2(0.0, h * 0.8), pos2(c.x + h * 1.6, c.y - h * 0.8)]
    } else {
        [pos2(c.x - h * 0.8, c.y - h * 1.6), c + vec2(h * 0.8, 0.0), pos2(c.x - h * 0.8, c.y + h * 1.6)]
    };
    painter.add(egui::Shape::line(points.to_vec(), Stroke::new(1.5, ink)));
}

#[cfg(test)]
mod tests {
    use egui::{Pos2, Rect, vec2};

    use super::*;

    /// Whether the section's rows were laid out this frame, after `events`.
    fn frame(ctx: &egui::Context, events: Vec<egui::Event>) -> bool {
        let time = Some(ctx.input(|i| i.time) + 0.25);
        let raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(400.0, 300.0))), time, events, ..Default::default() };
        let mut shown = false;
        let t = Tokens::for_dark(true);
        let mut out = ctx.run_ui(raw, |ui| shown = section(ui, &t, "options", "Options", true, |_| ()).is_some());
        out.textures_delta.clear();
        shown
    }

    fn click(at: Pos2) -> Vec<egui::Event> {
        let button =
            |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
        vec![egui::Event::PointerMoved(at), button(true), button(false)]
    }

    #[test]
    fn a_click_on_the_heading_folds_and_unfolds_it_and_it_stays_so() {
        let ctx = egui::Context::default();
        assert!(frame(&ctx, vec![]), "open by default");
        let heading = pos2(40.0, SPACE_2 + CONTROL_HEIGHT / 2.0);
        assert!(!frame(&ctx, click(heading)), "folded");
        assert!(!frame(&ctx, vec![]), "stays folded");
        frame(&ctx, vec![]);
        assert!(frame(&ctx, click(heading)), "unfolded");
    }
}
