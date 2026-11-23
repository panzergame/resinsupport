//! The Models card, at the viewport's top left in every edition: a row per model, as SolidCraft's
//! browser lists bodies, one level deep. A click selects (Shift toggles; in Slice edition one model
//! at a time), the eye hides or shows the model, a double click or F2 renames it in its row, and a
//! right click opens its menu: Duplicate, Rename, Hide or Show, Delete. H hides the
//! selected models and Alt+H shows every hidden one. The chevron folds the card to its header.

use egui::{Pos2, pos2, vec2};
use resin_engine::MAX_NAME_CHARS;
use serde_json::json;

use crate::theme::Tokens;
use crate::theme::metrics::{FORM_BROWSER_WIDTH, SPACE_12};
use crate::ui_kit::{self, Action, BrowserRow, Buttons, Card, MenuItem, RenameEnd};
use crate::{EditMode, ResinApp, i18n, object_label};

pub(crate) const CARD_ID: &str = "models";

/// Ctrl+D: duplicate the selected models.
pub(crate) const DUPLICATE: egui::KeyboardShortcut = egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::D);
/// F2: rename the selected model.
pub(crate) const RENAME: egui::KeyboardShortcut = egui::KeyboardShortcut::new(egui::Modifiers::NONE, egui::Key::F2);
/// H: hide the selected models.
pub(crate) const HIDE: egui::KeyboardShortcut = egui::KeyboardShortcut::new(egui::Modifiers::NONE, egui::Key::H);
/// Alt+H: show every hidden model.
pub(crate) const SHOW_ALL: egui::KeyboardShortcut = egui::KeyboardShortcut::new(egui::Modifiers::ALT, egui::Key::H);

/// What the card keeps between frames.
#[derive(Default)]
pub struct ModelsCard {
    /// Folded to its header.
    pub(crate) folded: bool,
    /// The model whose name is being typed in its row.
    pub(crate) renaming: Option<Renaming>,
}

pub(crate) struct Renaming {
    pub(crate) id: u64,
    pub(crate) text: String,
    /// Its first frame: the field takes the focus with the whole name selected.
    pub(crate) fresh: bool,
}

/// What a row asked for, run once the rows are drawn.
enum Act {
    Select(u64, bool),
    Visible(Vec<u64>, bool),
    StartRename(u64),
    Rename(u64, String),
    CancelRename,
    /// A right click on a model: it is selected, unless it is selected already or hidden.
    MenuOn(u64),
    Duplicate(Vec<u64>),
    Delete(Vec<u64>),
}

/// Where the card's top left goes in the viewport: its first corner.
fn origin(app: &ResinApp) -> Pos2 {
    app.viewport_rect.map_or(pos2(SPACE_12, 160.0), |r| r.left_top() + vec2(SPACE_12, SPACE_12))
}

/// Where the cards beside it open: past its right edge.
pub(crate) fn beside(app: &ResinApp) -> Pos2 {
    origin(app) + vec2(FORM_BROWSER_WIDTH + SPACE_12, 0.0)
}

pub fn card(app: &mut ResinApp, ctx: &egui::Context) {
    let t = Tokens::for_dark(ctx.global_style().visuals.dark_mode);
    // A model being renamed that went (deleted, undone) takes its rename with it.
    if app.models_card.renaming.as_ref().is_some_and(|r| app.session.scene.model(r.id).is_none()) {
        app.models_card.renaming = None;
    }
    let folded = app.models_card.folded;
    let card = Card {
        id: egui::Id::new(CARD_ID),
        title: i18n::tr("models.heading"),
        icon: "",
        at: origin(app),
        bounds: app.viewport_rect,
        width: FORM_BROWSER_WIDTH,
        buttons: Buttons::Fold { folded },
    };
    let mut acts = Vec::new();
    if card.show(ctx, &t, |ui| rows(app, ui, &t, &mut acts)) == Action::Fold {
        app.models_card.folded = !folded;
    }
    for act in acts {
        apply(app, act);
    }
}

