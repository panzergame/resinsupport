//! Text as outlines in the built-in font (Liberation Sans, SIL OFL 1.1; see
//! `assets/fonts/liberation-sans/OFL.txt` and `ATTRIBUTION.md`): each glyph's contours, its curves
//! flattened to lines, to be filled by the nonzero rule.
//!
//! Ported from SolidCraft `crates/sketch/src/text.rs` (`Outline`, the line layout of
//! `TextMetrics` and `text_geometry_styled`, `cap_height`) at `4bdffef0`, reduced: the built-in
//! font only, no shaping, styles or paths; curves become lines here rather than sketch curves.

static FONT: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/fonts/liberation-sans/LiberationSans-Regular.ttf"));

/// Most characters set at once.
pub const MAX_TEXT_CHARS: usize = 1000;
/// Most lines a curve is cut into.
const MAX_CURVE_STEPS: f64 = 64.0;

/// Text set as closed contours, in millimetres.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextOutline {
    /// Each a closed polyline, its first point not repeated at the end.
    pub contours: Vec<Vec<[f64; 2]>>,
    /// The characters the font has no glyph for, left out, each once.
    pub missing: Vec<char>,
}

impl TextOutline {
    /// The box round every contour, `(min, max)`; `None` when there are none.
    pub fn bounds(&self) -> Option<([f64; 2], [f64; 2])> {
        let mut points = self.contours.iter().flatten();
        let first = *points.next()?;
        Some(points.fold((first, first), |(lo, hi), p| ([lo[0].min(p[0]), lo[1].min(p[1])], [hi[0].max(p[0]), hi[1].max(p[1])])))
    }
}

/// Collects a glyph's contours, mapped from font units and flattened within `tolerance`.
struct Flatten<'a> {
    map: &'a dyn Fn(f32, f32) -> [f64; 2],
    tolerance: f64,
    contours: &'a mut Vec<Vec<[f64; 2]>>,
    current: Vec<[f64; 2]>,
}

impl Flatten<'_> {
    fn last(&self) -> [f64; 2] {
        self.current.last().copied().unwrap_or([0.0; 2])
    }
    fn push(&mut self, p: [f64; 2]) {
        let l = self.last();
        if self.current.is_empty() || (p[0] - l[0]).hypot(p[1] - l[1]) > 1e-9 {
            self.current.push(p);
        }
    }
    /// How many lines keep a curve within the tolerance, given how far its chord strays when cut
    /// in one: a cut in n strays n² times less.
    fn steps(&self, second: f64) -> usize {
        let n = (second / self.tolerance).sqrt().ceil();
        if n.is_finite() { n.clamp(1.0, MAX_CURVE_STEPS) as usize } else { 1 }
    }
    fn finish(&mut self) {
        let mut c = std::mem::take(&mut self.current);
        if let (Some(a), Some(b)) = (c.first().copied(), c.last().copied())
            && c.len() > 1
            && (a[0] - b[0]).hypot(a[1] - b[1]) <= 1e-9
        {
            c.pop();
        }
        if c.len() >= 3 {
            self.contours.push(c);
        }
    }
}

fn second_difference(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> f64 {
    (a[0] - 2.0 * b[0] + c[0]).hypot(a[1] - 2.0 * b[1] + c[1])
}

impl ttf_parser::OutlineBuilder for Flatten<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        self.finish();
        let p = (self.map)(x, y);
        self.push(p);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        let p = (self.map)(x, y);
        self.push(p);
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let (a, b, c) = (self.last(), (self.map)(x1, y1), (self.map)(x, y));
        let n = self.steps(0.25 * second_difference(a, b, c));
        for i in 1..=n {
            let t = i as f64 / n as f64;
            let u = 1.0 - t;
            let p = [0, 1].map(|k| u * u * a[k] + 2.0 * u * t * b[k] + t * t * c[k]);
            self.push(p);
        }
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let (a, b, c, d) = (self.last(), (self.map)(x1, y1), (self.map)(x2, y2), (self.map)(x, y));
        let n = self.steps(0.75 * second_difference(a, b, c).max(second_difference(b, c, d)));
        for i in 1..=n {
            let t = i as f64 / n as f64;
            let u = 1.0 - t;
            let p = [0, 1].map(|k| u * u * u * a[k] + 3.0 * u * u * t * b[k] + 3.0 * u * t * t * c[k] + t * t * t * d[k]);
            self.push(p);
        }
    }
    fn close(&mut self) {
        self.finish();
    }
}

/// The capital height in font units: the font's own, else its H's, else 0.7 em.
fn cap_height(face: &ttf_parser::Face) -> f64 {
    face.capital_height()
        .map(f64::from)
        .filter(|c| *c > 0.0)
        .or_else(|| face.glyph_index('H').and_then(|g| face.glyph_bounding_box(g)).map(|b| f64::from(b.y_max)).filter(|c| *c > 0.0))
        .unwrap_or(f64::from(face.units_per_em().max(1)) * 0.7)
}

