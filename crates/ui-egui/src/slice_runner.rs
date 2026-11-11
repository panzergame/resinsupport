//! Slicing off the window thread. One job at a time on a worker thread, its layers spread over
//! scoped threads; starting another cancels it. The browser has no threads: there the job runs
//! where it is started, one layer after another.

use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, TryRecvError};

use resin_engine::SliceSource;
use resin_engine::slice::{Cancel, Progress, SliceError, SlicedJob, Stamp, slice_with};

/// How long after the last print settings change a new slice starts, seconds.
pub const DEBOUNCE: f64 = 0.5;

/// A finished job and the supports it left out, or why there is none.
pub type Outcome = Result<(SlicedJob, usize), String>;

struct Running {
    stamp: Stamp,
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
    /// Cancel the running job, if any, and slice `source`.
    pub fn start(&mut self, source: SliceSource) {
        self.stop();
        self.pending = None;
        self.held = None;
        self.started += 1;
        let (progress, cancel) = (Arc::new(Progress::default()), Arc::new(Cancel::default()));
        let (tx, done) = mpsc::channel();
        let stamp = source.stamp();
        let (p, c) = (Arc::clone(&progress), Arc::clone(&cancel));
        let job = move || {
            let (input, unreachable) = source.input();
            let out = slice_with(&input, &p, &c, &for_each_layer).map(|j| (j, unreachable));
            // The receiver is gone when this job was cancelled: nobody wants its result.
            let _ = tx.send(out);
        };
        #[cfg(not(target_arch = "wasm32"))]
        if let Err(e) = std::thread::Builder::new().name("slicer".into()).spawn(job) {
            log::error!("could not start the slicer thread: {e}");
        }
        #[cfg(target_arch = "wasm32")]
        job();
        self.running = Some(Running { stamp, progress, cancel, done });
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
}

/// Every layer, on as many threads as the machine has, this one among them.
#[cfg(not(target_arch = "wasm32"))]
fn for_each_layer(count: usize, layer: &(dyn Fn(usize) + Sync)) {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let next = AtomicUsize::new(0);
    let work = || {
        loop {
            let i = next.fetch_add(1, Ordering::Relaxed);
            if i >= count {
                break;
            }
            layer(i);
        }
    };
    let helpers = std::thread::available_parallelism().map_or(1, |n| n.get()).min(count).saturating_sub(1);
    std::thread::scope(|s| {
        for _ in 0..helpers {
            // A helper that cannot start leaves its share to the others.
            let _ = std::thread::Builder::new().name("slicer-layer".into()).spawn_scoped(s, work);
        }
        work();
    });
}

#[cfg(target_arch = "wasm32")]
fn for_each_layer(count: usize, layer: &(dyn Fn(usize) + Sync)) {
    (0..count).for_each(layer);
}