fn rows(app: &mut ResinApp, ui: &mut egui::Ui, t: &Tokens, acts: &mut Vec<Act>) {
    let scene = &app.session.scene;
    if scene.models().is_empty() {
        ui_kit::hint(ui, t, i18n::tr("models.empty"));
        return;
    }
    ui.spacing_mut().item_spacing.y = 0.0;
    let shift = ui.input(|i| i.modifiers.shift);
    let shortcut = |s: &egui::KeyboardShortcut| ui.ctx().format_shortcut(s);
    let (duplicate_keys, rename_keys) = (shortcut(&DUPLICATE), shortcut(&RENAME));
    let hide_keys = shortcut(&HIDE);
    for m in scene.models() {
        let title = object_label::titled(m);
        let selected = scene.selected_models.contains(&m.id);
        let row = BrowserRow { icon: "box", label: &title, selected, visible: !m.hidden };
        if let Some(r) = app.models_card.renaming.as_mut().filter(|r| r.id == m.id) {
            match ui_kit::rename_row(ui, t, m.id, &row, &mut r.text, &mut r.fresh, MAX_NAME_CHARS) {
                RenameEnd::Editing => {}
                RenameEnd::Commit => acts.push(Act::Rename(m.id, r.text.clone())),
                RenameEnd::Cancel => acts.push(Act::CancelRename),
            }
            continue;
        }
        let out = ui_kit::browser_row(ui, t, m.id, &row);
        if out.eye {
            acts.push(Act::Visible(vec![m.id], m.hidden));
        } else if out.double_clicked {
            acts.push(Act::StartRename(m.id));
        } else if out.clicked {
            acts.push(Act::Select(m.id, shift));
        }
        if out.row.secondary_clicked() {
            acts.push(Act::MenuOn(m.id));
        }
        // The menu acts on the selection when the model is in it, else on the model alone.
        let targets: Vec<u64> = if selected { scene.selected_models.iter().copied().collect() } else { vec![m.id] };
        egui::Popup::context_menu(&out.row).show(|ui| {
            ui_kit::menu_list(ui, |ui| {
                let duplicate = MenuItem::new(i18n::tr("models.duplicate")).icon("copy").shortcut(&duplicate_keys);
                if duplicate.show(ui, t).clicked() {
                    acts.push(Act::Duplicate(targets.clone()));
                }
                if MenuItem::new(i18n::tr("models.rename")).icon("rename").shortcut(&rename_keys).show(ui, t).clicked() {
                    acts.push(Act::StartRename(m.id));
                }
                // A hidden model is never selected: its menu acts on it alone.
                let visibility = if m.hidden {
                    MenuItem::new(i18n::tr("models.show")).icon("eye")
                } else {
                    MenuItem::new(i18n::tr("models.hide")).icon("suppress").shortcut(&hide_keys)
                };
                if visibility.show(ui, t).clicked() {
                    acts.push(Act::Visible(targets.clone(), m.hidden));
                }
                ui_kit::menu_separator(ui, t);
                let delete = MenuItem::new(i18n::tr("models.delete")).icon("delete").shortcut(i18n::tr("key.delete")).danger();
                if delete.show(ui, t).clicked() {
                    acts.push(Act::Delete(targets.clone()));
                }
            });
        });
        let size = m.bounds.size();
        out.row.on_hover_text(i18n::format(
            "models.tip",
            &[
                ("name", &title),
                ("triangles", &m.mesh.triangle_count()),
                ("x", &format!("{:.2}", size.x)),
                ("y", &format!("{:.2}", size.y)),
                ("z", &format!("{:.2}", size.z)),
                ("volume", &format!("{:.1}", m.volume())),
            ],
        ));
    }
}

