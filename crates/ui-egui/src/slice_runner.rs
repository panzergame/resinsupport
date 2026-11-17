//! Slicing off the window thread. One job at a time on a worker thread, its layers spread over
//! scoped threads, the one on screen first and readable before the job is in; starting another
//! cancels it. The browser has no threads: there the job runs where it is started, one layer after
//! another.

use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, TryRecvError};

use resin_engine::SliceSource;
use resin_engine::slice::{Cancel, Layer, PrinterProfile, Progress, SliceError, SlicedJob, Stamp, slice_with};

/// How long after the last print settings change a new slice starts, seconds.
pub const DEBOUNCE: f64 = 0.5;

/// A finished job and the supports it left out, or why there is none.
pub type Outcome = Result<(SlicedJob, usize), String>;

struct Running {
    stamp: Stamp,
    profile: PrinterProfile,
    progress: Arc<Progress>,
    cancel: Arc<Cancel>,
    done: Receiver<Result<(SlicedJob, usize), SliceError>>,
}

#[derive(Default)]
pub struct SliceRunner {
    running: Option<Running>,
    /// The stamp a change left, and when (egui's clock) to slice it unless it changes again.
    pub(crate) pending: Option<(Stamp, f64)>,
    /// The stamp the user cancelled at: nothing starts on its own again until the scene or a
    /// setting moves past it.
    pub(crate) held: Option<Stamp>,
    /// Jobs started so far.
    pub(crate) started: u64,
}

impl SliceRunner {
    /// Cancel the running job, if any, and slice `source`, layer `shown` first.
    pub fn start(&mut self, source: SliceSource, shown: usize) {
        self.stop();
        self.pending = None;
        self.held = None;
        self.started += 1;
        let (progress, cancel) = (Arc::new(Progress::default()), Arc::new(Cancel::default()));
        let (tx, done) = mpsc::channel();
        progress.want(shown);
        let (stamp, profile) = (source.stamp(), source.profile().clone());
        let (p, c) = (Arc::clone(&progress), Arc::clone(&cancel));
        let job = move || {
            let (input, unreachable) = source.input();
            let out = slice_with(&input, &p, &c, &run_workers).map(|j| (j, unreachable));
            // The receiver is gone when this job was cancelled: nobody wants its result.
            let _ = tx.send(out);
        };
        #[cfg(not(target_arch = "wasm32"))]
        if let Err(e) = std::thread::Builder::new().name("slicer".into()).spawn(job) {
            log::error!("could not start the slicer thread: {e}");
        }
        #[cfg(target_arch = "wasm32")]
        job();
        self.running = Some(Running { stamp, profile, progress, cancel, done });
    }

    /// Cancel the running job; its result is dropped.
    pub fn stop(&mut self) {
        if let Some(r) = self.running.take() {
            r.cancel.cancel();
        }
    }

    /// Cancel the running job at the user's word: nothing starts on its own until `stamp`
    /// changes.
    pub fn hold(&mut self, stamp: Stamp) {
        self.stop();
        self.pending = None;
        self.held = Some(stamp);
    }

    /// The running job's result, once it is there.
    pub fn poll(&mut self) -> Option<Outcome> {
        let r = self.running.as_ref()?;
        let out = match r.done.try_recv() {
            Ok(out) => out.map_err(|e| e.to_string()),
            Err(TryRecvError::Empty) => return None,
            Err(TryRecvError::Disconnected) => Err("the slicer stopped without a result".into()),
        };
        self.running = None;
        Some(out)
    }

    pub fn is_running(&self) -> bool {
        self.running.is_some()
    }

    /// What the running job was started from.
    pub fn running_stamp(&self) -> Option<Stamp> {
        self.running.as_ref().map(|r| r.stamp)
    }

    /// The running job's layers done and to do; to do is 0 while its solids are being built.
    pub fn progress(&self) -> Option<(usize, usize)> {
        self.running.as_ref().map(|r| r.progress.get())
    }

    /// Slice the running job's layer `i` next.
    pub fn want(&self, i: usize) {
        if let Some(r) = &self.running {
            r.progress.want(i);
        }
    }

    /// Which of the running job's layers stands for layer `i` (its last one when `i` is past
    /// the end, as a kept job would show), once its layers are counted.
    pub fn layer_key(&self, i: usize) -> Option<(Stamp, usize)> {
        let r = self.running.as_ref()?;
        let (_, total) = r.progress.get();
        (total > 0).then(|| (r.stamp, i.min(total - 1)))
    }

    /// The running job's layer `i`, once sliced, and the printer it is sliced for.
    pub fn layer(&self, i: usize) -> Option<(Layer, &PrinterProfile)> {
        let r = self.running.as_ref()?;
        Some((r.progress.layer(i)?, &r.profile))
    }
}

/// The slicer's workers: as many as the machine has threads, this one among them.
#[cfg(not(target_arch = "wasm32"))]
fn run_workers(count: usize, work: &(dyn Fn() -> bool + Sync)) {
    let helpers = std::thread::available_parallelism().map_or(1, |n| n.get()).min(count).saturating_sub(1);
    std::thread::scope(|s| {
        for _ in 0..helpers {
            // A helper that cannot start leaves its share to the others.
            let _ = std::thread::Builder::new().name("slicer-layer".into()).spawn_scoped(s, || while work() {});
        }
        while work() {}
    });
}

#[cfg(target_arch = "wasm32")]
fn run_workers(_: usize, work: &(dyn Fn() -> bool + Sync)) {
    while work() {}
}
