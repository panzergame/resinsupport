//! L5: the egui desktop front end. Thin by design: panels read the session and act through
//! [`ResinApp::run`], which runs engine commands.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod gizmo;
pub mod gpu;
pub mod i18n;
pub mod icons;
pub mod navigation;
pub mod object_settings;
pub mod objects;
pub mod preferences;
pub mod prefs;
pub mod prefs_writer;
pub mod print_settings;
pub mod support_routes;
pub mod support_settings;
pub mod supports;
pub mod theme;
pub mod toolbar;
pub mod transform;
pub mod ui_kit;
pub mod viewport;

use resin_engine::Session;
use resin_engine::render::{Camera, CameraAnim};
use serde_json::{Value, json};

use crate::prefs::Prefs;
use crate::theme::Tokens;

pub use gpu::{DEPTH_BITS, MSAA_SAMPLES};

/// The edit mode: what a left click in the viewport works on. Tab switches.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EditMode {
    /// Object edition: select models, then move, rotate or scale them.
    #[default]
    Object,
    /// Support edition: place and select support tips.
    Support,
}

impl EditMode {
    /// Catalog id of its name.
    pub fn label(self) -> &'static str {
        match self {
            EditMode::Object => "mode.object",
            EditMode::Support => "mode.support",
        }
    }
    fn other(self) -> EditMode {
        match self {
            EditMode::Object => EditMode::Support,
            EditMode::Support => EditMode::Object,
        }
    }
}

pub struct ResinApp {
    pub session: Session,
    pub prefs: Prefs,
    pub mode: EditMode,
    /// The Move, Rotate or Scale tool in use.
    pub(crate) tool: Option<transform::Tool>,
    /// A press this frame, or a drag, on the gizmo: neither the camera nor picking may take it.
    pub(crate) gizmo_grab: bool,
    /// The Preferences window is open, on this page.
    pub(crate) prefs_open: bool,
    pub(crate) prefs_page: usize,
    pub(crate) nav: navigation::NavState,
    pub(crate) supports: supports::SupportTool,
    /// Every support's route to the plate, and the solids it is drawn with.
    pub(crate) routes: support_routes::RouteCache,
    pub(crate) viewport_rect: Option<egui::Rect>,
    anim: Option<CameraAnim>,
    /// egui's clock this frame, for camera animations.
    time: f64,
    /// The last command's outcome, shown in the status bar.
    status: Option<(String, bool)>,
    /// Files to import once the viewport has a size (from the command line).
    pending: Vec<std::path::PathBuf>,
    /// The support settings card's tab.
    pub(crate) settings_tab: usize,
    /// Restore defaults was clicked: its confirmation is open.
    pub(crate) confirm_reset: bool,
    /// The print settings card is open, on this tab.
    pub(crate) print_open: bool,
    pub(crate) print_tab: usize,
    /// The print settings' Restore defaults was clicked: its confirmation is open.
    pub(crate) confirm_print_reset: bool,
    prefs_writer: prefs_writer::PrefsWriter,
    /// The window title last sent, so it is sent only when it changes.
    title: String,
    /// A project waiting for its confirmation (the scene has unsaved changes, and opening can't
    /// be undone), and the STLs to import into it once open.
    pub(crate) pending_open: Option<(std::path::PathBuf, Vec<std::path::PathBuf>)>,
}

impl ResinApp {
    /// Set up the viewport renderer and restore saved preferences. Fails when the window has no
    /// wgpu renderer (eframe started with another backend).
    pub fn new(cc: &eframe::CreationContext<'_>) -> Result<ResinApp, String> {
        let rs = cc.wgpu_render_state.as_ref().ok_or("the window has no wgpu renderer")?;
        gpu::Renderer::install(rs);
        theme::apply(&cc.egui_ctx);
        let (prefs, problems) = cc.storage.map(prefs::load).unwrap_or_default();
        Ok(ResinApp::build(prefs, &problems))
    }

