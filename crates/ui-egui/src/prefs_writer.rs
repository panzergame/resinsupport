// Ported from SolidCraft (MIT OR Apache-2.0), apps/solidcraft/src/main.rs (`PrefsWriter`,
// `changes_settings`, and their tests) at 4bdffef0; see ATTRIBUTION.md. Reduced: the preferences
// are the only settings written.

//! Preferences written to eframe's storage soon after they change. eframe saves them only every
//! 30 s and when the window closes normally, so a change was lost whenever the app did not get
//! that far: ended from a task manager, stopped by a sign-out or shutdown, or crashed on the way
//! out.
//!
//! Some settings can change continuously (a field dragged), so they are written at most once per
//! [`PrefsWriter::INTERVAL`], and a change made in between is written when that interval ends.

use std::time::{Duration, Instant};

use crate::prefs;

#[derive(Default)]
pub struct PrefsWriter {
    /// The preferences written (or read at start) last.
    pub(crate) written: Option<String>,
    /// The previous frame had input that can change settings: what it changed shows in this
    /// frame's preferences.
    input_before: bool,
    /// When the preferences were last written by this writer.
    last_write: Option<Instant>,
    /// A change waits until then to be written.
    pending_until: Option<Instant>,
}

impl PrefsWriter {
    /// The least time between two writes.
    pub const INTERVAL: Duration = Duration::from_secs(1);

    /// The preferences as they were read at start: they are stored already.
    pub fn read(prefs: Option<String>) -> Self {
        Self { written: prefs, ..Self::default() }
    }

    /// Whether this frame's preferences are worth comparing: input that can change them came in
    /// this frame or the one before, or a waiting change's time has come. Pointer motion alone
    /// isn't such input; eframe's own save still catches anything else.
    pub fn due(&mut self, input: bool, now: Instant) -> bool {
        let due = input || self.input_before || self.pending_until.is_some_and(|at| now >= at);
        self.input_before = input;
        due
    }

    /// Write `prefs` and flush them if they differ from those written last and the last write is
    /// at least [`Self::INTERVAL`] old. Returns how long a change waits otherwise: the caller asks
    /// for a frame then, so an idle app still writes it.
    pub fn store(&mut self, storage: &mut dyn eframe::Storage, prefs: String, now: Instant) -> Option<Duration> {
        if self.written.as_deref() == Some(prefs.as_str()) {
            self.pending_until = None;
            return None;
        }
        if let Some(wait) = self.last_write.map(|at| Self::INTERVAL.saturating_sub(now.saturating_duration_since(at))).filter(|w| !w.is_zero()) {
            self.pending_until = Some(now + wait);
            return Some(wait);
        }
        storage.set_string(prefs::STORAGE_KEY, prefs.clone());
        storage.flush();
        self.written = Some(prefs);
        self.last_write = Some(now);
        self.pending_until = None;
        None
    }
}

