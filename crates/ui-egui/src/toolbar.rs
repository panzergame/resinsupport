// Ported from SolidCraft (MIT OR Apache-2.0), crates/ui-egui/src/toolbar.rs (`app_bar`,
// `toolbar`, `ribbon_tabs`, the ribbon button and section painting) at 4bdffef0; see
// ATTRIBUTION.md. Reduced: one workspace, fixed groups, no search, customisation, collapsing,
// contextual tabs or document tabs.

//! The application bar (always dark: brand, Home, File, settings, version) and the ribbon under
//! it: the workspace card, a tab per edit mode (Object, Support, Slice), and the tab's groups of
//! buttons, each an icon over its label with the group's name underneath.

use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Ui, pos2, vec2};
use serde_json::json;

use crate::theme::Tokens;
use crate::theme::metrics::*;
use crate::ui_kit::Tip as _;
use crate::ui_kit::{self, MenuItem};
use crate::{EditMode, ResinApp, i18n, icons, layer_preview, transform};

pub const APP_BAR_HEIGHT: f32 = 34.0;
const TAB_ROW_HEIGHT: f32 = 30.0;
const GROUP_HEIGHT: f32 = 86.0;
pub const RIBBON_HEIGHT: f32 = TAB_ROW_HEIGHT + GROUP_HEIGHT + 1.0;
const WORKSPACE_WIDTH: f32 = 88.0;
const RIBBON_TAB_INSET: f32 = 4.0;
const PANEL_GAP: f32 = 1.0;
/// The narrowest group: one button's.
const COLLAPSED_WIDTH: f32 = 72.0;
const BUTTON_HEIGHT: f32 = 60.0;
const BUTTON_GAP: f32 = 3.0;
const BUTTON_MIN_WIDTH: f32 = 54.0;
const BUTTON_MAX_WIDTH: f32 = 76.0;
const BUTTON_LABEL_PADDING: f32 = 10.0;
/// From a button's top to its label's.
const BUTTON_LABEL_TOP: f32 = 32.0;
const SECTION_HEIGHT: f32 = 20.0;

/// What a ribbon button does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Cmd {
    Import,
    Move,
    Rotate,
    Scale,
    DeleteModels,
    Clear,
    PlaceTips,
    DeleteTips,
    Fit,
    Home,
    Perspective,
    SliceNow,
    CancelSlice,
    ExportCtb,
    Preview3d,
    PreviewLayer,
}

/// A button's state this frame.
struct State {
    /// A key, or a catalog id for a key with a name (`key.delete`).
    shortcut: Option<&'static str>,
    /// Catalog id of why it can't act now.
    disabled: Option<&'static str>,
    pressed: bool,
}

