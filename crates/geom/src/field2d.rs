//! A signed distance field sampled on a square grid in the XY plane: the outline of a raft round
//! the supports' feet, and every outline offset from it.

/// Most nodes a field holds: past it the cells grow, so a raft over the whole plate stays a few
/// tens of megabytes.
pub const MAX_NODES: usize = 4_000_000;
/// Widest span a field covers, mm: past it the input is not a printer's plate.
const MAX_SPAN: f64 = 10_000.0;
/// Most centres taken.
const MAX_CENTERS: usize = 200_000;
/// What stands for "no site" in the distance transform: finite, so differences stay numbers.
const FAR: f64 = 1e20;

/// Values at the nodes of a grid: node `(i, j)` sits at `origin + (i, j) * cell`.
#[derive(Clone, Debug, PartialEq)]
pub struct Field2 {
    pub origin: [f64; 2],
    pub cell: f64,
    pub nx: usize,
    pub ny: usize,
    /// Row-major, `ny` rows of `nx`: negative inside the outline, the distance to it outside.
    pub values: Vec<f64>,
}

impl Field2 {
    /// The value at node `(i, j)`; far outside off the grid.
    pub fn at(&self, i: usize, j: usize) -> f64 {
        if i >= self.nx {
            return FAR;
        }
        self.values.get(j * self.nx + i).copied().unwrap_or(FAR)
    }

    pub fn point(&self, i: usize, j: usize) -> [f64; 2] {
        [self.origin[0] + i as f64 * self.cell, self.origin[1] + j as f64 * self.cell]
    }