fn apply(app: &mut ResinApp, act: Act) {
    match act {
        Act::Select(id, shift) => {
            // Slice edition picks one model at a time, for its own slicing settings.
            if app.mode == EditMode::Slice {
                app.run("model.select", json!({ "ids": [id] }));
            } else {
                app.set_mode(EditMode::Object);
                app.run("model.select", json!({ "ids": [id], "mode": if shift { "toggle" } else { "replace" } }));
            }
        }
        Act::Visible(ids, visible) => {
            app.run("model.visibility", json!({ "ids": ids, "visible": visible }));
        }
        Act::StartRename(id) => start_rename(app, id),
        Act::Rename(id, text) => {
            app.models_card.renaming = None;
            if app.session.scene.model(id).is_some_and(|m| m.name != text.trim()) {
                app.run("model.rename", json!({ "id": id, "name": text }));
            }
        }
        Act::CancelRename => app.models_card.renaming = None,
        Act::MenuOn(id) => {
            if !app.session.scene.selected_models.contains(&id) && app.session.scene.shows(id) {
                pick(app, id);
            }
        }
        Act::Duplicate(ids) => duplicate(app, ids),
        Act::Delete(ids) => delete(app, ids),
    }
}

/// Select `id` alone, as a plain click on its row does.
fn pick(app: &mut ResinApp, id: u64) {
    if app.mode != EditMode::Slice {
        app.set_mode(EditMode::Object);
    }
    app.run("model.select", json!({ "ids": [id] }));
}

fn start_rename(app: &mut ResinApp, id: u64) {
    let Some(name) = app.session.scene.model(id).map(|m| m.name.clone()) else { return };
    app.models_card.folded = false;
    app.models_card.renaming = Some(Renaming { id, text: name, fresh: true });
}

/// F2: rename the one selected model.
pub(crate) fn rename_selected(app: &mut ResinApp) {
    let selected: Vec<u64> = app.session.scene.selected_models.iter().copied().collect();
    if let [id] = selected[..] {
        start_rename(app, id);
    }
}

/// H: hide the selected models and their supports, one step. With none selected, says so.
pub(crate) fn hide_selected(app: &mut ResinApp) {
    if let Some(v) = app.run("model.visibility", json!({ "visible": false })) {
        let n = v["ids"].as_array().map_or(0, Vec::len);
        app.status = Some((i18n::count("status.hid_models", n, &[]), false));
    }
}

/// Alt+H: show every hidden model, one step.
pub(crate) fn show_all(app: &mut ResinApp) {
    let ids: Vec<u64> = app.session.scene.models().iter().filter(|m| m.hidden).map(|m| m.id).collect();
    if ids.is_empty() {
        return;
    }
    if app.run("model.visibility", json!({ "ids": ids, "visible": true })).is_some() {
        app.status = Some((i18n::count("status.showed_models", ids.len(), &[]), false));
    }
}

/// Ctrl+D: duplicate the selected models.
pub(crate) fn duplicate_selected(app: &mut ResinApp) {
    let ids: Vec<u64> = app.session.scene.selected_models.iter().copied().collect();
    if !ids.is_empty() {
        duplicate(app, ids);
    }
}

fn duplicate(app: &mut ResinApp, ids: Vec<u64>) {
    // The open tool remembers the selection's placements; the copies replace the selection.
    app.tool = None;
    if let Some(v) = app.run("model.duplicate", json!({ "ids": ids })) {
        let n = v["ids"].as_array().map_or(0, Vec::len);
        let beside = v["beside"].as_array().map_or(0, Vec::len);
        let id = if beside > 0 { "status.duplicated_models_beside" } else { "status.duplicated_models" };
        app.status = Some((i18n::count(id, n, &[("beside", &beside)]), false));
    }
}

/// Delete `ids` and their supports, as one step.
pub(crate) fn delete(app: &mut ResinApp, ids: Vec<u64>) {
    app.session.begin_step();
    let n = ids.into_iter().filter(|id| app.run("model.delete", json!({ "id": id })).is_some()).count();
    app.session.end_step();
    if n > 0 {
        app.status = Some((i18n::count("status.deleted_models", n, &[]), false));
    }
}

#[cfg(test)]
mod tests {
    use egui::{Event, Key, Modifiers, PointerButton, Pos2, Rect, pos2, vec2};

    use super::*;
    use crate::prefs::Prefs;
    use crate::theme::metrics::{FORM_HEADER_HEIGHT, SPACE_4, TREE_ROW_HEIGHT};

    struct Rig {
        ctx: egui::Context,
        app: ResinApp,
        ids: Vec<u64>,
    }

