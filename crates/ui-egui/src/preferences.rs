//! The Preferences window: a page list on the left, the page's settings on the right, Restore
//! defaults and Close in the footer. Settings apply as they change; the rows come from the
//! `preference_settings!` table in [`crate::prefs`].

use crate::prefs::{self, PAGES};
use crate::theme::Tokens;
use crate::theme::metrics::{FORM_WIDE_CARD_WIDTH, SPACE_12};
use crate::ui_kit::{self, Action, Buttons, Card};
use crate::{ResinApp, i18n};

/// The page list's width.
const PAGE_LIST_WIDTH: f32 = 130.0;
/// The body's least height: the longest page's.
const PAGE_MIN_HEIGHT: f32 = 300.0;

pub fn window(app: &mut ResinApp, ctx: &egui::Context) {
    if !app.prefs_open {
        return;
    }
    let t = Tokens::for_dark(ctx.global_style().visuals.dark_mode);
    let at = app.viewport_rect.map_or(egui::pos2(240.0, 160.0), |r| r.left_top() + egui::vec2(SPACE_12, SPACE_12));
    let card = Card {
        id: egui::Id::new("preferences"),
        title: i18n::tr("prefs.title"),
        icon: "settings",
        at,
        bounds: app.viewport_rect,
        width: FORM_WIDE_CARD_WIDTH,
        buttons: Buttons::ResetClose,
    };
    let mut page = app.prefs_page.min(PAGES.len().saturating_sub(1));
    let prefs = &mut app.prefs;
    let action = card.show(ctx, &t, |ui| {
        let row = ui.horizontal_top(|ui| {
            // Pages differ in length: a steady height keeps the footer from jumping between them.
            ui.set_min_height(PAGE_MIN_HEIGHT);
            ui.vertical(|ui| {
                ui.set_width(PAGE_LIST_WIDTH);
                for (i, (title, icon)) in PAGES.iter().enumerate() {
                    if ui_kit::page_tab(ui, &t, icon, i18n::tr(title), i == page) {
                        page = i;
                    }
                }
            });
            // A rule between list and page, drawn below at the content's height: egui's vertical
            // separator takes all the height it is offered and stretches the window to its bounds.
            let (gap, _) = ui.allocate_exact_size(egui::vec2(SPACE_12 * 2.0, 0.0), egui::Sense::hover());
            ui.vertical(|ui| prefs::page_rows(prefs, page, ui, &t));
            gap.center().x
        });
        let x = row.inner;
        ui.painter().vline(x, row.response.rect.y_range(), egui::Stroke::new(1.0, t.dialog_rule));
    });
    app.prefs_page = page;
    match action {
        Action::Ok | Action::Cancel => app.prefs_open = false,
        Action::Reset => prefs::reset_page(&mut app.prefs, page),
        Action::None => {}
    }
}