impl Cmd {
    /// Catalog id of its label; its tooltip is the same id plus `.tip`.
    fn label(self) -> &'static str {
        match self {
            Cmd::Import => "ribbon.import",
            Cmd::Move => "ribbon.move",
            Cmd::Rotate => "ribbon.rotate",
            Cmd::Scale => "ribbon.scale",
            Cmd::DeleteModels => "ribbon.delete_models",
            Cmd::DeleteTips => "ribbon.delete_tips",
            Cmd::Clear => "ribbon.clear",
            Cmd::PlaceTips => "ribbon.tips",
            Cmd::Fit => "ribbon.fit",
            Cmd::Home => "ribbon.home",
            Cmd::Perspective => "ribbon.perspective",
            Cmd::SliceNow => "ribbon.slice_now",
            Cmd::CancelSlice => "ribbon.cancel_slice",
            Cmd::ExportCtb => "ribbon.export_ctb",
            Cmd::Preview3d => "ribbon.preview_3d",
            Cmd::PreviewLayer => "ribbon.preview_layer",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Cmd::Import => "import",
            Cmd::Move => "move",
            Cmd::Rotate => "revolve",
            Cmd::Scale => "scale",
            Cmd::DeleteModels | Cmd::DeleteTips | Cmd::Clear => "delete",
            Cmd::PlaceTips => "cone",
            Cmd::Fit => "fit",
            Cmd::Home => "home",
            Cmd::Perspective => "perspective",
            Cmd::SliceNow => "play",
            Cmd::CancelSlice => "stop",
            Cmd::ExportCtb => "export",
            Cmd::Preview3d => "box",
            Cmd::PreviewLayer => "section",
        }
    }

    fn state(self, app: &ResinApp) -> State {
        let scene = &app.session.scene;
        let tool = |op| transform::tool(app).is_some_and(|t| t.op == op);
        let need_pick = scene.selected_models.is_empty().then_some("reason.select_model");
        let need_models = scene.models().is_empty();
        // Hidden models are not printed: with none shown, there is nothing to export.
        let need_shown =
            if need_models { Some("reason.import_first") } else { scene.models().iter().all(|m| m.hidden).then_some("reason.all_hidden") };
        let state = |shortcut, disabled, pressed| State { shortcut, disabled, pressed };
        match self {
            #[cfg(not(target_arch = "wasm32"))]
            Cmd::Import => state(Some("Ctrl+I"), None, false),
            #[cfg(target_arch = "wasm32")]
            Cmd::Import => state(None, Some("reason.drop_stl"), false),
            Cmd::Move => state(Some("T"), need_pick, tool(transform::Op::Move)),
            Cmd::Rotate => state(Some("R"), need_pick, tool(transform::Op::Rotate)),
            Cmd::Scale => state(Some("S"), need_pick, tool(transform::Op::Scale)),
            Cmd::DeleteModels => state(Some("key.delete"), need_pick, false),
            Cmd::Clear => state(None, need_models.then_some("reason.empty_scene"), false),
            Cmd::PlaceTips => state(None, need_models.then_some("reason.import_first"), true),
            Cmd::DeleteTips => {
                state(Some("key.delete"), (scene.selected.is_empty() && scene.selected_braces.is_empty()).then_some("reason.select_tips"), false)
            }
            Cmd::Fit => state(Some("F"), None, false),
            Cmd::Home => state(None, None, false),
            Cmd::Perspective => state(Some("P"), None, app.session.camera.fov > 0.0),
            Cmd::SliceNow => state(None, None, false),
            Cmd::CancelSlice => state(None, (!app.slicer.is_running()).then_some("reason.not_slicing"), false),
            #[cfg(not(target_arch = "wasm32"))]
            Cmd::ExportCtb => state(Some("Ctrl+E"), need_shown, false),
            #[cfg(target_arch = "wasm32")]
            Cmd::ExportCtb => state(None, Some("reason.export_desktop"), false),
            Cmd::Preview3d => state(None, None, !layer_preview::active(app) || !app.preview.flat),
            Cmd::PreviewLayer => {
                let active = layer_preview::active(app);
                state(None, (!active).then_some("reason.no_job"), active && app.preview.flat)
            }
        }
    }

    fn tip(self) -> String {
        format!("{}.tip", self.label())
    }

    fn run(self, app: &mut ResinApp) {
        match self {
            #[cfg(not(target_arch = "wasm32"))]
            Cmd::Import => app.import_dialog(),
            #[cfg(target_arch = "wasm32")]
            Cmd::Import => {}
            Cmd::Move => transform::open(app, transform::Op::Move),
            Cmd::Rotate => transform::open(app, transform::Op::Rotate),
            Cmd::Scale => transform::open(app, transform::Op::Scale),
            Cmd::DeleteModels | Cmd::DeleteTips => app.delete_selection(),
            Cmd::Clear => {
                app.run("scene.clear", json!({}));
            }
            Cmd::PlaceTips => {}
            Cmd::Fit => app.animate_command("view.fit", json!({ "aspect": app.aspect() })),
            Cmd::Home => app.animate_home(app.aspect()),
            Cmd::Perspective => app.toggle_projection(),
            Cmd::SliceNow => app.slice_now(),
            Cmd::CancelSlice => app.cancel_slice(),
            #[cfg(not(target_arch = "wasm32"))]
            Cmd::ExportCtb => app.export_dialog(),
            #[cfg(target_arch = "wasm32")]
            Cmd::ExportCtb => {}
            Cmd::Preview3d => app.preview.flat = false,
            Cmd::PreviewLayer => app.preview.flat = true,
        }
    }
}