/// Input that can change a setting: a click, a key, typed or pasted text, a wheel or a pinch.
/// Pointer motion, which comes every frame while the mouse moves, cannot by itself.
pub fn changes_settings(event: &egui::Event) -> bool {
    matches!(
        event,
        egui::Event::PointerButton { .. }
            | egui::Event::Key { .. }
            | egui::Event::Text(_)
            | egui::Event::Paste(_)
            | egui::Event::Cut
            | egui::Event::Ime(_)
            | egui::Event::MouseWheel { .. }
            | egui::Event::Zoom(_)
            | egui::Event::Rotate(_)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prefs::{Prefs, STORAGE_KEY, load, to_json};
    use std::collections::HashMap;

    /// eframe's settings file: what was flushed to disk is what the next start reads.
    #[derive(Default)]
    struct Disk {
        pending: HashMap<String, String>,
        flushed: HashMap<String, String>,
        flushes: usize,
    }

    impl eframe::Storage for Disk {
        fn get_string(&self, key: &str) -> Option<String> {
            self.pending.get(key).cloned()
        }
        fn set_string(&mut self, key: &str, value: String) {
            self.pending.insert(key.into(), value);
        }
        fn remove_string(&mut self, key: &str) {
            self.pending.remove(key);
        }
        fn flush(&mut self) {
            self.flushed = self.pending.clone();
            self.flushes += 1;
        }
    }

    impl Disk {
        /// What the next start reads.
        fn next_start(&self) -> Prefs {
            load(&Disk { pending: self.flushed.clone(), ..Disk::default() }).0
        }
    }

    fn json(p: &Prefs) -> String {
        to_json(p).unwrap()
    }

    /// A support setting changed in the card is on disk in that frame, so the next start has it
    /// even when this one never closes normally.
    #[test]
    fn changed_settings_reach_the_disk_in_the_frame_they_change() {
        let mut prefs = Prefs::default();
        let mut disk = Disk::default();
        let mut writer = PrefsWriter::read(Some(json(&prefs)));
        let now = Instant::now();
        assert_eq!(writer.store(&mut disk, json(&prefs), now), None);
        assert_eq!(disk.flushes, 0, "the settings read at start are not written again");
        prefs.support_settings.medium.tip.length = 2.4;
        prefs.nav = "blender".into();
        assert!(writer.due(true, now), "a frame with input is checked");
        assert_eq!(writer.store(&mut disk, json(&prefs), now), None);
        assert_eq!(disk.flushes, 1, "flushed at once, not left for eframe's next save");
        assert_eq!(writer.store(&mut disk, json(&prefs), now + Duration::from_secs(5)), None);
        assert_eq!(disk.flushes, 1, "unchanged settings are not written again");

        let next = disk.next_start();
        assert_eq!((next.support_settings.medium.tip.length, next.nav.as_str()), (2.4, "blender"));
        assert!(disk.flushed.contains_key(STORAGE_KEY));
    }

    /// A frame without input is checked only right after one with input (whose clicks the UI
    /// handled in that frame); after that, idle frames cost nothing. Pointer motion doesn't count.
    #[test]
    fn frames_are_checked_with_input_and_once_after_it() {
        let now = Instant::now();
        let mut writer = PrefsWriter::default();
        assert!(!writer.due(false, now));
        assert!(writer.due(true, now));
        assert!(writer.due(false, now), "the frame after input");
        assert!(!writer.due(false, now));

        let click = egui::Event::PointerButton {
            pos: egui::Pos2::ZERO,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        };
        assert!(changes_settings(&click));
        assert!(changes_settings(&egui::Event::Text("5".into())));
        assert!(!changes_settings(&egui::Event::PointerMoved(egui::Pos2::ZERO)));
        assert!(!changes_settings(&egui::Event::MouseMoved(egui::Vec2::X)));
    }

    /// A field dragged for 1.5 s changes the settings in every frame but writes at most once a
    /// second, and its last value is on disk at most a second after the drag ends, with a frame
    /// asked for then so an idle app writes it.
    #[test]
    fn a_continuous_change_is_written_once_a_second_and_its_end_soon_after() {
        let mut prefs = Prefs::default();
        let mut disk = Disk::default();
        let mut writer = PrefsWriter::read(Some(json(&prefs)));
        let start = Instant::now();
        let frame = Duration::from_millis(16);
        let mut asked = None;
        for n in 0..94u32 {
            let now = start + frame * n;
            prefs.support_settings.medium.tip.length = 1.0 + f64::from(n) / 100.0;
            assert!(writer.due(true, now));
            asked = writer.store(&mut disk, json(&prefs), now);
        }
        assert_eq!(disk.flushes, 2, "the first change at once, then one a second later");
        let wait = asked.expect("the last value waits, and a frame is asked for when it can be written");
        assert!(wait <= PrefsWriter::INTERVAL);

        let end = start + frame * 93;
        assert!(writer.due(false, end + frame), "the frame right after input");
        assert!(!writer.due(false, end + frame * 2), "an idle frame before the wait is over");
        let then = end + wait;
        assert!(writer.due(false, then));
        assert_eq!(writer.store(&mut disk, json(&prefs), then), None);
        assert_eq!(disk.flushes, 3);
        assert_eq!(disk.next_start().support_settings.medium.tip.length, 1.0 + 93.0 / 100.0, "the value the drag ended at");
        assert!(!writer.due(false, then + frame), "nothing waits any more");
    }
}
