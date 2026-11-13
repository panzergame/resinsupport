//! Exporting a CTB file from the window (File ▸ Export CTB…, Slice ▸ Export CTB, Ctrl+E). The
//! previews are drawn from the scene first; the scene is sliced when the kept job no longer
//! matches it; the file is written on a worker thread. Until the summary that follows is closed,
//! the window shows nothing else: the 2D view of the layer being written (then the last one)
//! under a card with the progress and Cancel, then the summary, so nothing can change the scene
//! the file is made from.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Duration, Instant};

use egui::{Align2, Sense, vec2};
use resin_engine::io::IoError;
use resin_engine::io::ctb::{self, CtbOptions, LARGE_PREVIEW, Previews, SMALL_PREVIEW};
use resin_engine::slice::{Cancel, Progress, SlicedJob, Stamp};

use crate::layer_image::LayerImage;
use crate::theme::Tokens;
use crate::theme::metrics::*;
use crate::ui_kit::{self, Action, Buttons, Card};
use crate::{ResinApp, gpu, i18n, layer_view, viewport};

/// How often the window looks at the export, and decodes the layer being written at most.
const POLL: Duration = Duration::from_millis(50);
const DECODE_EVERY: f64 = 0.2;

pub struct Export {
    path: PathBuf,
    /// Drawn by the renderer when the export started; without one, the plain previews.
    previews: Option<Previews>,
    phase: Phase,
    /// The layer shown, decoded, and when (egui's clock).
    image: Option<(Arc<LayerImage>, f64)>,
    started: Instant,
    /// How long the slice took, when the export sliced.
    slicing: Option<Duration>,
}

enum Phase {
    /// Waiting for the slice of the scene as it was when the export started.
    Slicing(Stamp),
    Writing(Writer),
    /// Written, or failed: the summary is open until it is closed.
    Done(Summary),
}

struct Writer {
    job: Arc<SlicedJob>,
    progress: Arc<Progress>,
    cancel: Arc<Cancel>,
    /// The bytes written.
    done: Receiver<Result<usize, IoError>>,
    started: Instant,
}

struct Summary {
    job: Arc<SlicedJob>,
    /// The bytes written, or why nothing was.
    outcome: Result<usize, String>,
    slicing: Option<Duration>,
    writing: Duration,
}

/// `path`, with `.ctb` added unless it has it: file dialogs leave it to the name typed.
pub fn with_ctb_extension(path: PathBuf) -> PathBuf {
    if ctb::is_ctb_path(&path) {
        return path;
    }
    let mut s = path.into_os_string();
    s.push(".");
    s.push(ctb::EXTENSION);
    s.into()
}

/// Export the scene to `path`: nothing else happens in the window until its summary is closed.
pub(crate) fn start(app: &mut ResinApp, path: PathBuf) {
    if app.export.is_some() || app.session.scene.models().is_empty() {
        return;
    }
    let started = Instant::now();
    // An open tool would cancel back to placements the export does not see.
    app.cancel_pending();
    let previews = render_previews(app);
    let phase = if app.session.job_is_current() {
        let Some(writer) = write(app, &path, previews.as_ref()) else { return };
        Phase::Writing(writer)
    } else {
        let stamp = app.session.slice_stamp();
        if app.slicer.running_stamp() != Some(stamp) {
            app.slice_now();
        }
        Phase::Slicing(stamp)
    };
    app.export = Some(Export { path, previews, phase, image: None, started, slicing: None });
}

/// The scene drawn for both previews, each at its own aspect ratio; none without a GPU.
fn render_previews(app: &mut ResinApp) -> Option<Previews> {
    let rs = app.render_state.clone()?;
    // The dark palette whatever the window's: a printer's screen is dark.
    let tokens = Tokens::for_dark(true);
    let mut draw = |(w, h): (u32, u32)| {
        let frame = viewport::preview_frame(app, &tokens, f64::from(w) / f64::from(h));
        gpu::render_offscreen(&rs, &frame, w, h)
    };
    Some(Previews { large: draw(LARGE_PREVIEW)?, small: draw(SMALL_PREVIEW)? })
}