/// A group: its name (a catalog id), its buttons, and whether its name opens the standard views.
struct Group {
    name: &'static str,
    cmds: &'static [Cmd],
    views: bool,
}

fn groups(mode: EditMode) -> &'static [Group] {
    const VIEW: Group = Group { name: "group.view", cmds: &[Cmd::Fit, Cmd::Home, Cmd::Perspective], views: true };
    match mode {
        EditMode::Object => &[
            Group { name: "group.import", cmds: &[Cmd::Import], views: false },
            Group { name: "group.arrange", cmds: &[Cmd::Move, Cmd::Rotate, Cmd::Scale], views: false },
            Group { name: "group.edit", cmds: &[Cmd::DeleteModels, Cmd::Clear], views: false },
            VIEW,
        ],
        EditMode::Support => &[Group { name: "group.supports", cmds: &[Cmd::PlaceTips, Cmd::DeleteTips], views: false }, VIEW],
        EditMode::Slice => &[
            Group { name: "group.slice", cmds: &[Cmd::SliceNow, Cmd::CancelSlice, Cmd::ExportCtb], views: false },
            Group { name: "group.preview", cmds: &[Cmd::Preview3d, Cmd::PreviewLayer], views: false },
            VIEW,
        ],
    }
}

const STANDARD_VIEWS: [(&str, &str); 7] = [
    ("stdview.front", "front"),
    ("stdview.back", "back"),
    ("stdview.left", "left"),
    ("stdview.right", "right"),
    ("stdview.top", "top"),
    ("stdview.bottom", "bottom"),
    ("stdview.iso", "iso"),
];

// ---------------------------------------------------------------------------------------------
// Application bar

