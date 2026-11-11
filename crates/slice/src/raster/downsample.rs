// Ported from mslicer (GPL-3.0), common/src/container/rle/downsample.rs (`downsample_adjacent`,
// `downsample`, `RunQueue`) at 355aca2; see ATTRIBUTION.md. Changed: an exhausted queue ends the
// averaging instead of spinning, nothing indexes an empty slice, equal neighbours merge.

//! Anti-aliasing: runs rasterized `n` times finer are averaged back to grey pixels.

use super::push;
use crate::Run;

/// Reads runs front to back, a part of one at a time.
struct RunQueue<'a> {
    runs: &'a [Run],
    next: usize,
    active: Run,
}

impl<'a> RunQueue<'a> {
    /// `runs`, or one dark run `fallback` long when there are none.
    fn new(runs: &'a [Run], fallback: u64) -> RunQueue<'a> {
        match runs.first() {
            Some(&first) => RunQueue { runs, next: 1, active: first },
            None => RunQueue { runs, next: 0, active: Run { len: fallback, grey: 0 } },
        }
    }

    fn remaining(&self) -> bool {
        self.active.len > 0
    }

    fn advance(&mut self) -> Run {
        let out = self.active;
        match self.runs.get(self.next) {
            Some(&r) => {
                self.active = r;
                self.next += 1;
            }
            None => self.active.len = 0,
        }
        out
    }

    fn take_up_to(&mut self, n: u64) -> Run {
        if self.active.len <= n {
            self.advance()
        } else {
            self.active.len -= n;
            Run { len: n, grey: self.active.grey }
        }
    }
}

/// One row `factor` times too wide, each `factor` neighbours averaged into one pixel.
pub(crate) fn downsample_adjacent(factor: u64, runs: &[Run], out: &mut Vec<Run>) {
    let mut queue = RunQueue::new(runs, 0);
    let mut i = 0;
    while queue.remaining() {
        let run = queue.advance();
        i += run.len;
        push(out, run.len / factor, run.grey);
        let mut filled = i % factor;
        if filled == 0 {
            continue;
        }
        // Not a clean split: average the pixel the run ends in with what follows.
        let mut sum = filled * u64::from(run.grey);
        while filled < factor {
            let next = queue.take_up_to(factor - filled);
            if next.len == 0 {
                break;
            }
            sum += next.len * u64::from(next.grey);
            filled += next.len;
            i += next.len;
        }
        push(out, 1, (sum / factor) as u8);
    }
}

/// Rows of `width` pixels averaged into one.
pub(crate) fn downsample(rows: &[Vec<Run>], width: u64, out: &mut Vec<Run>) {
    if rows.is_empty() {
        return;
    }
    let mut queues: Vec<RunQueue> = rows.iter().map(|r| RunQueue::new(r, width)).collect();
    let count = queues.len() as u64;
    while queues.iter().all(RunQueue::remaining) {
        let len = queues.iter().map(|q| q.active.len).min().unwrap_or(0);
        let sum: u64 = queues.iter_mut().map(|q| u64::from(q.take_up_to(len).grey)).sum();
        push(out, len, (sum / count) as u8);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(len: u64, grey: u8) -> Run {
        Run { len, grey }
    }

    #[test]
    fn neighbours_and_rows_average_to_grey() {
        let mut out = Vec::new();
        // 3 dark, 5 lit: pixels of 4 are 3/4 dark then all lit.
        downsample_adjacent(4, &[run(3, 0), run(5, 255)], &mut out);
        assert_eq!(out, vec![run(1, 63), run(1, 255)]);
        let mut two = Vec::new();
        downsample(&[vec![run(2, 255)], vec![run(1, 255), run(1, 0)]], 2, &mut two);
        assert_eq!(two, vec![run(1, 255), run(1, 127)]);
        // Short or empty input never spins.
        let mut short = Vec::new();
        downsample_adjacent(4, &[run(2, 255)], &mut short);
        downsample_adjacent(4, &[], &mut short);
        assert_eq!(short, vec![run(1, 127)]);
    }
}