/// Write the session's job to `path` on a worker thread; none without a job.
fn write(app: &ResinApp, path: &std::path::Path, previews: Option<&Previews>) -> Option<Writer> {
    let job = app.session.job().map(|j| Arc::clone(&j.sliced))?;
    let previews = previews.cloned();
    let (progress, cancel) = (Arc::new(Progress::default()), Arc::new(Cancel::default()));
    let (tx, done) = mpsc::channel();
    let (j, p, c, path) = (Arc::clone(&job), Arc::clone(&progress), Arc::clone(&cancel), path.to_path_buf());
    let work = move || {
        // The plain previews walk every layer: here, off the window thread.
        let previews = previews.unwrap_or_else(|| Previews::plain(&j));
        let opts = CtbOptions { encryption_key: ctb::DEFAULT_KEY, modified_minutes: ctb::minutes_now() };
        let out = ctb::write_ctb_with(&j, &previews, &opts, &p, &c)
            .and_then(|bytes| resin_engine::io::write_atomic(&path, &bytes, false).map(|()| bytes.len()).map_err(IoError::from));
        let _ = tx.send(out);
    };
    if let Err(e) = std::thread::Builder::new().name("ctb-writer".into()).spawn(work) {
        log::error!("could not start the CTB writer thread: {e}");
    }
    Some(Writer { job, progress, cancel, done, started: Instant::now() })
}

/// Each frame: write once the slice is in, and open the summary once the file is written.
pub(crate) fn drive(app: &mut ResinApp, ctx: &egui::Context) {
    let Some(ex) = &app.export else { return };
    let name = crate::file_name(&ex.path);
    match &ex.phase {
        Phase::Slicing(stamp) => {
            ctx.request_repaint_after(POLL);
            if app.session.job_is_current()
                && let Some(writer) = write(app, &ex.path, ex.previews.as_ref())
            {
                if let Some(ex) = &mut app.export {
                    ex.slicing = Some(ex.started.elapsed());
                    ex.phase = Phase::Writing(writer);
                }
            } else if app.slicer.running_stamp() != Some(*stamp) {
                // The slice failed (its error is in the status bar) or was cancelled.
                app.export = None;
                if app.status.as_ref().is_none_or(|(_, error)| !error) {
                    app.status = Some((i18n::format("status.export_failed", &[("name", &name), ("error", &i18n::tr("export.no_job"))]), true));
                }
            }
        }
        Phase::Writing(w) => {
            ctx.request_repaint_after(POLL);
            let outcome = match w.done.try_recv() {
                Err(TryRecvError::Empty) => return,
                Ok(Err(IoError::Cancelled)) => {
                    app.export = None;
                    app.status = Some((i18n::format("status.export_cancelled", &[("name", &name)]), false));
                    return;
                }
                Ok(Ok(bytes)) => Ok(bytes),
                Ok(Err(e)) => Err(e.to_string()),
                Err(TryRecvError::Disconnected) => Err(i18n::tr("export.stopped").to_string()),
            };
            app.status = Some(match &outcome {
                Ok(bytes) => {
                    (i18n::format("status.exported", &[("name", &name), ("layers", &w.job.layers.len()), ("size", &megabytes(*bytes))]), false)
                }
                Err(e) => (i18n::format("status.export_failed", &[("name", &name), ("error", e)]), true),
            });
            let summary = Summary { job: Arc::clone(&w.job), outcome, slicing: ex.slicing, writing: w.started.elapsed() };
            if let Some(ex) = &mut app.export {
                ex.phase = Phase::Done(summary);
            }
        }
        Phase::Done(_) => {}
    }
}

