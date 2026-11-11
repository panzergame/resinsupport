//! Footprints arranged on the build plate. Each new footprint, the largest first, takes the free
//! spot nearest the plate's centre, its long side along the plate's if it may turn, else turned
//! a quarter if that is the only way it fits. Spots are the positions where it touches the
//! plate's edge or another footprint on each axis: when it fits anywhere, it fits at one of them
//! (the lowest, then leftmost, position that fits touches something on both axes). Footprints
//! that fit nowhere go in a column right of the plate.

/// A rectangle on the plate, millimetres.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub min: [f64; 2],
    pub max: [f64; 2],
}

impl Rect {
    /// A `width` by `depth` rectangle centred on the origin: a build plate.
    pub fn centred(width: f64, depth: f64) -> Rect {
        Rect::around([0.0, 0.0], [width, depth])
    }
    pub fn around(centre: [f64; 2], size: [f64; 2]) -> Rect {
        Rect { min: [centre[0] - size[0] / 2.0, centre[1] - size[1] / 2.0], max: [centre[0] + size[0] / 2.0, centre[1] + size[1] / 2.0] }
    }
    pub fn size(&self) -> [f64; 2] {
        [self.max[0] - self.min[0], self.max[1] - self.min[1]]
    }
    pub fn centre(&self) -> [f64; 2] {
        [(self.min[0] + self.max[0]) / 2.0, (self.min[1] + self.max[1]) / 2.0]
    }
    fn grown(&self, d: f64) -> Rect {
        Rect { min: [self.min[0] - d, self.min[1] - d], max: [self.max[0] + d, self.max[1] + d] }
    }
    /// Whether the insides meet: rectangles that only touch do not.
    fn overlaps(&self, o: &Rect) -> bool {
        self.min[0] < o.max[0] && o.min[0] < self.max[0] && self.min[1] < o.max[1] && o.min[1] < self.max[1]
    }
}

/// The gap kept between footprints, and between a footprint and the plate's edge, mm.
pub const ARRANGE_MARGIN: f64 = 1.0;

/// How far right of the plate the column of footprints that fit nowhere starts, mm.
const COLUMN_GAP: f64 = 10.0;

/// Positions tried per axis at most, the nearest the plate's centre kept: past a few hundred
/// footprints on one plate, packing gives way to staying quick.
const MAX_POSITIONS: usize = 256;

/// Rounding in a touching position must not read as an overlap.
const TOUCH: f64 = 1e-9;

/// A footprint to arrange: its size (width, depth) as it lies, and whether it may turn a quarter.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Item {
    pub size: [f64; 2],
    pub turnable: bool,
}

/// Where an [`Item`] goes: its centre, whether it turns a quarter, whether that is on the plate
/// (`false`: in the column beside it).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spot {
    pub centre: [f64; 2],
    pub turned: bool,
    pub on_plate: bool,
}

/// Where each item goes on `plate`, clear of `fixed` and of each other. Sizes that are not
/// finite find no spot on the plate.
pub fn arrange(plate: Rect, fixed: &[Rect], items: &[Item]) -> Vec<Spot> {
    let along_x = plate.size()[0] >= plate.size()[1];
    // Far from the plate, a footprint cannot be in the way of one on it.
    let mut taken: Vec<Rect> = fixed.iter().filter(|f| f.overlaps(&plate.grown(ARRANGE_MARGIN))).copied().collect();
    let mut order: Vec<usize> = (0..items.len()).collect();
    let area = |k: usize| items[k].size[0] * items[k].size[1];
    order.sort_by(|&a, &b| area(b).total_cmp(&area(a)).then(a.cmp(&b)));

    let mut spots = vec![Spot { centre: [0.0, 0.0], turned: false, on_plate: false }; items.len()];
    let mut beside = Vec::new();
    for k in order {
        let Item { size, turnable } = items[k];
        let can_turn = turnable && size[0] != size[1];
        let lengthwise = (size[0] >= size[1]) == along_x;
        let ways: &[bool] = match (can_turn, lengthwise) {
            (false, _) => &[false],
            (true, true) => &[false, true],
            (true, false) => &[true, false],
        };
        let found = ways.iter().find_map(|&turned| {
            let s = if turned { [size[1], size[0]] } else { size };
            free_spot(&plate, &taken, s).map(|c| (c, s, turned))
        });
        match found {
            Some((centre, s, turned)) => {
                taken.push(Rect::around(centre, s));
                spots[k] = Spot { centre, turned, on_plate: true };
            }
            None => beside.push((k, ways.first().copied().unwrap_or(false))),
        }
    }

    // The rest, in order: a column right of the plate, going down from its middle, clear of
    // whatever is there already.
    beside.sort_unstable();
    let mut column: Vec<Rect> = fixed.iter().filter(|f| f.max[0] > plate.max[0]).copied().collect();
    let mut top = plate.centre()[1] + ARRANGE_MARGIN;
    for (k, turned) in beside {
        let s = items[k].size.map(|v| if v.is_finite() { v } else { 0.0 });
        let s = if turned { [s[1], s[0]] } else { s };
        let x = plate.max[0] + COLUMN_GAP + s[0] / 2.0;
        let mut r = Rect::around([x, top - s[1] / 2.0], s);
        // Below each footprint in the way; each is passed once, so this ends.
        for _ in 0..=column.len() {
            let clear = r.grown(ARRANGE_MARGIN - TOUCH);
            let Some(lowest) = column.iter().filter(|o| o.overlaps(&clear)).map(|o| o.min[1]).reduce(f64::min) else {
                break;
            };
            r = Rect::around([x, lowest - ARRANGE_MARGIN - s[1] / 2.0], s);
        }
        column.push(r);
        top = r.min[1] - ARRANGE_MARGIN;
        spots[k] = Spot { centre: r.centre(), turned, on_plate: false };
    }
    spots
}

