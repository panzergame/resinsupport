// Ported from mslicer (GPL-3.0), slicer/src/slicer/raster/edge_table.rs at 355aca2; see
// ATTRIBUTION.md. Changed: an edge carries its depth change instead of mslicer's `entering`
// flag, exposure and priority; sorting uses `total_cmp`.

//! The global edge table and the active edge list of a scanline fill
//! (https://www.cs.rit.edu/~icss571/filling/how_to.html).

use std::collections::VecDeque;

use super::Segment;

pub(crate) struct Edge {
    p_min: [f32; 2],
    p_max: [f32; 2],
    inv_slope: f32,
    delta: i8,
}

impl Edge {
    pub(crate) fn p_min_y(&self) -> f32 {
        self.p_min[1]
    }
}

pub(crate) struct ActiveEdge {
    /// Where the edge crosses the current row's centre.
    pub(crate) x: f32,
    y_max: f32,
    inv_slope: f32,
    pub(crate) delta: i8,
}

/// Every segment as an edge, cut where it crosses X = 0 or Y = 0 and held to the plate's side of
/// them, sorted by its lowest end.
pub(crate) fn global_edge_table(segments: &[Segment]) -> VecDeque<Edge> {
    let mut edges = Vec::with_capacity(segments.len());
    for s in segments {
        let (p0, d) = (s.a, [s.b[0] - s.a[0], s.b[1] - s.a[1]]);
        let mut ts = [0.0, 1.0, 0.0, 0.0];
        let mut n = 2;
        for (o, dd) in [(p0[0], d[0]), (p0[1], d[1])] {
            if dd != 0.0 {
                let t = -o / dd;
                if (0.0..=1.0).contains(&t)
                    && let Some(slot) = ts.get_mut(n)
                {
                    *slot = t;
                    n += 1;
                }
            }
        }
        let ts = ts.get_mut(..n).unwrap_or_default();
        ts.sort_by(f32::total_cmp);
        for w in ts.windows(2) {
            let [t0, t1] = [w.first().copied().unwrap_or(0.0), w.get(1).copied().unwrap_or(0.0)];
            // The ends exactly as given: a neighbour's segment shares them to the bit.
            let at = |t: f32| {
                let p = if t == 0.0 {
                    s.a
                } else if t == 1.0 {
                    s.b
                } else {
                    [p0[0] + d[0] * t, p0[1] + d[1] * t]
                };
                [p[0].max(0.0), p[1].max(0.0)]
            };
            let (q0, q1) = (at(t0), at(t1));
            if q0[1] == q1[1] {
                continue;
            }
            let inv_slope = (q1[0] - q0[0]) / (q1[1] - q0[1]);
            let (p_min, p_max) = if q0[1] < q1[1] { (q0, q1) } else { (q1, q0) };
            edges.push(Edge { p_min, p_max, inv_slope, delta: s.delta });
        }
    }
    edges.sort_by(|a, b| a.p_min[1].total_cmp(&b.p_min[1]).then_with(|| a.p_min[0].total_cmp(&b.p_min[0])));
    VecDeque::from(edges)
}

/// Bring `active` to row `y`: edges ending above its centre go, the others step along, the
/// edges starting at or before it come in, all sorted by X.
pub(crate) fn update_active_edges(edges: &mut VecDeque<Edge>, active: &mut Vec<ActiveEdge>, y: u64) {
    let scan_y = y as f32 + 0.5;
    // Half-open: an edge spans p_min.y <= scan_y < p_max.y.
    active.retain(|e| e.y_max > scan_y);
    for e in active.iter_mut() {
        e.x += e.inv_slope;
    }
    while edges.front().is_some_and(|e| e.p_min[1] <= scan_y) {
        let Some(e) = edges.pop_front() else { break };
        // It ends before this row's centre: no row samples it.
        if e.p_max[1] <= scan_y {
            continue;
        }
        active.push(ActiveEdge { x: e.p_min[0] + (scan_y - e.p_min[1]) * e.inv_slope, y_max: e.p_max[1], inv_slope: e.inv_slope, delta: e.delta });
    }
    active.sort_by(|a, b| a.x.total_cmp(&b.x));
}