/// Stop the export: a slice it started stops at once; a file being written stops at its next
/// layer, and nothing is written over the target.
pub(crate) fn cancel(app: &mut ResinApp) {
    let Some(ex) = &app.export else { return };
    match &ex.phase {
        Phase::Slicing(_) => {
            let name = crate::file_name(&ex.path);
            app.export = None;
            app.cancel_slice();
            app.status = Some((i18n::format("status.export_cancelled", &[("name", &name)]), false));
        }
        Phase::Writing(w) => w.cancel.cancel(),
        Phase::Done(_) => {}
    }
}

/// Close the summary: the window is back. Only once the export is over.
pub(crate) fn close(app: &mut ResinApp) {
    if app.export.as_ref().is_some_and(|ex| matches!(ex.phase, Phase::Done(_))) {
        app.export = None;
    }
}

fn megabytes(bytes: usize) -> String {
    format!("{:.1}", bytes as f64 / 1_000_000.0)
}

fn seconds(d: Duration) -> String {
    format!("{:.1}", d.as_secs_f64())
}

/// The window while exporting: the layer being written, and the card over it.
pub(crate) fn screen(app: &mut ResinApp, ui: &mut egui::Ui, t: &Tokens) {
    let ctx = ui.ctx().clone();
    let time = ctx.input(|i| i.time);
    egui::CentralPanel::no_frame().show(ui, |ui| {
        let rect = ui.available_rect_before_wrap();
        ui.painter().rect_filled(rect, 0.0, t.viewport_bottom);
        // Takes every press and drag: the view is not navigated while exporting.
        ui.allocate_rect(rect, Sense::click_and_drag());
        let Some(ex) = &mut app.export else { return };
        // The layer being written, a few times a second; once done, the last one.
        let shown = match &ex.phase {
            Phase::Slicing(_) => None,
            Phase::Writing(w) => {
                let (done, total) = w.progress.get();
                Some((&w.job, done.min(total.saturating_sub(1)), DECODE_EVERY))
            }
            Phase::Done(d) => Some((&d.job, d.job.layers.len().saturating_sub(1), 0.0)),
        };
        if let Some((job, layer, every)) = shown {
            let stale = ex.image.as_ref().is_none_or(|(img, at)| img.key.1 != layer && time - at >= every);
            if stale && let Some(l) = job.layers.get(layer) {
                ex.image = Some((Arc::new(LayerImage::decode((job.stamp, layer), l, &job.profile, app.max_texture)), time));
            }
        }
        if let Some((image, _)) = &ex.image {
            layer_view::paint(ui, rect, image, &layer_view::fitted(rect, image), false, t);
        }
    });

    let Some(ex) = &app.export else { return };
    let name = crate::file_name(&ex.path);
    if let Phase::Done(summary) = &ex.phase {
        if summary_card(&ctx, t, &name, summary) != Action::None {
            close(app);
        }
        return;
    }
    let title = i18n::format("export.title", &[("name", &name)]);
    let (line, fraction, cancelling) = match &ex.phase {
        Phase::Slicing(_) => match app.slicer.progress() {
            Some((done, total)) if total > 0 => {
                (i18n::format("export.slicing", &[("done", &done), ("total", &total)]), done as f32 / total as f32, false)
            }
            _ => (i18n::tr("export.preparing").to_string(), 0.0, false),
        },
        Phase::Writing(w) => {
            let (done, total) = w.progress.get();
            let cancelling = w.cancel.is_cancelled();
            let line = if cancelling {
                i18n::tr("export.cancelling").to_string()
            } else {
                i18n::format("export.writing", &[("done", &done), ("total", &total)])
            };
            (line, if total > 0 { done as f32 / total as f32 } else { 0.0 }, cancelling)
        }
        Phase::Done(_) => return,
    };
    let mut stop = ctx.input(|i| i.key_pressed(egui::Key::Escape));
    let card = Card {
        id: egui::Id::new("export_ctb"),
        title: &title,
        icon: "",
        at: egui::Pos2::ZERO,
        bounds: None,
        width: FORM_CARD_WIDTH,
        buttons: Buttons::None,
    };
    card.show_anchored(&ctx, t, Align2::CENTER_BOTTOM, vec2(0.0, -SPACE_24), |ui| {
        ui.spacing_mut().item_spacing = vec2(0.0, SPACE_8);
        ui.label(line);
        ui.add(egui::ProgressBar::new(fraction.clamp(0.0, 1.0)).show_percentage());
        ui.add_enabled_ui(!cancelling, |ui| {
            if ui_kit::button(ui, i18n::tr("export.cancel")).clicked() {
                stop = true;
            }
        });
    });
    if stop && !cancelling {
        cancel(app);
    }
}