pub fn app_bar(app: &mut ResinApp, ui: &mut Ui, t: &Tokens) {
    let frame = egui::Frame::NONE.fill(t.app_bar);
    egui::Panel::top("app_bar").exact_size(APP_BAR_HEIGHT).resizable(false).show_separator_line(false).frame(frame).show(ui, |ui| {
        let r = ui.max_rect();
        ui_kit::mark(ui.layer_id(), "app_bar", r, "");
        let left = r.left();
        // Brand mark and name.
        ui.painter().circle_filled(pos2(left + 20.0, r.center().y), 7.0, t.accent);
        ui.painter().text(pos2(left + 36.0, r.center().y), Align2::LEFT_CENTER, "ResinSupport", FontId::proportional(FONT_HEADING), t.app_bar_text);
        let mut x = left + 148.0;
        let home = Rect::from_center_size(pos2(x - 4.0, r.center().y), vec2(32.0, 32.0));
        let resp = ui.interact(home, ui.id().with("appbar_home"), Sense::click());
        ui_kit::mark(ui.layer_id(), "app_bar_button", home, "home");
        if resp.hovered() {
            ui.painter().rect_filled(home.shrink(3.0), RADIUS_SM, t.app_bar_hover_button);
        }
        icons::paint(ui.painter(), home.shrink(7.0), "home", true, 1.0);
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, i18n::tr("appbar.home")));
        if resp.tip(i18n::tr("appbar.home")).clicked() {
            app.animate_home(app.aspect());
        }
        x = home.right() + 8.0;
        separator(ui, r, x, t);
        x += 16.0;
        // File: a text button opening its menu.
        let galley = ui.painter().layout_no_wrap(i18n::tr("appbar.file").into(), FontId::proportional(FONT_BUTTON), t.app_bar_text);
        let fr = Rect::from_min_size(pos2(x - 13.0, r.center().y - 13.0), vec2(galley.size().x + 16.0, 26.0));
        let file = ui.interact(fr, ui.id().with("appbar_file"), Sense::click());
        file.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, i18n::tr("appbar.file")));
        ui_kit::mark(ui.layer_id(), "app_bar_menu", fr, i18n::tr("appbar.file"));
        let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&file));
        if file.hovered() || open {
            ui.painter().rect_filled(fr, RADIUS_SM, t.app_bar_hover);
        }
        ui.painter().galley(pos2(fr.left() + 8.0, fr.center().y - galley.size().y / 2.0), galley, t.app_bar_text);
        egui::Popup::menu(&file).show(|ui| file_menu(app, ui, t));
        // Undo and Redo, as in the File menu and their shortcuts.
        x = fr.right() + 24.0;
        for forward in [false, true] {
            let b = Rect::from_center_size(pos2(x, r.center().y), vec2(32.0, 32.0));
            let (icon, tip) = history_button(app, ui.ctx(), forward);
            let enabled = history_step(app, forward).is_some();
            let resp = ui.interact(b, ui.id().with(icon), if enabled { Sense::click() } else { Sense::hover() });
            ui_kit::mark(ui.layer_id(), "app_bar_button", b, icon);
            if enabled && resp.hovered() {
                ui.painter().rect_filled(b.shrink(3.0), RADIUS_SM, t.app_bar_hover_button);
            }
            icons::paint(ui.painter(), Rect::from_center_size(b.center(), vec2(18.0, 18.0)), icon, true, if enabled { 1.0 } else { 0.35 });
            resp.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, i18n::tr(if forward { "menu.redo" } else { "menu.undo" }))
            });
            if resp.tip(tip).clicked() && enabled {
                app.step_history(forward);
            }
            x += 34.0;
        }
        // Right: version, then settings.
        let version = format!("v{}", env!("CARGO_PKG_VERSION"));
        let vg = ui.painter().layout_no_wrap(version, FontId::proportional(FONT_SMALL), t.app_bar_text);
        let vx = r.right() - SPACE_12 - vg.size().x;
        ui.painter().galley(pos2(vx, r.center().y - vg.size().y / 2.0), vg, t.app_bar_text);
        let gear = Rect::from_center_size(pos2(vx - SPACE_12 - 16.0, r.center().y), vec2(32.0, 32.0));
        let resp = ui.interact(gear, ui.id().with("appbar_settings"), Sense::click());
        ui_kit::mark(ui.layer_id(), "app_bar_button", gear, "settings");
        if resp.hovered() || app.prefs_open {
            ui.painter().rect_filled(gear.shrink(3.0), RADIUS_SM, t.app_bar_hover_button);
        }
        icons::paint(ui.painter(), Rect::from_center_size(gear.center(), vec2(18.0, 18.0)), "settings", true, 1.0);
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, i18n::tr("appbar.preferences")));
        if resp.tip(i18n::tr("appbar.preferences")).clicked() {
            app.prefs_open = !app.prefs_open;
        }
    });
}

fn separator(ui: &Ui, bar: Rect, x: f32, t: &Tokens) {
    ui.painter().vline(x, bar.center().y - 9.0..=bar.center().y + 9.0, Stroke::new(1.0, t.app_bar_text.gamma_multiply(0.3)));
}

/// The command that made the step Undo (`forward` false) or Redo would take back or again.
fn history_step(app: &ResinApp, forward: bool) -> Option<&'static str> {
    if forward { app.session.redo_command() } else { app.session.undo_command() }
}

/// The app bar's Undo or Redo button: its icon and tooltip.
fn history_button(app: &ResinApp, ctx: &egui::Context, forward: bool) -> (&'static str, String) {
    let (icon, shortcut) = if forward { ("redo", &crate::REDO) } else { ("undo", &crate::UNDO) };
    let tip = match (history_step(app, forward), forward) {
        (Some(cmd), _) => {
            let step = i18n::tr_built(&format!("cmd.{cmd}"));
            let label = if forward { i18n::format("appbar.redo", &[("step", &step)]) } else { i18n::format("appbar.undo", &[("step", &step)]) };
            format!("{label} ({})", ctx.format_shortcut(shortcut))
        }
        (None, true) => i18n::tr("appbar.nothing_to_redo").to_owned(),
        (None, false) => i18n::tr("appbar.nothing_to_undo").to_owned(),
    };
    (icon, tip)
}