    /// The app on these preferences; `problems` names the stored ones that fell back to defaults.
    pub(crate) fn build(prefs: Prefs, problems: &[String]) -> ResinApp {
        let mut session = Session::new();
        if let Err(e) = session.set_support_settings(prefs.support_settings) {
            log::warn!("support settings: {e}");
        }
        if let Err(e) = session.set_print_config(prefs.print_config.clone()) {
            log::warn!("print settings: {e}");
        }
        if let Err(e) = session.set_lift_height(prefs.lift_height) {
            log::warn!("lift height: {e}");
        }
        let status = (!problems.is_empty()).then(|| (i18n::format("status.prefs_reset", &[("fields", &problems.join(", "))]), true));
        let writer = prefs_writer::PrefsWriter::read(prefs::to_json(&prefs));
        let mut app = ResinApp {
            session,
            prefs,
            mode: EditMode::default(),
            tool: None,
            gizmo_grab: false,
            prefs_open: false,
            prefs_page: 0,
            nav: Default::default(),
            supports: Default::default(),
            routes: Default::default(),
            viewport_rect: None,
            anim: None,
            time: 0.0,
            status,
            pending: Vec::new(),
            settings_tab: 0,
            confirm_reset: false,
            print_open: false,
            print_tab: 0,
            confirm_print_reset: false,
            prefs_writer: writer,
            title: String::new(),
            pending_open: None,
        };
        app.keep_settings();
        app
    }

    /// Import these files (open a project) once the window is up.
    pub fn open_later(&mut self, paths: impl IntoIterator<Item = std::path::PathBuf>) {
        self.pending.extend(paths);
    }

    /// Run a command, reporting a failure in the status bar.
    pub fn run(&mut self, id: &str, params: Value) -> Option<Value> {
        match self.session.execute(id, &params) {
            Ok(v) => {
                self.keep_settings();
                Some(v)
            }
            Err(e) => {
                log::warn!("{id}: {e}");
                self.status = Some((e.to_string(), true));
                None
            }
        }
    }

    /// Copy the session's support and print settings and lift height into the preferences when a command
    /// changed them, whoever ran it (a card, a script, undo), so they are saved.
    fn keep_settings(&mut self) {
        if self.prefs.support_settings != *self.session.support_settings() {
            self.prefs.support_settings = *self.session.support_settings();
        }
        if self.prefs.print_config != *self.session.print_config() {
            self.prefs.print_config = self.session.print_config().clone();
        }
        let lift = self.session.scene.lift_height();
        if self.prefs.lift_height != lift {
            self.prefs.lift_height = lift;
        }
    }

    /// Switch edit mode. Every mode change comes here, so its side effects live in one place.
    pub fn set_mode(&mut self, mode: EditMode) {
        if mode == self.mode {
            return;
        }
        if mode == EditMode::Support {
            self.cancel_pending();
        }
        self.mode = mode;
    }

    /// Cancel whatever is pending, as its own Cancel would: the open Move, Rotate or Scale tool.
    pub(crate) fn cancel_pending(&mut self) {
        transform::cancel(self);
    }

    /// Undo (`forward` false) or redo the last step. The tool is put away: what it remembers
    /// (placements to cancel back to, a drag's start) may describe models the step removes.
    pub fn step_history(&mut self, forward: bool) {
        self.tool = None;
        let (id, msg) = if forward { ("edit.redo", "status.redone") } else { ("edit.undo", "status.undone") };
        if let Some(v) = self.run(id, json!({})) {
            let step = i18n::tr_built(&format!("cmd.{}", v["command"].as_str().unwrap_or_default()));
            self.status = Some((i18n::format(msg, &[("step", &step)]), false));
        }
    }

    /// Run a view command and animate the camera to where it leaves it. Taps add up: a command
    /// run during an animation starts from where that animation is going.
    pub fn animate_command(&mut self, id: &str, params: Value) {
        if let Some(a) = self.anim.take() {
            self.session.camera = a.to;
        }
        let from = self.session.camera;
        if self.run(id, params).is_some() {
            self.animate_from(from);
        }
    }