    /// The outline round discs of `radius` about `centers`, with every gap narrower than `merge`
    /// filled (a closing by a disc of half of it), sampled every `cell` mm (coarser when the
    /// grid would pass [`MAX_NODES`]), exact from `reach` mm past it to `depth` mm inside it.
    ///
    /// Inside the discs widened by half of `merge` (`D`), the value is half of `merge` less the
    /// distance to the outside of `D`: its zero level is the closing, a level below it the
    /// closing shrunk by that much, exactly, and a level above it a closing by a smaller disc.
    /// Outside `D` it is the distance to the nearest disc. None for no usable centre or size.
    pub fn closed_discs(centers: &[[f64; 2]], radius: f64, merge: f64, reach: f64, depth: f64, cell: f64) -> Option<Field2> {
        let centers: Vec<[f64; 2]> = centers.iter().copied().filter(|c| c[0].is_finite() && c[1].is_finite()).take(MAX_CENTERS).collect();
        let usable = |x: f64| x.is_finite() && x >= 0.0;
        if centers.is_empty()
            || !(radius > 0.0)
            || !usable(radius)
            || !usable(merge)
            || !usable(reach)
            || !usable(depth)
            || !(cell > 0.0)
            || !cell.is_finite()
        {
            return None;
        }
        let half = merge / 2.0;
        // Sampled exactly this far from a centre; past it a node is beyond everything asked for.
        let splat = radius + half.max(reach) + 2.0 * cell;
        let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
        for c in &centers {
            for k in 0..2 {
                lo[k] = lo[k].min(c[k] - splat);
                hi[k] = hi[k].max(c[k] + splat);
            }
        }
        let span = [hi[0] - lo[0], hi[1] - lo[1]];
        if !(span[0] <= MAX_SPAN && span[1] <= MAX_SPAN) {
            return None;
        }
        let nodes = |c: f64| ((span[0] / c).ceil() + 1.0) * ((span[1] / c).ceil() + 1.0);
        let mut cell = cell.max(MAX_SPAN / 1e6);
        while nodes(cell) > MAX_NODES as f64 {
            cell *= 1.25;
        }
        let nx = (span[0] / cell).ceil() as usize + 1;
        let ny = (span[1] / cell).ceil() as usize + 1;
        let mut f = Field2 { origin: lo, cell, nx, ny, values: vec![f64::INFINITY; nx * ny] };

        // The distance to the nearest centre, within `splat` of one.
        let r = (splat / cell).ceil() as isize;
        for c in &centers {
            let ci = ((c[0] - lo[0]) / cell).round() as isize;
            let cj = ((c[1] - lo[1]) / cell).round() as isize;
            for j in (cj - r).max(0)..=(cj + r).min(ny as isize - 1) {
                let y = lo[1] + j as f64 * cell - c[1];
                let row = j as usize * nx;
                for i in (ci - r).max(0)..=(ci + r).min(nx as isize - 1) {
                    let x = lo[0] + i as f64 * cell - c[0];
                    if let Some(v) = f.values.get_mut(row + i as usize) {
                        *v = v.min((x * x + y * y).sqrt());
                    }
                }
            }
        }
        if half <= 0.0 {
            for v in &mut f.values {
                *v = (*v - radius).min(FAR);
            }
            return Some(f);
        }
        let outer = radius + half;
        let outside: Vec<bool> = f.values.iter().map(|d| *d > outer).collect();
        let nearest = nearest_sites(&outside, nx, ny);
        let centre = f.values.clone();
        let index = CenterIndex::new(&centers, outer);
        // Neighbouring nodes mostly share their nearest outside node, and with it the outline.
        let mut outlines: std::collections::HashMap<usize, Outline> = std::collections::HashMap::new();
        for (k, v) in f.values.iter_mut().enumerate() {
            let d = centre.get(k).copied().unwrap_or(f64::INFINITY);
            if d > outer {
                *v = (d - radius).min(FAR);
                continue;
            }
            // The nearest node outside `D`, less how far past `D`'s edge that node lies: a node
            // apart from the edge, it would leave a step on the outline.
            let g = match nearest.get(k).copied().flatten() {
                Some(q) => {
                    let (pi, pj) = ((k % nx) as f64, (k / nx) as f64);
                    let (qi, qj) = ((q % nx) as f64, (q / nx) as f64);
                    let past = centre.get(q).map_or(0.0, |dq| (dq - outer).clamp(0.0, 1.5 * cell));
                    let rough = (((pi - qi).powi(2) + (pj - qj).powi(2)).sqrt() * cell - past).max(0.0);
                    // Exact where levels are drawn: the rough value is off by up to a cell, which
                    // shows as a wavy outline.
                    if rough >= half - reach - 2.0 * cell && rough <= half + depth + 2.0 * cell {
                        let at = |i: f64, j: f64| [lo[0] + i * cell, lo[1] + j * cell];
                        let outline = outlines.entry(q).or_insert_with(|| index.near_outline(at(qi, qj), 3.0 * cell));
                        index.boundary_distance(at(pi, pj), outline).unwrap_or(rough)
                    } else {
                        rough
                    }
                }
                None => FAR,
            };
            *v = half - g;
        }
        Some(f)
    }
}

/// Discs of one radius, their centres in a grid of buckets a quarter of the radius wide.
struct CenterIndex<'a> {
    centers: &'a [[f64; 2]],
    radius: f64,
    origin: [f64; 2],
    size: f64,
    dims: [usize; 2],
    /// Bucket `b` holds `members[start[b]..start[b + 1]]`.
    start: Vec<u32>,
    members: Vec<u32>,
}

/// Most circles taken round one point: the discs within a few cells of it.
const MAX_CANDIDATES: usize = 24;
/// Most buckets: past it they grow.
const MAX_BUCKETS: f64 = 1e6;