fn file_menu(app: &mut ResinApp, ui: &mut Ui, t: &Tokens) {
    ui_kit::menu_list(ui, |ui| {
        let shortcut = |ui: &Ui, s: &egui::KeyboardShortcut| ui.ctx().format_shortcut(s);
        let has_models = !app.session.scene.models().is_empty();
        #[cfg(not(target_arch = "wasm32"))]
        let has_shown = app.session.scene.models().iter().any(|m| !m.hidden);
        #[cfg(not(target_arch = "wasm32"))]
        {
            if MenuItem::new(i18n::tr("menu.open")).shortcut(&shortcut(ui, &crate::OPEN)).show(ui, t).clicked() {
                app.open_dialog();
            }
            if MenuItem::new(i18n::tr("menu.save")).shortcut(&shortcut(ui, &crate::SAVE)).show(ui, t).clicked() {
                app.save_project(false);
            }
            if MenuItem::new(i18n::tr("menu.save_as")).shortcut(&shortcut(ui, &crate::SAVE_AS)).show(ui, t).clicked() {
                app.save_project(true);
            }
            if MenuItem::new(i18n::tr("menu.export_ctb")).shortcut(&shortcut(ui, &crate::EXPORT_CTB)).enabled(has_shown).show(ui, t).clicked() {
                app.export_dialog();
            }
            ui_kit::menu_separator(ui, t);
            if MenuItem::new(i18n::tr("menu.import")).shortcut(&shortcut(ui, &crate::IMPORT)).show(ui, t).clicked() {
                app.import_dialog();
            }
        }
        if MenuItem::new(i18n::tr("menu.clear")).enabled(has_models).show(ui, t).clicked() {
            app.run("scene.clear", json!({}));
        }
        ui_kit::menu_separator(ui, t);
        let undo = history_step(app, false).is_some();
        if MenuItem::new(i18n::tr("menu.undo")).shortcut(&shortcut(ui, &crate::UNDO)).enabled(undo).show(ui, t).clicked() {
            app.step_history(false);
        }
        let redo = history_step(app, true).is_some();
        if MenuItem::new(i18n::tr("menu.redo")).shortcut(&shortcut(ui, &crate::REDO)).enabled(redo).show(ui, t).clicked() {
            app.step_history(true);
        }
        ui_kit::menu_separator(ui, t);
        if MenuItem::new(i18n::tr("menu.quit")).show(ui, t).clicked() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
    });
}

// ---------------------------------------------------------------------------------------------
// Ribbon