    /// The iso view, framing everything.
    pub fn animate_home(&mut self, aspect: f64) {
        if let Some(a) = self.anim.take() {
            self.session.camera = a.to;
        }
        let from = self.session.camera;
        if self.run("view.standard", json!({ "view": "iso" })).is_some() && self.run("view.fit", json!({ "aspect": aspect })).is_some() {
            self.animate_from(from);
        }
    }

    /// Switch projection at once: a field-of-view blend reads as a dolly zoom, not a view change.
    /// A move in progress carries on in the new projection.
    pub fn toggle_projection(&mut self) {
        if self.run("view.projection", json!({ "mode": "toggle" })).is_some() {
            let fov = self.session.camera.fov;
            if let Some(a) = &mut self.anim {
                a.from.fov = fov;
                a.to.fov = fov;
            }
        }
    }

    fn animate_from(&mut self, from: Camera) {
        let to = self.session.camera;
        if to != from {
            self.session.camera = from;
            self.anim = Some(CameraAnim::new(from, to, self.now()));
        }
    }

    pub(crate) fn cancel_view_animation(&mut self) {
        self.anim = None;
    }

    pub(crate) fn step_view_animation(&mut self, ctx: &egui::Context) {
        let Some(a) = self.anim else { return };
        let (cam, done) = a.sample(ctx.input(|i| i.time));
        self.session.camera = cam;
        if done {
            self.anim = None;
        } else {
            ctx.request_repaint();
        }
    }

    fn now(&self) -> f64 {
        self.time
    }