    impl Rig {
        /// The whole window, with `n` 10 mm cubes named a, b, c…, none selected.
        fn new(n: usize) -> Rig {
            let dir = std::env::temp_dir().join(format!("resin-ui-models-{}-{:?}", std::process::id(), std::thread::current().id()));
            std::fs::create_dir_all(&dir).unwrap();
            let path = dir.join("cube.stl");
            let v = |i: usize| format!("{} {} {}", (i & 1) * 10, (i >> 1 & 1) * 10, (i >> 2 & 1) * 10);
            let mut text = String::from("solid c\n");
            for [a, b, c, d] in [[0, 2, 3, 1], [4, 5, 7, 6], [0, 1, 5, 4], [2, 6, 7, 3], [0, 4, 6, 2], [1, 3, 7, 5]] {
                for [p, q, r] in [[a, b, c], [a, c, d]] {
                    text += &format!("facet normal 0 0 0\nouter loop\nvertex {}\nvertex {}\nvertex {}\nendloop\nendfacet\n", v(p), v(q), v(r));
                }
            }
            text += "endsolid c\n";
            std::fs::write(&path, text).unwrap();
            let mut app = ResinApp::build(Prefs { language: "en".into(), ..Prefs::default() }, &[]);
            for _ in 0..n {
                app.import(&path);
            }
            std::fs::remove_dir_all(dir).ok();
            let ids: Vec<u64> = app.session.scene.models().iter().map(|m| m.id).collect();
            for (id, name) in ids.iter().zip(["a", "b", "c", "d"]) {
                app.run("model.rename", json!({ "id": id, "name": name })).unwrap();
            }
            app.run("model.select", json!({ "ids": [] })).unwrap();
            let mut rig = Rig { ctx: egui::Context::default(), app, ids };
            for _ in 0..4 {
                rig.frame(Vec::new());
            }
            rig
        }

        fn frame(&mut self, events: Vec<Event>) {
            self.frame_held(events, Modifiers::NONE);
        }

        /// A frame with `modifiers` held.
        fn frame_held(&mut self, events: Vec<Event>, modifiers: Modifiers) {
            // A quarter of a second a frame: clicks on following frames make a double click.
            let time = Some(self.ctx.input(|i| i.time) + 0.25);
            let screen_rect = Some(Rect::from_min_size(Pos2::ZERO, vec2(1280.0, 800.0)));
            let events = std::iter::once(Event::ModifiersChanged(modifiers)).chain(events).collect();
            let raw = egui::RawInput { screen_rect, time, events, ..Default::default() };
            let app = &mut self.app;
            let mut out = self.ctx.run_ui(raw, |ui| {
                app.frame(ui);
            });
            out.textures_delta.clear();
        }

        /// The middle of row `i`, or of its eye.
        fn row(&self, i: usize, eye: bool) -> Pos2 {
            let card = self.card();
            let y = card.top() + FORM_HEADER_HEIGHT + SPACE_4 + (i as f32 + 0.5) * TREE_ROW_HEIGHT;
            pos2(if eye { card.right() - 14.0 } else { card.center().x - 20.0 }, y)
        }

        fn card(&self) -> Rect {
            self.ctx.memory(|m| m.area_rect(egui::Id::new(CARD_ID))).unwrap()
        }

        fn click(&mut self, at: Pos2, button: PointerButton, modifiers: Modifiers) {
            let press = |pressed| Event::PointerButton { pos: at, button, pressed, modifiers };
            self.frame_held(vec![Event::PointerMoved(at), press(true), press(false)], modifiers);
            self.frame(Vec::new());
        }

        fn key(&mut self, key: Key, modifiers: Modifiers) {
            // Released too: egui takes a second press of a key still down for a repeat.
            let event = |pressed| Event::Key { key, physical_key: None, pressed, repeat: false, modifiers };
            self.frame(vec![event(true), event(false)]);
            self.frame(Vec::new());
        }

        /// The open context menu's rectangle.
        fn menu(&self) -> Option<Rect> {
            self.ctx
                .memory(|m| m.areas().visible_layer_ids().into_iter().filter(|l| l.order == egui::Order::Foreground).find_map(|l| m.area_rect(l.id)))
        }