pub fn ribbon(app: &mut ResinApp, ui: &mut Ui, t: &Tokens) {
    let frame = egui::Frame::NONE.fill(t.toolbar);
    egui::Panel::top("ribbon").exact_size(RIBBON_HEIGHT).resizable(false).show_separator_line(false).frame(frame).show(ui, |ui| {
        let r = ui.max_rect();
        ui_kit::mark(ui.layer_id(), "ribbon", r, "");
        let painter = ui.painter().clone();
        // Workspace card.
        let ws = Rect::from_min_size(r.left_top(), vec2(WORKSPACE_WIDTH, RIBBON_HEIGHT - 1.0));
        ui_kit::mark(ui.layer_id(), "workspace", ws, i18n::tr("workspace.prepare"));
        painter.rect_filled(Rect::from_min_size(ws.left_top(), vec2(ws.width(), SPACE_3)), 0, t.accent);
        painter.vline(ws.right() - 0.5, ws.y_range(), Stroke::new(1.0, t.border));
        icons::paint(&painter, Rect::from_center_size(pos2(ws.center().x, ws.top() + 39.0), vec2(30.0, 30.0)), "ws_design", t.dark, 1.0);
        painter.text(
            pos2(ws.center().x, ws.top() + 69.0),
            Align2::CENTER_CENTER,
            i18n::tr("workspace.prepare"),
            FontId::proportional(FONT_BUTTON),
            t.text,
        );
        painter.text(
            pos2(ws.center().x, ws.top() + 89.0),
            Align2::CENTER_CENTER,
            i18n::tr("workspace.caption"),
            FontId::proportional(FONT_SMALL),
            t.text_dim,
        );
        ui.interact(ws, ui.id().with("workspace"), Sense::hover()).tip(i18n::tr("workspace.tip"));

        tabs(app, ui, Rect::from_min_max(pos2(ws.right(), r.top()), pos2(r.right(), r.top() + TAB_ROW_HEIGHT)), t);

        let top = r.top() + TAB_ROW_HEIGHT;
        let mut px = ws.right();
        let mut clicked = None;
        let mut view = None;
        for g in groups(app.mode) {
            let widths: Vec<f32> = g.cmds.iter().map(|c| button_width(&painter, i18n::tr(c.label()))).collect();
            let content = widths.iter().sum::<f32>() + widths.len().saturating_sub(1) as f32 * BUTTON_GAP;
            let width = (content + 8.0).max(COLLAPSED_WIDTH);
            let mut bx = px + (width - content) / 2.0;
            ui_kit::mark(ui.layer_id(), "ribbon_group", Rect::from_min_size(pos2(px, top), vec2(width, GROUP_HEIGHT)), i18n::tr(g.name));
            for (cmd, w) in g.cmds.iter().zip(widths) {
                let whole = Rect::from_min_size(pos2(bx, top + 2.0), vec2(w, BUTTON_HEIGHT));
                bx += w + BUTTON_GAP;
                if ribbon_button(ui, &painter, whole, *cmd, app, t) {
                    clicked = Some(*cmd);
                }
            }
            // The group's name; a name with a menu (View: the standard views) opens it.
            let lr = Rect::from_min_size(
                pos2(px + RIBBON_TAB_INSET, top + GROUP_HEIGHT - SECTION_HEIGHT - (RIBBON_TAB_INSET - 1.0)),
                vec2((width - 2.0 * RIBBON_TAB_INSET).max(0.0), SECTION_HEIGHT),
            );
            if g.views {
                let lresp = ui.interact(lr, ui.id().with(("group", g.name)), Sense::click());
                lresp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, i18n::tr(g.name)));
                ui_kit::mark(ui.layer_id(), "ribbon_group_menu", lr, i18n::tr(g.name));
                let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&lresp));
                if lresp.hovered() || open {
                    painter.rect_filled(lr, RADIUS_SM, if open { t.accent_soft } else { t.hover });
                }
                let ink = if open { t.selected_ink } else { t.text_dim };
                let name = i18n::tr(g.name);
                let lw = painter.layout_no_wrap(name.into(), FontId::proportional(FONT_CAPTION), ink).size().x;
                painter.text(lr.center() - vec2(6.0, 0.0), Align2::CENTER_CENTER, name, FontId::proportional(FONT_CAPTION), ink);
                dropdown_arrow(&painter, pos2(lr.center().x + lw / 2.0 + 1.0, lr.center().y + 0.5), ink);
                egui::Popup::menu(&lresp).show(|ui| {
                    ui_kit::menu_list(ui, |ui| {
                        for (label, id) in STANDARD_VIEWS {
                            if MenuItem::new(i18n::tr(label)).show(ui, t).clicked() {
                                view = Some(id);
                            }
                        }
                    });
                });
            } else {
                painter.text(lr.center(), Align2::CENTER_CENTER, i18n::tr(g.name), FontId::proportional(FONT_CAPTION), t.text_dim);
            }
            px += width;
            painter.vline(px, top + 4.0..=top + GROUP_HEIGHT - 3.0, Stroke::new(1.0, t.border));
            px += PANEL_GAP;
        }
        painter.hline(r.x_range(), r.bottom() - 0.5, Stroke::new(1.0, t.border));
        if let Some(cmd) = clicked {
            cmd.run(app);
        }
        if let Some(id) = view {
            app.animate_command("view.standard", json!({ "view": id }));
        }
    });
}