    fn aspect(&self) -> f64 {
        self.viewport_rect.map_or(1.0, |r| f64::from(r.aspect_ratio()))
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn import_dialog(&mut self) {
        if let Some(paths) = rfd::FileDialog::new().add_filter("STL", &["stl", "STL"]).set_title(i18n::tr("menu.import_title")).pick_files() {
            self.import_all(paths);
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn open_dialog(&mut self) {
        let dialog = rfd::FileDialog::new().add_filter(i18n::tr("menu.project_filter"), &[PROJECT, "RSUP"]).set_title(i18n::tr("menu.open_title"));
        if let Some(path) = dialog.pick_file() {
            self.open_project(path, Vec::new());
        }
    }

    /// Replace the scene with the project at `path`, then import `then` into it. Opening starts a
    /// fresh undo history, so unsaved changes wait for [`ResinApp::confirm_open`] first.
    pub(crate) fn open_project(&mut self, path: std::path::PathBuf, then: Vec<std::path::PathBuf>) {
        if self.session.modified() {
            self.pending_open = Some((path, then));
            return;
        }
        self.pending_open = None;
        self.open_now(&path);
        self.import_all(then);
    }

    /// Ask before an open drops unsaved changes.
    fn confirm_open(&mut self, ctx: &egui::Context, t: &Tokens) {
        let Some((path, _)) = &self.pending_open else { return };
        let message = i18n::format("open.discard_confirm", &[("name", &file_name(path))]);
        let confirm = ui_kit::Confirm {
            id: egui::Id::new("open_discard"),
            icon: "import",
            title: i18n::tr("open.discard_title"),
            message: &message,
            ok: "open.discard_ok",
        };
        match confirm.show(ctx, t) {
            ui_kit::Action::Ok => {
                if let Some((path, then)) = self.pending_open.take() {
                    self.open_now(&path);
                    self.import_all(then);
                }
            }
            ui_kit::Action::Cancel => self.pending_open = None,
            ui_kit::Action::Reset | ui_kit::Action::None => {}
        }
    }

    fn open_now(&mut self, path: &std::path::Path) {
        let Some(path_str) = path.to_str() else {
            self.status = Some((i18n::format("status.bad_utf8", &[("path", &path.display())]), true));
            return;
        };
        // What the tool remembers describes models the open removes.
        self.tool = None;
        let from = self.session.camera;
        if self.run("file.open", json!({ "path": path_str })).is_some() {
            if !self.session.scene.models().is_empty() {
                self.run("view.fit", json!({ "aspect": self.aspect() }));
            }
            self.animate_from(from);
            self.status = Some((i18n::format("status.opened", &[("name", &file_name(path))]), false));
        }
    }

    /// Save to the scene's file, or ask where (`ask`, or the scene has none: Save As).
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn save_project(&mut self, ask: bool) {
        let path = match self.session.file() {
            Some(p) if !ask => std::path::PathBuf::from(p),
            current => {
                let name = current.and_then(|p| std::path::Path::new(p).file_name()).map_or_else(
                    || format!("{}.{PROJECT}", self.session.scene.models().first().map_or(i18n::tr("menu.untitled"), |m| m.name.as_str())),
                    |n| n.to_string_lossy().into_owned(),
                );
                let mut dialog = rfd::FileDialog::new()
                    .add_filter(i18n::tr("menu.project_filter"), &[PROJECT, "RSUP"])
                    .set_title(i18n::tr("menu.save_title"))
                    .set_file_name(name);
                if let Some(dir) = current.and_then(|p| std::path::Path::new(p).parent()) {
                    dialog = dialog.set_directory(dir);
                }
                let Some(path) = dialog.save_file() else { return };
                with_project_extension(path)
            }
        };
        let Some(path_str) = path.to_str() else {
            self.status = Some((i18n::format("status.bad_utf8", &[("path", &path.display())]), true));
            return;
        };
        if self.run("file.save", json!({ "path": path_str })).is_some() {
            self.status = Some((i18n::format("status.saved", &[("name", &file_name(&path))]), false));
        }
    }

    /// The window title: the project's file name, marked while it has unsaved changes.
    fn update_title(&mut self, ctx: &egui::Context) {
        let name = self.session.file().map(|p| file_name(std::path::Path::new(p)));
        let mark = if self.session.modified() { "*" } else { "" };
        let title = match name {
            Some(n) => format!("{n}{mark} — ResinSupport"),
            None => format!("{}{mark} — ResinSupport", i18n::tr("menu.untitled")),
        };
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }

    /// Import these files; into an empty scene, the view is then fitted to all of them at once.
    /// A project among them (the last one) is opened first, replacing the scene, and they join it.
    fn import_all(&mut self, paths: Vec<std::path::PathBuf>) {
        let (mut projects, paths): (Vec<_>, Vec<_>) = paths.into_iter().partition(|p| is_project(p));
        if let Some(project) = projects.pop() {
            self.open_project(project, paths);
            return;
        }
        if paths.is_empty() {
            return;
        }
        let fit = self.session.scene.models().is_empty();
        let from = self.session.camera;
        self.session.begin_step();
        for path in &paths {
            self.import(path);
        }
        self.session.end_step();
        if fit && !self.session.scene.models().is_empty() {
            self.run("view.fit", json!({ "aspect": self.aspect() }));
            self.animate_from(from);
        }
    }

    fn import(&mut self, path: &std::path::Path) {
        // Command parameters are JSON strings, and serialising a non-UTF-8 path through `json!`
        // would panic.
        let Some(path_str) = path.to_str() else {
            self.status = Some((i18n::format("status.bad_utf8", &[("path", &path.display())]), true));
            return;
        };
        if let Some(v) = self.run("file.import_stl", json!({ "path": path_str, "fit": false, "lifted": self.prefs.lift_on_import })) {
            let dropped = v["dropped"].as_u64().unwrap_or(0) as usize;
            let triangles = v["triangles"].as_u64().unwrap_or(0) as usize;
            let name = v["name"].as_str().unwrap_or(i18n::tr("models.unnamed"));
            let mut msg = i18n::count("status.imported", triangles, &[("name", &name)]);
            if dropped > 0 {
                msg = i18n::count("status.skipped", dropped, &[("imported", &msg)]);
            }
            self.status = Some((msg, false));
        }
    }

    fn models_panel(&mut self, ui: &mut egui::Ui) {
        ui.heading(i18n::tr("models.heading"));
        ui.separator();
        let mut delete = None;
        let mut select = None;
        let shift = ui.input(|i| i.modifiers.shift);
        egui::ScrollArea::vertical().show(ui, |ui| {
            for m in self.session.scene.models() {
                ui.horizontal(|ui| {
                    let selected = self.session.scene.selected_models.contains(&m.id);
                    let size = m.bounds.size();
                    let label = ui.selectable_label(selected, &m.name).on_hover_text(i18n::format(
                        "models.tip",
                        &[
                            ("triangles", &m.mesh.triangle_count()),
                            ("x", &format!("{:.2}", size.x)),
                            ("y", &format!("{:.2}", size.y)),
                            ("z", &format!("{:.2}", size.z)),
                            ("volume", &format!("{:.1}", m.volume())),
                        ],
                    ));
                    if label.clicked() {
                        select = Some(m.id);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("🗑").on_hover_text(i18n::tr("models.delete")).clicked() {
                            delete = Some(m.id);
                        }
                    });
                });
            }
        });
        if let Some(id) = delete {
            self.run("model.delete", json!({ "id": id }));
        }
        if let Some(id) = select {
            self.set_mode(EditMode::Object);
            self.run("model.select", json!({ "ids": [id], "mode": if shift { "toggle" } else { "replace" } }));
        }
    }

    fn status_bar(&mut self, ui: &mut egui::Ui, tokens: &Tokens) {
        ui.horizontal(|ui| {
            let tris: usize = self.session.scene.models().iter().map(|m| m.mesh.triangle_count()).sum();
            let scene = &self.session.scene;
            ui.label(i18n::tr(self.mode.label()));
            ui.separator();
            let selected = match self.mode {
                EditMode::Object => scene.selected_models.len(),
                EditMode::Support => scene.selected.len() + scene.selected_parts.len(),
            };
            ui.label(i18n::format(
                "status.summary",
                &[("models", &scene.models().len()), ("triangles", &tris), ("supports", &scene.supports().len())],
            ));
            if selected > 0 {
                ui.label(i18n::format("status.selected", &[("n", &selected)]));
            }
            let unreachable = self.routes.unreachable();
            if unreachable > 0 {
                ui.colored_label(tokens.support_unreachable, i18n::count("status.unreachable", unreachable, &[]));
            }
            if let Some((msg, error)) = &self.status {
                ui.separator();
                if *error {
                    ui.colored_label(tokens.error, msg);
                } else {
                    ui.label(msg);
                }
            }
        });
    }

    fn keys(&mut self, ctx: &egui::Context) {
        // Only typing keeps keys from the shortcuts: a button keeps the focus after a click, and
        // a focused widget alone would otherwise swallow every shortcut.
        if ctx.text_edit_focused() {
            return;
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            if ctx.input_mut(|i| i.consume_shortcut(&IMPORT)) {
                self.import_dialog();
            }
            if ctx.input_mut(|i| i.consume_shortcut(&OPEN)) {
                self.open_dialog();
            }
            // Ctrl+Shift+S first: Ctrl+S would match it too, egui ignoring an extra Shift.
            if ctx.input_mut(|i| i.consume_shortcut(&SAVE_AS)) {
                self.save_project(true);
            } else if ctx.input_mut(|i| i.consume_shortcut(&SAVE)) {
                self.save_project(false);
            }
        }
        // Ctrl+Shift+Z first: Ctrl+Z would match it too, egui ignoring an extra Shift.
        if ctx.input_mut(|i| i.consume_shortcut(&REDO_SHIFT_Z) || i.consume_shortcut(&REDO)) {
            self.step_history(true);
        }
        if ctx.input_mut(|i| i.consume_shortcut(&UNDO)) {
            self.step_history(false);
        }
        let pressed: Vec<(egui::Key, egui::Modifiers)> = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Key { key, pressed: true, repeat: false, modifiers, .. } => Some((*key, *modifiers)),
                    _ => None,
                })
                .collect()
        });
        for (key, mods) in pressed {
            if navigation::view_key(self, ctx, key, mods) {
                continue;
            }
            let plain = mods.is_none();
            match key {
                egui::Key::F if plain => self.animate_command("view.fit", json!({ "aspect": self.aspect() })),
                egui::Key::P if plain => self.toggle_projection(),
                egui::Key::Tab if plain => {
                    // egui has moved the focus on with this Tab; the key is ours.
                    ctx.memory_mut(|m| {
                        if let Some(id) = m.focused() {
                            m.surrender_focus(id);
                        }
                    });
                    self.set_mode(self.mode.other());
                }
                egui::Key::Escape if plain => self.tool = None,
                egui::Key::T if plain => transform::open(self, transform::Op::Move),
                egui::Key::R if plain => transform::open(self, transform::Op::Rotate),
                egui::Key::S if plain => transform::open(self, transform::Op::Scale),
                // Backspace too: Mac laptops have no Delete key.
                egui::Key::Delete | egui::Key::Backspace if plain => self.delete_selection(),
                _ => {}
            }
        }
    }
}