        fn selected(&self) -> Vec<u64> {
            self.app.session.scene.selected_models.iter().copied().collect()
        }

        fn name(&self, i: usize) -> String {
            self.app.session.scene.model(self.ids[i]).unwrap().name.clone()
        }
    }

    #[test]
    fn rows_select_and_their_eyes_hide_and_show() {
        let mut r = Rig::new(3);
        assert!(r.card().right() <= crate::models_card::beside(&r.app).x, "the tool cards open past it");
        let settings = r.ctx.memory(|m| m.area_rect(egui::Id::new(crate::object_settings::CARD_ID))).unwrap();
        assert!(settings.left() >= r.card().right(), "{settings:?} beside {:?}", r.card());

        r.click(r.row(1, false), PointerButton::Primary, Modifiers::NONE);
        assert_eq!(r.selected(), vec![r.ids[1]]);
        r.click(r.row(2, false), PointerButton::Primary, Modifiers::SHIFT);
        assert_eq!(r.selected(), vec![r.ids[1], r.ids[2]], "Shift adds");

        let depth = r.app.session.undo_depth();
        r.click(r.row(1, true), PointerButton::Primary, Modifiers::NONE);
        assert!(r.app.session.scene.model(r.ids[1]).unwrap().hidden, "the eye hides");
        assert_eq!(r.selected(), vec![r.ids[2]], "and takes it out of the selection");
        assert_eq!(r.app.session.undo_depth(), depth + 1, "one step");
        r.click(r.row(1, false), PointerButton::Primary, Modifiers::NONE);
        assert!(r.selected().is_empty(), "a hidden model is not selected");
        r.click(r.row(1, true), PointerButton::Primary, Modifiers::NONE);
        assert!(!r.app.session.scene.model(r.ids[1]).unwrap().hidden, "and shows again");

        // The chevron folds the card to its header, and unfolds it.
        let open = r.card().height();
        let chevron = pos2(r.card().right() - 8.0 - 13.0, r.card().top() + FORM_HEADER_HEIGHT / 2.0);
        r.click(chevron, PointerButton::Primary, Modifiers::NONE);
        r.frame(Vec::new());
        assert!(r.app.models_card.folded);
        assert!(r.card().height() < open - 2.0 * TREE_ROW_HEIGHT, "{} vs {open}", r.card().height());
        r.click(chevron, PointerButton::Primary, Modifiers::NONE);
        assert!(!r.app.models_card.folded);
    }

    #[test]
    fn the_menu_duplicates_renames_and_deletes() {
        let mut r = Rig::new(2);
        r.click(r.row(1, false), PointerButton::Secondary, Modifiers::NONE);
        assert_eq!(r.selected(), vec![r.ids[1]], "a right click selects the row");
        let menu = r.menu().expect("the menu is open");
        // Duplicate, Rename, Hide, a rule, Delete: 30, 30, 30, 11 and 30 pt tall.
        let item = |y: f32| pos2(menu.center().x, menu.top() + 7.0 + y);
        r.click(item(15.0), PointerButton::Primary, Modifiers::NONE);
        assert_eq!(r.app.session.scene.models().len(), 3, "duplicated");
        let copy = r.selected();
        assert_eq!(copy.len(), 1);
        assert_eq!(r.app.session.scene.model(copy[0]).unwrap().name, "b");
        assert!(r.menu().is_none(), "the menu closes");

        r.click(r.row(0, false), PointerButton::Secondary, Modifiers::NONE);
        let menu = r.menu().unwrap();
        let item = |y: f32| pos2(menu.center().x, menu.top() + 7.0 + y);
        r.click(item(45.0), PointerButton::Primary, Modifiers::NONE);
        assert!(r.app.models_card.renaming.as_ref().is_some_and(|x| x.id == r.ids[0]), "Rename opens the name's field");
        r.frame(vec![Event::Text("Base".into())]);
        r.key(Key::Enter, Modifiers::NONE);
        assert_eq!(r.name(0), "Base", "the whole name was selected, so typing replaced it");
        assert!(r.app.models_card.renaming.is_none());

        // Delete on a selected model deletes the selection.
        r.app.run("model.select", json!({ "ids": [r.ids[0], r.ids[1]] }));
        r.frame(Vec::new());
        r.click(r.row(0, false), PointerButton::Secondary, Modifiers::NONE);
        assert_eq!(r.selected(), vec![r.ids[0], r.ids[1]], "a right click in the selection keeps it");
        let depth = r.app.session.undo_depth();
        let menu = r.menu().unwrap();
        r.click(pos2(menu.center().x, menu.top() + 7.0 + 3.0 * 30.0 + 11.0 + 15.0), PointerButton::Primary, Modifiers::NONE);
        assert_eq!(r.app.session.scene.models().iter().map(|m| m.id).collect::<Vec<_>>(), copy);
        assert_eq!(r.app.session.undo_depth(), depth + 1, "one step");
    }