/// The tab row: one tab per edit mode, the selected one in the soft accent.
fn tabs(app: &mut ResinApp, ui: &mut Ui, row: Rect, t: &Tokens) {
    let font = FontId::proportional(FONT_BUTTON);
    let mut x = row.left() + RIBBON_TAB_INSET;
    for mode in EditMode::ALL {
        let label = i18n::tr(mode.tab());
        let width = ui.painter().layout_no_wrap(label.into(), font.clone(), t.text).size().x + 24.0;
        let tr = Rect::from_min_size(pos2(x, row.top() + RIBBON_TAB_INSET), vec2(width, TAB_ROW_HEIGHT - 2.0 * RIBBON_TAB_INSET));
        let resp = ui.interact(tr, ui.id().with(("tab", label)), Sense::click());
        ui_kit::mark(ui.layer_id(), "ribbon_tab", tr, label);
        let active = app.mode == mode;
        resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, active, label));
        let (fill, ink) = match (active, resp.hovered()) {
            (true, _) => (t.accent_soft, t.selected_ink),
            (false, true) => (t.hover, t.text),
            (false, false) => (Color32::TRANSPARENT, t.text),
        };
        ui.painter().rect_filled(tr, CONTROL_RADIUS.saturating_sub(1), fill);
        ui.painter().text(tr.center(), Align2::CENTER_CENTER, label, font.clone(), ink);
        if resp.tip(i18n::format("tab.tip", &[("mode", &i18n::tr(mode.label()))])).clicked() {
            app.set_mode(mode);
        }
        x += width + 2.0;
    }
    ui.painter().hline(row.x_range(), row.bottom() - 0.5, Stroke::new(1.0, t.toolbar.lerp_to_gamma(t.border, 0.5)));
}

/// One large button: pressed (soft accent, outlined) while its tool is in use, the hover fill
/// under the pointer, dimmed when it can't act. Whether it was clicked and can act.
fn ribbon_button(ui: &Ui, painter: &egui::Painter, whole: Rect, cmd: Cmd, app: &ResinApp, t: &Tokens) -> bool {
    let info = cmd.state(app);
    let enabled = info.disabled.is_none();
    let resp = ui.interact(whole, ui.id().with(("cmd", cmd as u8)), Sense::click());
    ui_kit::mark(ui.layer_id(), "ribbon_button", whole, i18n::tr(cmd.label()));
    if info.pressed || (enabled && resp.is_pointer_button_down_on()) {
        painter.rect(whole, RADIUS_SM, t.accent_soft, Stroke::new(1.0, t.accent), egui::StrokeKind::Inside);
    } else if enabled && resp.hovered() {
        painter.rect_filled(whole, RADIUS_SM, t.hover);
    }
    let icon = Rect::from_center_size(pos2(whole.center().x, whole.top() + 16.0), vec2(26.0, 26.0));
    icons::paint(painter, icon, cmd.icon(), t.dark, if enabled { 1.0 } else { 0.35 });
    let ink = if enabled { t.text } else { t.text_dim };
    let label = i18n::tr(cmd.label());
    let lines = label_lines(painter, label, whole.width() - BUTTON_LABEL_PADDING);
    let line_height = FONT_BODY + 2.0;
    let height = line_height * lines.len() as f32;
    let label_top = (whole.top() + BUTTON_LABEL_TOP).min(whole.bottom() - 2.0 - height);
    for (i, line) in lines.iter().enumerate() {
        let y = label_top + line_height * (i as f32 + 0.5);
        painter.text(pos2(whole.center().x, y), Align2::CENTER_CENTER, line, FontId::proportional(FONT_BODY), ink);
    }
    let title = match info.shortcut {
        // A named key (`key.delete`) is a message; a combination (`Ctrl+I`) is the same everywhere.
        Some(key) => i18n::format(
            "ribbon.with_key",
            &[("label", &label), ("key", &if key.starts_with("key.") { i18n::tr_built(key) } else { key.to_string() })],
        ),
        None => label.to_string(),
    };
    let mut tip = format!("{title}\n{}", i18n::tr_built(&cmd.tip()));
    if let Some(why) = info.disabled {
        tip = format!("{tip}\n{}", i18n::tr(why));
    }
    if enabled && resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
    resp.tip(tip).clicked() && enabled
}