/// The free centre nearest the plate's centre for a footprint of `size`, inside the plate with
/// its margin and clear of `taken` by the margin.
fn free_spot(plate: &Rect, taken: &[Rect], size: [f64; 2]) -> Option<[f64; 2]> {
    let centre = plate.centre();
    let axis = |a: usize| -> Option<Vec<f64>> {
        let half = size[a] / 2.0;
        let (lo, hi) = (plate.min[a] + ARRANGE_MARGIN + half, plate.max[a] - ARRANGE_MARGIN - half);
        if !(lo <= hi) || !centre[a].is_finite() {
            return None;
        }
        let touching = taken.iter().flat_map(|o| [o.max[a] + ARRANGE_MARGIN + half, o.min[a] - ARRANGE_MARGIN - half]);
        let mut v: Vec<f64> = [centre[a].clamp(lo, hi), lo, hi].into_iter().chain(touching.filter(|v| (lo..=hi).contains(v))).collect();
        v.sort_by(|p, q| (p - centre[a]).abs().total_cmp(&(q - centre[a]).abs()).then(p.total_cmp(q)));
        v.dedup();
        v.truncate(MAX_POSITIONS);
        Some(v)
    };
    let (xs, ys) = (axis(0)?, axis(1)?);
    let mut candidates: Vec<[f64; 2]> = xs.iter().flat_map(|&x| ys.iter().map(move |&y| [x, y])).collect();
    let dist = |c: &[f64; 2]| (c[0] - centre[0]).powi(2) + (c[1] - centre[1]).powi(2);
    candidates.sort_by(|a, b| dist(a).total_cmp(&dist(b)).then(a[1].total_cmp(&b[1])).then(a[0].total_cmp(&b[0])));
    candidates.into_iter().find(|&c| {
        let r = Rect::around(c, size).grown(ARRANGE_MARGIN - TOUCH);
        !taken.iter().any(|o| o.overlaps(&r))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SATURN: (f64, f64) = (218.88, 123.12);

    fn item(w: f64, d: f64) -> Item {
        Item { size: [w, d], turnable: true }
    }

    fn footprints(items: &[Item], spots: &[Spot]) -> Vec<Rect> {
        items.iter().zip(spots).map(|(i, s)| Rect::around(s.centre, if s.turned { [i.size[1], i.size[0]] } else { i.size })).collect()
    }

    fn inside(r: &Rect, plate: &Rect) -> bool {
        let p = plate.grown(-ARRANGE_MARGIN + 1e-6);
        r.min[0] >= p.min[0] && r.min[1] >= p.min[1] && r.max[0] <= p.max[0] && r.max[1] <= p.max[1]
    }

    fn apart(a: &Rect, b: &Rect) -> bool {
        !a.grown(ARRANGE_MARGIN - 1e-6).overlaps(b)
    }

    #[test]
    fn one_footprint_on_an_empty_plate_lands_on_its_centre_lengthwise() {
        let plate = Rect::centred(SATURN.0, SATURN.1);
        assert_eq!(arrange(plate, &[], &[item(37.0, 12.0)]), vec![Spot { centre: [0.0, 0.0], turned: false, on_plate: true }]);
        assert!(arrange(plate, &[], &[item(12.0, 37.0)])[0].turned, "turned to lie along the plate");
        assert!(!arrange(plate, &[], &[Item { size: [12.0, 37.0], turnable: false }])[0].turned);
        assert!(arrange(Rect::centred(80.0, 200.0), &[], &[item(37.0, 12.0)])[0].turned, "a plate deeper than wide");
    }

    /// Two 90 × 47 mm models, one after the other: the second fits only turned, beside the first.
    #[test]
    fn a_model_that_fits_only_turned_is_turned() {
        let plate = Rect::centred(SATURN.0, SATURN.1);
        let first = arrange(plate, &[], &[item(90.0, 47.0)])[0];
        assert!(first.on_plate && !first.turned);
        let fixed = [Rect::around(first.centre, [90.0, 47.0])];
        let second = arrange(plate, &fixed, &[item(90.0, 47.0)])[0];
        assert!(second.on_plate && second.turned, "{second:?}");
        let r = Rect::around(second.centre, [47.0, 90.0]);
        assert!(inside(&r, &plate) && apart(&r, &fixed[0]), "{r:?}");
        // Unable to turn, it has nowhere to go.
        assert!(!arrange(plate, &fixed, &[Item { size: [90.0, 47.0], turnable: false }])[0].on_plate);
    }

    #[test]
    fn footprints_fill_the_plate_apart_and_clear_of_fixed_ones() {
        let plate = Rect::centred(SATURN.0, SATURN.1);
        let fixed = [Rect::around([0.0, 0.0], [30.0, 30.0]), Rect::around([500.0, 0.0], [30.0, 30.0])];
        let items: Vec<Item> = (0..20).map(|k| item(15.0 + f64::from(k % 4) * 5.0, 12.0 + f64::from(k % 3) * 4.0)).collect();
        let spots = arrange(plate, &fixed, &items);
        let boxes = footprints(&items, &spots);
        for (k, b) in boxes.iter().enumerate() {
            assert!(spots[k].on_plate && inside(b, &plate), "{b:?} on the plate");
            assert!(fixed.iter().all(|f| apart(b, f)), "{b:?} clear of the fixed ones");
            assert!(boxes[..k].iter().all(|o| apart(o, b)), "{b:?} apart");
        }
    }

    #[test]
    fn the_largest_goes_nearest_the_centre() {
        let plate = Rect::centred(SATURN.0, SATURN.1);
        let spots = arrange(plate, &[], &[item(10.0, 10.0), item(50.0, 30.0)]);
        assert_eq!(spots[1].centre, [0.0, 0.0]);
        assert!(spots[0].on_plate && spots[0].centre != [0.0, 0.0]);
    }

    #[test]
    fn what_does_not_fit_goes_beside_the_plate_and_leaves_room_for_the_rest() {
        let plate = Rect::centred(100.0, 60.0);
        let items = [item(150.0, 20.0), item(10.0, 10.0), item(150.0, 20.0)];
        let spots = arrange(plate, &[], &items);
        assert_eq!(spots[1], Spot { centre: [0.0, 0.0], turned: false, on_plate: true });
        let boxes = footprints(&items, &spots);
        for k in [0, 2] {
            assert!(!spots[k].on_plate && boxes[k].min[0] > plate.max[0], "right of the plate: {spots:?}");
        }
        assert!(apart(&boxes[0], &boxes[2]) && boxes[2].max[1] < boxes[0].min[1], "going down: {boxes:?}");
        // The next one steps past those already beside the plate.
        let next = arrange(plate, &boxes, &[item(150.0, 20.0)])[0];
        let r = Rect::around(next.centre, [150.0, 20.0]);
        assert!(!next.on_plate && apart(&r, &boxes[0]) && apart(&r, &boxes[2]), "{r:?}");
    }

    #[test]
    fn many_fixed_footprints_and_hostile_sizes_stay_quick_and_never_panic() {
        let plate = Rect::centred(1000.0, 1000.0);
        let mut fixed: Vec<Rect> =
            (0..2000).map(|k| Rect::around([f64::from(k % 45) * 22.0 - 490.0, f64::from(k / 45) * 22.0 - 490.0], [20.0, 20.0])).collect();
        fixed.push(Rect::around([-1.0e6, 0.0], [10.0, 2.0e6]));
        let items = [item(10.0, 10.0), item(f64::NAN, 1.0), item(f64::INFINITY, 0.0), item(1e300, 1e300), item(-5.0, 3.0)];
        let started = std::time::Instant::now();
        let spots = arrange(plate, &fixed, &items);
        assert!(started.elapsed().as_secs_f64() < 5.0, "{:?}", started.elapsed());
        assert_eq!(spots.len(), items.len());
        assert!(spots[1..4].iter().all(|s| !s.on_plate && s.centre.iter().all(|v| v.is_finite())), "{spots:?}");
        assert!(arrange(Rect::centred(f64::NAN, 1.0), &fixed, &[item(1.0, 1.0)]).iter().all(|s| !s.on_plate));
        assert!(arrange(plate, &fixed, &[]).is_empty());
    }
}