    #[test]
    fn h_hides_the_selection_and_alt_h_shows_every_hidden_model() {
        let mut r = Rig::new(3);
        let hidden = |r: &Rig| r.ids.iter().map(|id| r.app.session.scene.model(*id).unwrap().hidden).collect::<Vec<_>>();
        let depth = r.app.session.undo_depth();
        r.key(Key::H, Modifiers::NONE);
        assert_eq!(hidden(&r), [false; 3], "nothing selected, nothing hidden");
        assert_eq!(r.app.session.undo_depth(), depth);

        r.app.run("model.select", json!({ "ids": [r.ids[0], r.ids[2]] }));
        r.key(Key::H, Modifiers::NONE);
        assert_eq!(hidden(&r), [true, false, true]);
        assert!(r.selected().is_empty());
        assert_eq!(r.app.session.undo_depth(), depth + 1, "one step");

        r.app.run("model.select", json!({ "ids": [r.ids[1]] }));
        r.app.set_mode(EditMode::Support);
        r.key(Key::H, Modifiers::NONE);
        assert_eq!(hidden(&r), [true; 3], "Support edition hides too");
        assert!(crate::support_brace::tool(&r.app).is_none());

        r.app.set_mode(EditMode::Slice);
        r.key(Key::H, Modifiers::ALT);
        assert_eq!(hidden(&r), [false; 3], "Alt+H shows them all, in any edition");
        assert_eq!(r.app.session.undo_depth(), depth + 3, "in one step");
        r.key(Key::H, Modifiers::ALT);
        assert_eq!(r.app.session.undo_depth(), depth + 3, "none hidden: no step");
    }

    #[test]
    fn f2_and_a_double_click_rename_in_the_row_and_esc_puts_the_name_back() {
        let mut r = Rig::new(2);
        r.app.run("model.select", json!({ "ids": [r.ids[1]] }));
        r.frame(Vec::new());
        r.key(Key::F2, Modifiers::NONE);
        assert!(r.app.models_card.renaming.is_some());
        r.frame(vec![Event::Text("Lid".into())]);
        r.key(Key::Escape, Modifiers::NONE);
        assert_eq!(r.name(1), "b", "Esc keeps the name");
        assert!(r.app.models_card.renaming.is_none());

        let depth = r.app.session.undo_depth();
        let at = r.row(0, false);
        let press = |pressed| Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
        r.frame(vec![Event::PointerMoved(at), press(true), press(false), press(true), press(false)]);
        r.frame(Vec::new());
        assert!(r.app.models_card.renaming.as_ref().is_some_and(|x| x.id == r.ids[0]), "a double click renames");
        r.frame(vec![Event::Text("  ".into())]);
        r.key(Key::Enter, Modifiers::NONE);
        assert_eq!(r.name(0), "a", "an empty name is refused");
        r.app.run("model.select", json!({ "ids": [r.ids[0]] }));
        r.key(Key::F2, Modifiers::NONE);
        r.frame(vec![Event::Text("Top".into())]);
        r.key(Key::Enter, Modifiers::NONE);
        assert_eq!(r.name(0), "Top");
        assert_eq!(r.app.session.undo_depth(), depth + 1, "the rename is one step");

        r.key(Key::D, Modifiers::COMMAND);
        assert_eq!(r.app.session.scene.models().len(), 3, "Ctrl+D duplicates the selection");
    }
}