impl ResinApp {
    /// Delete what is selected in the current mode.
    fn delete_selection(&mut self) {
        match self.mode {
            EditMode::Object => {
                let ids: Vec<u64> = self.session.scene.selected_models.iter().copied().collect();
                self.session.begin_step();
                let n = ids.into_iter().filter(|id| self.run("model.delete", json!({ "id": id })).is_some()).count();
                self.session.end_step();
                if n > 0 {
                    self.status = Some((i18n::count("status.deleted_models", n, &[]), false));
                }
            }
            EditMode::Support if !self.session.scene.selected.is_empty() => {
                if let Some(v) = self.run("support.delete", json!({})) {
                    let n = v["removed"].as_array().map_or(0, Vec::len);
                    self.status = Some((i18n::count("status.deleted_supports", n, &[]), false));
                }
            }
            EditMode::Support => {}
        }
    }
}

pub(crate) const UNDO: egui::KeyboardShortcut = egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::Z);
pub(crate) const REDO: egui::KeyboardShortcut = egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::Y);
const REDO_SHIFT_Z: egui::KeyboardShortcut = egui::KeyboardShortcut::new(egui::Modifiers { shift: true, ..egui::Modifiers::COMMAND }, egui::Key::Z);

#[cfg(not(target_arch = "wasm32"))]
pub(crate) const IMPORT: egui::KeyboardShortcut = egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::I);
#[cfg(not(target_arch = "wasm32"))]
pub(crate) const OPEN: egui::KeyboardShortcut = egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::O);
#[cfg(not(target_arch = "wasm32"))]
pub(crate) const SAVE: egui::KeyboardShortcut = egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::S);
#[cfg(not(target_arch = "wasm32"))]
pub(crate) const SAVE_AS: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers { shift: true, ..egui::Modifiers::COMMAND }, egui::Key::S);