fn text_width(painter: &egui::Painter, text: &str) -> f32 {
    painter.layout_no_wrap(text.into(), FontId::proportional(FONT_BODY), Color32::WHITE).size().x
}

/// The label in at most two lines, broken at spaces; a line too wide ends in an ellipsis.
fn label_lines(painter: &egui::Painter, label: &str, width: f32) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for word in label.split_whitespace() {
        let full = lines.len() == 2;
        match lines.last_mut() {
            Some(line) if full || text_width(painter, &format!("{line} {word}")) <= width => {
                line.push(' ');
                line.push_str(word);
            }
            _ => lines.push(word.to_string()),
        }
    }
    for line in &mut lines {
        if text_width(painter, line) > width {
            while !line.is_empty() && text_width(painter, &format!("{line}…")) > width {
                line.pop();
            }
            line.push('…');
        }
    }
    lines
}

fn button_width(painter: &egui::Painter, label: &str) -> f32 {
    let widest = label_lines(painter, label, BUTTON_MAX_WIDTH - BUTTON_LABEL_PADDING).iter().map(|l| text_width(painter, l)).fold(0.0, f32::max);
    (widest + BUTTON_LABEL_PADDING).clamp(BUTTON_MIN_WIDTH, BUTTON_MAX_WIDTH)
}

/// The chevron every ribbon menu shares.
fn dropdown_arrow(painter: &egui::Painter, at: Pos2, ink: Color32) {
    let s = 2.5;
    painter.line(vec![at + vec2(-s, -s / 2.0), at + vec2(0.0, s / 2.0), at + vec2(s, -s / 2.0)], Stroke::new(1.2, ink));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_fit_their_buttons() {
        let ctx = egui::Context::default();
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            let painter = ui.painter();
            for mode in EditMode::ALL {
                for g in groups(mode) {
                    for label in ["Import STL", "Clear Scene", "Perspective", "Move", "An unusually long ribbon label"] {
                        let w = button_width(painter, label);
                        assert!((BUTTON_MIN_WIDTH..=BUTTON_MAX_WIDTH).contains(&w), "{label}: {w}");
                        let lines = label_lines(painter, label, w - BUTTON_LABEL_PADDING);
                        assert!(!lines.is_empty() && lines.len() <= 2, "{label}: {lines:?}");
                        assert!(lines.iter().all(|l| text_width(painter, l) <= w - BUTTON_LABEL_PADDING + 0.5), "{label}: {lines:?}");
                    }
                    assert!(!g.cmds.is_empty());
                }
            }
        });
        // Nothing renders the frame: its font atlas upload is dropped on purpose.
        out.textures_delta.clear();
    }

    #[test]
    fn every_ribbon_word_is_in_the_catalog() {
        for mode in EditMode::ALL {
            assert!(i18n::has(mode.label()) && i18n::has(mode.tab()));
            for g in groups(mode) {
                assert!(i18n::has(g.name), "{}", g.name);
                for c in g.cmds {
                    assert!(i18n::has(c.label()) && i18n::has(&c.tip()), "{c:?}");
                }
            }
        }
        for (id, _) in STANDARD_VIEWS {
            assert!(i18n::has(id), "{id}");
        }
    }

    #[test]
    fn every_button_has_an_icon() {
        for mode in EditMode::ALL {
            for g in groups(mode) {
                for c in g.cmds {
                    assert!(icons::exists(c.icon()), "{c:?}: {}", c.icon());
                }
            }
        }
        assert!(icons::exists("ws_design") && icons::exists("home") && icons::exists("settings") && icons::exists("close"));
    }
}