/// What was written and what printing it takes, or why nothing was written; Close (Enter, Esc)
/// ends the export.
fn summary_card(ctx: &egui::Context, t: &Tokens, name: &str, s: &Summary) -> Action {
    let (title, icon) = match s.outcome {
        Ok(_) => (i18n::format("export.done.title", &[("name", &name)]), "export"),
        Err(_) => (i18n::format("export.done.failed_title", &[("name", &name)]), ""),
    };
    let card = Card {
        id: egui::Id::new("export_ctb_done"),
        title: &title,
        icon,
        at: egui::Pos2::ZERO,
        bounds: None,
        width: FORM_CARD_WIDTH,
        buttons: Buttons::Close,
    };
    card.show_anchored(ctx, t, Align2::CENTER_BOTTOM, vec2(0.0, -SPACE_24), |ui| {
        let bytes = match &s.outcome {
            Ok(bytes) => *bytes,
            Err(e) => {
                ui.colored_label(t.error, e);
                return;
            }
        };
        let (job, st, settings) = (&s.job, &s.job.stats, &s.job.settings);
        let value = |ui: &mut egui::Ui, text: String| {
            ui.label(text);
        };
        ui_kit::row(ui, t, i18n::tr("export.done.size"), |ui| value(ui, i18n::format("export.done.megabytes", &[("size", &megabytes(bytes))])));
        ui_kit::row(ui, t, i18n::tr("export.done.layers"), |ui| {
            let (bottom, transition) = (settings.layers.bottom_count, settings.layers.transition_count);
            value(ui, i18n::format("export.done.layer_count", &[("layers", &job.layers.len()), ("bottom", &bottom), ("transition", &transition)]))
        });
        ui_kit::row(ui, t, i18n::tr("export.done.height"), |ui| {
            value(ui, i18n::format("export.done.millimetres", &[("mm", &format!("{:.2}", job.height()))]))
        });
        ui_kit::row(ui, t, i18n::tr("export.done.print_time"), |ui| value(ui, crate::duration(st.print_time_s)));
        ui_kit::row(ui, t, i18n::tr("export.done.resin"), |ui| {
            let grams = st.volume_ml * settings.estimates.resin_density;
            value(ui, i18n::format("export.done.resin_value", &[("ml", &format!("{:.1}", st.volume_ml)), ("g", &format!("{grams:.1}"))]))
        });
        let e = &settings.estimates;
        if e.bottle_cost > 0.0 && e.bottle_volume > 0.0 {
            ui_kit::row(ui, t, i18n::tr("export.done.cost"), |ui| value(ui, format!("{:.2}", st.volume_ml / e.bottle_volume * e.bottle_cost)));
        }
        ui_kit::row(ui, t, i18n::tr("export.done.time"), |ui| {
            let total = s.slicing.unwrap_or_default() + s.writing;
            value(
                ui,
                match s.slicing {
                    Some(slicing) => i18n::format(
                        "export.done.seconds_sliced",
                        &[("s", &seconds(total)), ("slicing", &seconds(slicing)), ("writing", &seconds(s.writing))],
                    ),
                    None => i18n::format("export.done.seconds", &[("s", &seconds(total))]),
                },
            )
        });
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn cube_stl(dir: &std::path::Path) -> PathBuf {
        let path = dir.join("cube.stl");
        let v = |x: f32, y: f32, z: f32| format!("vertex {x} {y} {z}\n");
        let mut s = String::from("solid c\n");
        let c = |i: usize| [(i & 1) as f32 * 10.0, ((i >> 1) & 1) as f32 * 10.0, ((i >> 2) & 1) as f32 * 10.0];
        for q in [[0, 2, 3, 1], [4, 5, 7, 6], [0, 1, 5, 4], [2, 6, 7, 3], [0, 4, 6, 2], [1, 3, 7, 5]] {
            for t in [[q[0], q[1], q[2]], [q[0], q[2], q[3]]] {
                s += "facet normal 0 0 0\nouter loop\n";
                for i in t {
                    let p = c(i);
                    s += &v(p[0], p[1], p[2]);
                }
                s += "endloop\nendfacet\n";
            }
        }
        s += "endsolid c\n";
        std::fs::write(&path, s).unwrap();
        path
    }

    fn summary(app: &ResinApp) -> Option<&Summary> {
        match &app.export.as_ref()?.phase {
            Phase::Done(s) => Some(s),
            _ => None,
        }
    }

    /// Run the export's frames until its summary is open.
    fn finish(app: &mut ResinApp, ctx: &egui::Context) {
        for _ in 0..2000 {
            app.drive_slicer(ctx);
            drive(app, ctx);
            if summary(app).is_some() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        panic!("the export did not finish");
    }

    #[test]
    fn exporting_slices_first_and_writes_the_job() {
        let dir = std::env::temp_dir().join(format!("resin-ui-export-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut app = ResinApp::build(crate::prefs::Prefs::default(), &[]);
        let ctx = egui::Context::default();
        let out = dir.join("cube.ctb");
        start(&mut app, out.clone());
        assert!(app.export.is_none(), "an empty scene has nothing to export");

        app.run("file.import_stl", json!({ "path": cube_stl(&dir), "fit": false })).unwrap();
        assert!(!app.session.job_is_current());
        start(&mut app, out.clone());
        assert!(app.export.is_some());
        finish(&mut app, &ctx);
        assert!(app.status.as_ref().is_some_and(|(_, error)| !error), "{:?}", app.status);
        let bytes = std::fs::read(&out).unwrap();
        let file = ctb::read_ctb(&bytes).unwrap();
        assert_eq!(file.layers.len(), 200, "the 10 mm cube, sliced at 0.05 mm");
        assert_eq!(file.large_preview.map(|p| (p.width, p.height)), Some(LARGE_PREVIEW));
        let s = summary(&app).unwrap();
        assert_eq!(s.outcome, Ok(bytes.len()));
        assert_eq!(s.job.layers.len(), 200);
        assert!(s.slicing.is_some(), "the job was stale: the export sliced it");
        // The summary stays until it is closed: more frames, a cancel, another start change nothing.
        app.drive_slicer(&ctx);
        drive(&mut app, &ctx);
        cancel(&mut app);
        start(&mut app, dir.join("again.ctb"));
        assert!(summary(&app).is_some() && !dir.join("again.ctb").exists());
        close(&mut app);
        assert!(app.export.is_none());

        // The job is current now: written without slicing.
        start(&mut app, out.clone());
        finish(&mut app, &ctx);
        assert!(summary(&app).is_some_and(|s| s.slicing.is_none() && s.outcome.is_ok()));
        close(&mut app);

        // Cancelled while slicing: nothing is written.
        let other = dir.join("other.ctb");
        app.run("model.move", json!({ "by": [1, 0, 0], "ids": [app.session.scene.models()[0].id] })).unwrap();
        start(&mut app, other.clone());
        cancel(&mut app);
        assert!(app.export.is_none() && !app.slicer.is_running());
        assert!(!other.exists());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn the_ctb_extension_is_added_once() {
        assert_eq!(with_ctb_extension(PathBuf::from("/a/plate")), PathBuf::from("/a/plate.ctb"));
        assert_eq!(with_ctb_extension(PathBuf::from("/a/plate.CTB")), PathBuf::from("/a/plate.CTB"));
        assert_eq!(with_ctb_extension(PathBuf::from("/a/plate.stl")), PathBuf::from("/a/plate.stl.ctb"));
    }
}