/// A project file's extension.
const PROJECT: &str = resin_engine::io::project::EXTENSION;

fn is_project(path: &std::path::Path) -> bool {
    path.extension().is_some_and(|e| e.eq_ignore_ascii_case(PROJECT))
}

/// `path`, with the project extension added unless it has it: file dialogs leave it to the name typed.
fn with_project_extension(path: std::path::PathBuf) -> std::path::PathBuf {
    if is_project(&path) {
        return path;
    }
    let mut s = path.into_os_string();
    s.push(".");
    s.push(PROJECT);
    s.into()
}

fn file_name(path: &std::path::Path) -> String {
    path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned())
}

impl eframe::App for ResinApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.time = ctx.input(|i| i.time);
        i18n::set_language(i18n::effective_language(&self.prefs.language));
        let tokens = Tokens::for_dark(ui.visuals().dark_mode);
        // Browsers drop bytes without a path; those wait for an import-from-bytes command.
        let dropped: Vec<_> =
            ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect());
        self.import_all(dropped);
        // Fitting the view needs the window's shape, so these wait for a laid-out viewport.
        if self.viewport_rect.is_some() {
            let pending = std::mem::take(&mut self.pending);
            self.import_all(pending);
        }
        self.keys(&ctx);
        self.update_title(&ctx);
        toolbar::app_bar(self, ui, &tokens);
        toolbar::ribbon(self, ui, &tokens);
        egui::Panel::bottom("status").show(ui, |ui| self.status_bar(ui, &tokens));
        egui::Panel::left("models").default_size(200.0).show(ui, |ui| self.models_panel(ui));
        egui::CentralPanel::no_frame().show(ui, |ui| viewport::show(self, ui, &tokens));
        transform::panel(self, &ctx);
        object_settings::panel(self, &ctx);
        support_settings::panel(self, &ctx);
        print_settings::panel(self, &ctx);
        preferences::window(self, &ctx);
        self.confirm_open(&ctx, &tokens);
        // A gesture's undo step (a gizmo drag, a dragged or typed field) lasts while the button is
        // held or the field is typed in; closing it here, not where it began, means a gesture cut
        // short (Esc mid-drag) can't leave every later command merged into it.
        if self.session.step_open() && !ctx.input(|i| i.pointer.any_down()) && !ctx.text_edit_focused() {
            self.session.end_step();
        }
        // After the frame's UI, so a setting changed in it is written now. `Instant` is not there
        // in the browser, where eframe's own saves are left to do it.
        #[cfg(not(target_arch = "wasm32"))]
        {
            let now = std::time::Instant::now();
            if self.prefs_writer.due(ctx.input(|i| i.events.iter().any(prefs_writer::changes_settings)), now)
                && let Some(storage) = frame.storage_mut()
                && let Some(json) = prefs::to_json(&self.prefs)
                && let Some(wait) = self.prefs_writer.store(storage, json, now)
            {
                ctx.request_repaint_after(wait);
            }
        }
        #[cfg(target_arch = "wasm32")]
        let _ = frame;
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        if let Some(json) = prefs::to_json(&self.prefs) {
            storage.set_string(prefs::STORAGE_KEY, json.clone());
            self.prefs_writer.written = Some(json);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One frame of the confirmation, with `key` pressed if given.
    fn frame(ctx: &egui::Context, app: &mut ResinApp, key: Option<egui::Key>) {
        let events = key.map(|key| egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE });
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 800.0))),
            events: events.into_iter().collect(),
            ..Default::default()
        };
        let mut out = ctx.run_ui(raw, |ui| app.confirm_open(ui.ctx(), &Tokens::for_dark(false)));
        out.textures_delta.clear();
    }

    #[test]
    fn opening_over_unsaved_changes_waits_for_its_confirmation() {
        let dir = std::env::temp_dir().join(format!("resin-ui-open-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let stl = dir.join("tri.stl");
        std::fs::write(&stl, "solid t\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 10 0 0\nvertex 0 10 0\nendloop\nendfacet\nendsolid t\n")
            .unwrap();
        let project = dir.join("plate.rsup");
        let mut app = ResinApp::build(Prefs::default(), &[]);
        app.run("file.import_stl", json!({ "path": stl, "fit": false })).unwrap();
        app.run("file.save", json!({ "path": project })).unwrap();
        app.open_project(project.clone(), Vec::new());
        assert!(app.pending_open.is_none(), "nothing unsaved: opened at once");
        assert_eq!(app.session.undo_depth(), 0);

        app.run("model.move", json!({ "by": [5, 0, 0], "ids": [app.session.scene.models()[0].id] })).unwrap();
        let moved = app.session.scene.models()[0].placement;
        app.open_project(project.clone(), vec![stl.clone()]);
        assert!(app.pending_open.is_some());
        assert_eq!(app.session.scene.models()[0].placement, moved, "nothing changes before the answer");
        let ctx = egui::Context::default();
        frame(&ctx, &mut app, None);
        frame(&ctx, &mut app, None);
        assert!(app.pending_open.is_some(), "still asking");
        frame(&ctx, &mut app, Some(egui::Key::Escape));
        assert!(app.pending_open.is_none());
        assert_eq!((app.session.scene.models()[0].placement, app.session.scene.models().len()), (moved, 1), "Cancel keeps the scene");

        app.open_project(project, vec![stl]);
        frame(&ctx, &mut app, None);
        frame(&ctx, &mut app, Some(egui::Key::Enter));
        assert!(app.pending_open.is_none());
        assert_eq!(app.session.scene.models().len(), 2, "opened, then the STL dropped with it imported");
        assert_ne!(app.session.scene.models()[0].placement, moved);
        std::fs::remove_dir_all(dir).ok();
    }
}