impl<'a> CenterIndex<'a> {
    fn new(centers: &'a [[f64; 2]], radius: f64) -> CenterIndex<'a> {
        let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
        for c in centers {
            for k in 0..2 {
                lo[k] = lo[k].min(c[k]);
                hi[k] = hi[k].max(c[k]);
            }
        }
        let span = [(hi[0] - lo[0]).max(0.0), (hi[1] - lo[1]).max(0.0)];
        let mut size = (radius / 4.0).max(1e-3);
        while (span[0] / size + 1.0) * (span[1] / size + 1.0) > MAX_BUCKETS {
            size *= 2.0;
        }
        let dims = [(span[0] / size) as usize + 1, (span[1] / size) as usize + 1];
        let origin = if lo[0].is_finite() { lo } else { [0.0; 2] };
        let mut index = CenterIndex { centers, radius, origin, size, dims, start: vec![0; dims[0] * dims[1] + 1], members: vec![0; centers.len()] };
        let buckets: Vec<Option<usize>> = centers.iter().map(|c| index.bucket(*c).and_then(|b| index.slot(b))).collect();
        for b in buckets.iter().flatten() {
            if let Some(s) = index.start.get_mut(b + 1) {
                *s += 1;
            }
        }
        for k in 1..index.start.len() {
            index.start[k] += index.start[k - 1];
        }
        let mut fill = index.start.clone();
        for (i, b) in buckets.iter().enumerate() {
            if let Some(at) = b.and_then(|b| fill.get_mut(b))
                && let Some(m) = index.members.get_mut(*at as usize)
            {
                *m = i as u32;
                *at += 1;
            }
        }
        index
    }

    /// The bucket `p` falls in, in bucket coordinates, possibly off the grid.
    fn bucket(&self, p: [f64; 2]) -> Option<[i64; 2]> {
        let b = [((p[0] - self.origin[0]) / self.size).floor(), ((p[1] - self.origin[1]) / self.size).floor()];
        (b[0].is_finite() && b[1].is_finite()).then_some([b[0] as i64, b[1] as i64])
    }

    fn slot(&self, b: [i64; 2]) -> Option<usize> {
        let (x, y) = (usize::try_from(b[0]).ok()?, usize::try_from(b[1]).ok()?);
        (x < self.dims[0] && y < self.dims[1]).then_some(y * self.dims[0] + x)
    }

    /// The centres in bucket `b`, none off the grid.
    fn in_bucket(&self, b: [i64; 2]) -> &[u32] {
        self.slot(b)
            .and_then(|s| {
                let (a, z) = (*self.start.get(s)? as usize, *self.start.get(s + 1)? as usize);
                self.members.get(a..z)
            })
            .unwrap_or_default()
    }

    /// The centres within `reach` of `p`, into `out`.
    fn near(&self, p: [f64; 2], reach: f64, out: &mut Vec<usize>) {
        out.clear();
        let (Some(a), Some(b)) = (self.bucket([p[0] - reach, p[1] - reach]), self.bucket([p[0] + reach, p[1] + reach])) else { return };
        for by in a[1].max(0)..=b[1].min(self.dims[1] as i64 - 1) {
            for bx in a[0].max(0)..=b[0].min(self.dims[0] as i64 - 1) {
                let found = self.in_bucket([bx, by]).iter().map(|&i| i as usize);
                out.extend(found.filter(|&i| self.centers.get(i).is_some_and(|c| dist(*c, p) <= reach)));
            }
        }
    }

    /// Whether `x` lies inside a disc other than `skip`'s: the buckets searched ring by ring
    /// outwards, until one is found or the rings are past the radius.
    fn covered(&self, x: [f64; 2], skip: [usize; 2]) -> bool {
        let Some(home) = self.bucket(x) else { return false };
        let inner = self.radius * (1.0 - 1e-9);
        let mut ring = 0i64;
        loop {
            for by in home[1] - ring..=home[1] + ring {
                let edge = by == home[1] - ring || by == home[1] + ring;
                let step = if edge || ring == 0 { 1 } else { 2 * ring };
                let mut bx = home[0] - ring;
                while bx <= home[0] + ring {
                    let hit = self.in_bucket([bx, by]).iter().any(|&i| {
                        let i = i as usize;
                        !skip.contains(&i) && self.centers.get(i).is_some_and(|c| dist(*c, x) < inner)
                    });
                    if hit {
                        return true;
                    }
                    bx += step;
                }
            }
            // Past this ring every centre lies at least `ring` buckets away.
            if ring as f64 * self.size >= inner || ring > (self.dims[0].max(self.dims[1]) as i64) + 1 {
                return false;
            }
            ring += 1;
        }
    }

    /// The circles passing within `around` of `q`, a point outside them near their outline (the
    /// nearest first, at most [`MAX_CANDIDATES`]), and the corners where two of them cross that
    /// no other disc covers.
    fn near_outline(&self, q: [f64; 2], around: f64) -> Outline {
        let mut circles = Vec::new();
        self.near(q, self.radius + around, &mut circles);
        let from_q = |i: &usize| self.centers.get(*i).map_or(f64::INFINITY, |c| dist(*c, q));
        circles.sort_by(|a, b| from_q(a).total_cmp(&from_q(b)));
        circles.truncate(MAX_CANDIDATES);
        let r = self.radius;
        let mut corners = Vec::new();
        for (n, &i) in circles.iter().enumerate() {
            let Some(&c) = self.centers.get(i) else { continue };
            for &j in circles.iter().skip(n + 1) {
                let Some(&e) = self.centers.get(j) else { continue };
                let dd = dist(c, e);
                if !(dd > 1e-12 && dd < 2.0 * r) {
                    continue;
                }
                let h = (r * r - dd * dd / 4.0).max(0.0).sqrt();
                let m = [(c[0] + e[0]) / 2.0, (c[1] + e[1]) / 2.0];
                for s in [-1.0, 1.0] {
                    let x = [m[0] - s * h * (e[1] - c[1]) / dd, m[1] + s * h * (e[0] - c[0]) / dd];
                    if !self.covered(x, [i, j]) {
                        corners.push(x);
                    }
                }
            }
        }
        Outline { circles, corners }
    }

    /// The distance from `p` to the outline of the union of the discs, near `outline`: the
    /// nearest point of each of its circles that no other disc covers, and its corners.
    fn boundary_distance(&self, p: [f64; 2], outline: &Outline) -> Option<f64> {
        let r = self.radius;
        let mut best = outline.corners.iter().map(|x| dist(*x, p)).fold(f64::INFINITY, f64::min);
        for &i in &outline.circles {
            let Some(&c) = self.centers.get(i) else { continue };
            let d = dist(c, p);
            // Nothing on this circle comes nearer than the best already found.
            if d > 1e-12 && (r - d).abs() < best {
                let x = [c[0] + (p[0] - c[0]) * r / d, c[1] + (p[1] - c[1]) * r / d];
                if !self.covered(x, [i, i]) {
                    best = best.min(dist(x, p));
                }
            }
        }
        best.is_finite().then_some(best)
    }
}

/// The piece of the discs' outline round a point outside them: see [`CenterIndex::near_outline`].
struct Outline {
    circles: Vec<usize>,
    corners: Vec<[f64; 2]>,
}

fn dist(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

/// For every node of an `nx` by `ny` grid, the index of the nearest node where `site` holds, by
/// straight-line distance: Felzenszwalb and Huttenlocher's exact distance transform, a column
/// pass then a row pass, carrying the site found.
fn nearest_sites(site: &[bool], nx: usize, ny: usize) -> Vec<Option<usize>> {
    let n = nx * ny;
    // Down each column: the squared distance in rows to the nearest site, and its row.
    let mut col_d = vec![FAR; n];
    let mut col_arg = vec![usize::MAX; n];
    let mut env = Envelope::default();
    let mut f = vec![FAR; ny];
    let mut d = vec![FAR; ny];
    let mut arg = vec![usize::MAX; ny];
    for i in 0..nx {
        for (j, fj) in f.iter_mut().enumerate() {
            *fj = if site.get(j * nx + i).copied().unwrap_or(false) { 0.0 } else { FAR };
        }
        env.run(&f, &mut d, &mut arg);
        for j in 0..ny {
            if let (Some(cd), Some(ca)) = (col_d.get_mut(j * nx + i), col_arg.get_mut(j * nx + i)) {
                *cd = d.get(j).copied().unwrap_or(FAR);
                *ca = arg.get(j).copied().unwrap_or(usize::MAX);
            }
        }
    }
    // Along each row, over the columns' distances.
    let mut out = vec![None; n];
    let mut d = vec![FAR; nx];
    let mut arg = vec![usize::MAX; nx];
    for j in 0..ny {
        let Some(row) = col_d.get(j * nx..(j + 1) * nx) else { continue };
        env.run(row, &mut d, &mut arg);
        for i in 0..nx {
            let Some(&x) = arg.get(i) else { continue };
            let Some(&y) = col_arg.get(j * nx + x) else { continue };
            if x < nx
                && y < ny
                && let Some(o) = out.get_mut(j * nx + i)
            {
                *o = Some(y * nx + x);
            }
        }
    }
    out
}

/// The lower envelope of the parabolas `(x - q)² + f(q)`: for each `x`, its minimum and the `q`
/// it comes from. Entries of `f` at [`FAR`] are no parabola.
#[derive(Default)]
struct Envelope {
    v: Vec<usize>,
    z: Vec<f64>,
}

impl Envelope {
    fn run(&mut self, f: &[f64], d: &mut [f64], arg: &mut [usize]) {
        self.v.clear();
        self.z.clear();
        let key = |q: usize, fq: f64| fq + (q * q) as f64;
        for (q, &fq) in f.iter().enumerate() {
            if fq >= FAR {
                continue;
            }
            let mut s = f64::NEG_INFINITY;
            while let (Some(&p), Some(&zp)) = (self.v.last(), self.z.last()) {
                let fp = f.get(p).copied().unwrap_or(FAR);
                s = (key(q, fq) - key(p, fp)) / (2.0 * (q as f64 - p as f64));
                if s <= zp {
                    self.v.pop();
                    self.z.pop();
                    s = f64::NEG_INFINITY;
                } else {
                    break;
                }
            }
            self.v.push(q);
            self.z.push(s);
        }
        let mut k = 0;
        for (x, (dx, ax)) in d.iter_mut().zip(arg.iter_mut()).enumerate() {
            while self.z.get(k + 1).is_some_and(|z| *z < x as f64) {
                k += 1;
            }
            match self.v.get(k) {
                Some(&q) => {
                    let dq = x as f64 - q as f64;
                    *dx = dq * dq + f.get(q).copied().unwrap_or(FAR);
                    *ax = q;
                }
                None => {
                    *dx = FAR;
                    *ax = usize::MAX;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The value at `(x, y)`, from the nearest node.
    fn sample(f: &Field2, x: f64, y: f64) -> f64 {
        let i = ((x - f.origin[0]) / f.cell).round() as usize;
        let j = ((y - f.origin[1]) / f.cell).round() as usize;
        f.at(i, j)
    }

    #[test]
    fn a_lone_disc_is_its_exact_distance() {
        let f = Field2::closed_discs(&[[0.0, 0.0]], 2.0, 4.0, 1.0, 3.0, 0.1).unwrap();
        for (x, y, want) in [(0.0, 0.0, -2.0), (2.0, 0.0, 0.0), (0.0, 3.0, 1.0), (-2.5, 0.0, 0.5), (1.0, 0.0, -1.0)] {
            let got = sample(&f, x, y);
            assert!((got - want).abs() < 0.08, "({x}, {y}): {got} not {want}");
        }
    }

    #[test]
    fn discs_closer_than_the_merge_distance_join_and_farther_ones_stay_apart() {
        // Radius 1, 3 mm between centres: a 1 mm gap.
        let pair = |merge: f64| Field2::closed_discs(&[[0.0, 0.0], [3.0, 0.0]], 1.0, merge, 0.0, 3.0, 0.05).unwrap();
        assert!(sample(&pair(2.0), 1.5, 0.0) < 0.0, "a 1 mm gap fills under a 2 mm merge distance");
        assert!(sample(&pair(0.5), 1.5, 0.0) > 0.0, "and stays open under 0.5 mm");
        assert!(sample(&pair(0.0), 1.5, 0.0) > 0.0);
        // The bridge is a closing: no wider than the discs.
        assert!(sample(&pair(2.0), 1.5, 1.2) > 0.0);
    }

    #[test]
    fn levels_below_zero_shrink_the_closing_exactly() {
        let f = Field2::closed_discs(&[[0.0, 0.0]], 3.0, 2.0, 0.0, 3.0, 0.05).unwrap();
        for x in [0.5, 1.0, 2.0, 2.9] {
            let got = sample(&f, x, 0.0);
            assert!((got - (x - 3.0)).abs() < 0.03, "{x}: {got}");
        }
    }

    #[test]
    fn hostile_input_gives_none_or_a_bounded_grid() {
        assert!(Field2::closed_discs(&[], 1.0, 0.0, 0.0, 3.0, 0.1).is_none());
        assert!(Field2::closed_discs(&[[f64::NAN, 0.0]], 1.0, 0.0, 0.0, 3.0, 0.1).is_none());
        assert!(Field2::closed_discs(&[[0.0, 0.0]], f64::NAN, 0.0, 0.0, 3.0, 0.1).is_none());
        assert!(Field2::closed_discs(&[[0.0, 0.0]], 1.0, -1.0, 0.0, 3.0, 0.1).is_none());
        assert!(Field2::closed_discs(&[[0.0, 0.0]], 1.0, 0.0, 0.0, 3.0, 0.0).is_none());
        assert!(Field2::closed_discs(&[[0.0, 0.0], [1e9, 0.0]], 1.0, 0.0, 0.0, 3.0, 0.1).is_none());
        let wide = Field2::closed_discs(&[[0.0, 0.0], [2000.0, 2000.0]], 1.0, 1.0, 0.0, 3.0, 0.01).unwrap();
        assert!(wide.nx * wide.ny <= MAX_NODES && wide.values.len() == wide.nx * wide.ny);
    }
}
#[cfg(test)]
mod exact_tests {
    use super::*;

    /// Exact distance from p to the outside of the union of discs of radius `r`.
    fn exact_g(p: [f64; 2], cs: &[[f64; 2]], r: f64) -> f64 {
        let inside =
            |x: [f64; 2], skip: &[usize]| cs.iter().enumerate().any(|(k, c)| !skip.contains(&k) && (x[0] - c[0]).hypot(x[1] - c[1]) < r - 1e-9);
        let mut best = f64::INFINITY;
        for (i, c) in cs.iter().enumerate() {
            let d = (p[0] - c[0]).hypot(p[1] - c[1]);
            if d > 1e-12 {
                let x = [c[0] + (p[0] - c[0]) * r / d, c[1] + (p[1] - c[1]) * r / d];
                if !inside(x, &[i]) {
                    best = best.min((x[0] - p[0]).hypot(x[1] - p[1]));
                }
            }
            for (j, e) in cs.iter().enumerate().skip(i + 1) {
                let dd = (e[0] - c[0]).hypot(e[1] - c[1]);
                if dd >= 2.0 * r || dd < 1e-12 {
                    continue;
                }
                let a = dd / 2.0;
                let h = (r * r - a * a).sqrt();
                let m = [c[0] + (e[0] - c[0]) * a / dd, c[1] + (e[1] - c[1]) * a / dd];
                for s in [-1.0, 1.0] {
                    let x = [m[0] - s * h * (e[1] - c[1]) / dd, m[1] + s * h * (e[0] - c[0]) / dd];
                    if !inside(x, &[i, j]) {
                        best = best.min((x[0] - p[0]).hypot(x[1] - p[1]));
                    }
                }
            }
        }
        best
    }
    #[test]
    fn the_closing_is_exact_where_its_levels_are_drawn() {
        let mut cs = Vec::new();
        let mut s = 12345u64;
        let mut rnd = || {
            s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (s >> 11) as f64 / (1u64 << 53) as f64
        };
        for _ in 0..30 {
            cs.push([rnd() * 20.0, rnd() * 12.0]);
        }
        let (radius, merge) = (1.5 + 0.68, 1.0);
        let f = Field2::closed_discs(&cs, radius, merge, 0.5, 3.0, 0.25).unwrap();
        let half = merge / 2.0;
        let mut worst: f64 = 0.0;
        for j in 0..f.ny {
            for i in 0..f.nx {
                let v = f.at(i, j);
                if v.abs() > 0.8 {
                    continue;
                }
                let p = f.point(i, j);
                let dc = cs.iter().map(|c| (p[0] - c[0]).hypot(p[1] - c[1])).fold(f64::INFINITY, f64::min);
                if dc > radius + half {
                    continue;
                }
                let want = half - exact_g(p, &cs, radius + half);
                worst = worst.max((v - want).abs());
            }
        }
        assert!(worst < 1e-9, "{worst} mm off");
    }
}