/// `text` with capital letters `height` mm tall, curves within `tolerance` mm: lines break at
/// `\n`, each centred on x = 0, the first line's baseline on y = 0 and the next ones below it,
/// the font's line pitch apart. No kerning. A character the font lacks is left out and listed.
/// `None` for more than [`MAX_TEXT_CHARS`] characters, an unusable height or tolerance, or a
/// font that cannot be read.
pub fn text_outline(text: &str, height: f64, tolerance: f64) -> Option<TextOutline> {
    let usable = |x: f64| x.is_finite() && x > 1e-6 && x < 1e6;
    if !usable(height) || !usable(tolerance) || text.chars().count() > MAX_TEXT_CHARS {
        return None;
    }
    let face = ttf_parser::Face::parse(FONT, 0).ok()?;
    let k = height / cap_height(&face);
    let pitch = f64::from(face.ascender()) - f64::from(face.descender()) + f64::from(face.line_gap());
    let pitch = if pitch > 0.0 { pitch * k } else { height * 1.5 };
    let mut out = TextOutline::default();
    for (row, line) in text.split('\n').enumerate() {
        let mut glyphs = Vec::new();
        let mut pen = 0.0;
        for ch in line.chars() {
            match face.glyph_index(ch).filter(|g| g.0 != 0) {
                Some(g) => {
                    glyphs.push((g, pen));
                    pen += f64::from(face.glyph_hor_advance(g).unwrap_or(0)) * k;
                }
                None if !out.missing.contains(&ch) => out.missing.push(ch),
                None => {}
            }
        }
        let (start, base) = (-pen / 2.0, -(row as f64) * pitch);
        for (g, x) in glyphs {
            let map = move |gx: f32, gy: f32| [start + x + f64::from(gx) * k, base + f64::from(gy) * k];
            let mut f = Flatten { map: &map, tolerance, contours: &mut out.contours, current: Vec::new() };
            let _ = face.outline_glyph(g, &mut f);
            f.finish();
        }
    }
    Some(out)
}

/// The characters of `text` the font has no glyph for, each once, line breaks aside.
pub fn missing_chars(text: &str) -> Vec<char> {
    let Ok(face) = ttf_parser::Face::parse(FONT, 0) else { return text.chars().filter(|c| *c != '\n').collect() };
    let mut out = Vec::new();
    for c in text.chars().filter(|c| *c != '\n') {
        if face.glyph_index(c).is_none_or(|g| g.0 == 0) && !out.contains(&c) {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Twice the area the contours enclose, by the shoelace: positive counter-clockwise.
    fn area(c: &[[f64; 2]]) -> f64 {
        (0..c.len()).map(|i| (c[i], c[(i + 1) % c.len()])).map(|(a, b)| a[0] * b[1] - b[0] * a[1]).sum::<f64>() / 2.0
    }

    #[test]
    fn capitals_stand_at_the_height_and_lines_are_centred_and_stacked() {
        let t = text_outline("H", 5.0, 0.01).unwrap();
        let (lo, hi) = t.bounds().unwrap();
        assert!(lo[1].abs() < 1e-9 && (hi[1] - 5.0).abs() < 1e-6, "{lo:?} {hi:?}");
        assert!((lo[0] + hi[0]).abs() < 1.0, "centred, give or take the side bearings: {lo:?} {hi:?}");

        let two = text_outline("H\nH", 5.0, 0.01).unwrap();
        let (lo2, hi2) = two.bounds().unwrap();
        assert!((hi2[1] - 5.0).abs() < 1e-6 && lo2[1] < -5.0, "{lo2:?}");
        assert!(two.missing.is_empty());
    }

    #[test]
    fn a_letter_with_a_counter_has_a_hole_wound_the_other_way() {
        let o = text_outline("O", 10.0, 0.005).unwrap();
        assert_eq!(o.contours.len(), 2);
        let (a, b) = (area(&o.contours[0]), area(&o.contours[1]));
        assert!(a * b < 0.0, "{a} {b}");
        // The ring: well under the outer disc, curves flattened close to the font's.
        let ring = (a + b).abs();
        assert!(ring > 15.0 && ring < 60.0, "{ring}");
    }

    #[test]
    fn missing_characters_are_listed_and_hostile_input_gives_none() {
        let o = text_outline("A\u{1F600}\u{1F600}", 5.0, 0.01).unwrap();
        assert_eq!(o.missing, vec!['\u{1F600}']);
        assert_eq!(missing_chars("A\u{1F600}\n\u{1F600}é"), o.missing);
        assert!(!o.contours.is_empty());
        assert_eq!(text_outline("", 5.0, 0.01).unwrap().bounds(), None);
        assert_eq!(text_outline("   ", 5.0, 0.01).unwrap().bounds(), None);
        for (h, tol) in [(f64::NAN, 0.01), (0.0, 0.01), (-1.0, 0.01), (1e300, 0.01), (5.0, 0.0), (5.0, f64::INFINITY)] {
            assert!(text_outline("A", h, tol).is_none(), "{h} {tol}");
        }
        assert!(text_outline(&"A".repeat(MAX_TEXT_CHARS + 1), 5.0, 0.01).is_none());
    }
}
