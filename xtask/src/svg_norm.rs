//! An icon as FigCraft exports it (paths carrying `matrix` transforms, masks and clips on groups,
//! definitions last) rewritten as a master `crates/ui-egui/src/icons.rs` draws: absolute
//! `M L C Q A Z` paths in the 64 × 64 box, definitions first, masks on paths, palette colours.

use std::collections::HashMap;
use std::fmt::Write as _;

#[derive(Debug, Clone)]
struct El {
    name: String,
    attrs: Vec<(String, String)>,
    children: Vec<El>,
}

impl El {
    fn get(&self, k: &str) -> Option<&str> {
        self.attrs.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str())
    }
}

/// The element tree of an SVG document (comments, declarations and text left out).
fn parse_tree(src: &str) -> Result<El, String> {
    let mut stack: Vec<El> = vec![El { name: String::new(), attrs: Vec::new(), children: Vec::new() }];
    let mut rest = src;
    while let Some(i) = rest.find('<') {
        let after = rest.get(i + 1..).ok_or("truncated")?;
        let j = after.find('>').ok_or("unterminated tag")?;
        let body = after.get(..j).ok_or("truncated")?;
        rest = after.get(j + 1..).ok_or("truncated")?;
        if body.starts_with('?') || body.starts_with('!') {
            continue;
        }
        if let Some(name) = body.strip_prefix('/') {
            let el = stack.pop().ok_or("unbalanced tags")?;
            if el.name != name.trim() {
                return Err(format!("</{}> closes <{}>", name.trim(), el.name));
            }
            stack.last_mut().ok_or("unbalanced tags")?.children.push(el);
            continue;
        }
        let selfclosing = body.ends_with('/');
        let body = body.strip_suffix('/').unwrap_or(body).trim();
        let name_end = body.find(char::is_whitespace).unwrap_or(body.len());
        let name = body.get(..name_end).ok_or("bad tag")?.to_string();
        let mut attrs = Vec::new();
        let mut a = body.get(name_end..).unwrap_or("");
        while let Some(eq) = a.find('=') {
            let key = a.get(..eq).ok_or("bad attribute")?.trim().to_string();
            let v = a.get(eq + 1..).ok_or("bad attribute")?.trim_start();
            let q = v.chars().next().filter(|c| *c == '"' || *c == '\'').ok_or("unquoted attribute")?;
            let v = v.get(1..).ok_or("bad attribute")?;
            let end = v.find(q).ok_or("unterminated attribute")?;
            attrs.push((key, v.get(..end).ok_or("bad attribute")?.to_string()));
            a = v.get(end + 1..).unwrap_or("");
        }
        let el = El { name, attrs, children: Vec::new() };
        if selfclosing {
            stack.last_mut().ok_or("unbalanced tags")?.children.push(el);
        } else {
            stack.push(el);
        }
    }
    let mut root = stack.pop().ok_or("unbalanced tags")?;
    if !stack.is_empty() {
        return Err("unbalanced tags".into());
    }
    root.children.retain(|c| c.name == "svg");
    root.children.pop().ok_or_else(|| "no <svg>".to_string())
}

/// An affine map `[a b c d e f]`: x' = a x + c y + e, y' = b x + d y + f.
#[derive(Clone, Copy, Debug, PartialEq)]
struct M([f64; 6]);

impl M {
    const ID: M = M([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
    fn then(self, inner: M) -> M {
        let [a, b, c, d, e, f] = self.0;
        let [a2, b2, c2, d2, e2, f2] = inner.0;
        M([a * a2 + c * b2, b * a2 + d * b2, a * c2 + c * d2, b * c2 + d * d2, a * e2 + c * f2 + e, b * e2 + d * f2 + f])
    }
    fn apply(self, (x, y): (f64, f64)) -> (f64, f64) {
        let [a, b, c, d, e, f] = self.0;
        (a * x + c * y + e, b * x + d * y + f)
    }
    fn det(self) -> f64 {
        self.0[0] * self.0[3] - self.0[1] * self.0[2]
    }
    /// The length a unit takes, for stroke widths.
    fn scale(self) -> f64 {
        self.det().abs().sqrt()
    }
}

fn numbers(s: &str) -> Result<Vec<f64>, String> {
    let mut out = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes.get(i).copied().unwrap_or_default();
        if c.is_ascii_whitespace() || c == b',' {
            i += 1;
            continue;
        }
        let start = i;
        if c == b'-' || c == b'+' {
            i += 1;
        }
        let (mut dot, mut exp) = (false, false);
        while let Some(&ch) = bytes.get(i) {
            match ch {
                b'0'..=b'9' => i += 1,
                b'.' if !dot && !exp => {
                    dot = true;
                    i += 1;
                }
                b'e' | b'E' if !exp => {
                    exp = true;
                    i += 1;
                    if matches!(bytes.get(i), Some(b'-' | b'+')) {
                        i += 1;
                    }
                }
                _ => break,
            }
        }
        let text = s.get(start..i).ok_or("bad number")?;
        out.push(text.parse::<f64>().map_err(|_| format!("bad number `{text}`"))?);
        if i == start {
            return Err(format!("bad number data `{s}`"));
        }
    }
    Ok(out)
}

fn transform_attr(v: Option<&str>) -> Result<M, String> {
    let Some(v) = v else { return Ok(M::ID) };
    let mut m = M::ID;
    for part in v.split(')').map(str::trim).filter(|p| !p.is_empty()) {
        let (name, args) = part.split_once('(').ok_or_else(|| format!("bad transform `{v}`"))?;
        let n = numbers(args)?;
        let at = |i: usize| n.get(i).copied().unwrap_or(0.0);
        let t = match (name.trim(), n.len()) {
            ("matrix", 6) => M([at(0), at(1), at(2), at(3), at(4), at(5)]),
            ("translate", 1 | 2) => M([1.0, 0.0, 0.0, 1.0, at(0), at(1)]),
            ("scale", 1) => M([at(0), 0.0, 0.0, at(0), 0.0, 0.0]),
            ("scale", 2) => M([at(0), 0.0, 0.0, at(1), 0.0, 0.0]),
            ("rotate", 1) => {
                let (s, c) = at(0).to_radians().sin_cos();
                M([c, s, -s, c, 0.0, 0.0])
            }
            _ => return Err(format!("transform `{part})` is not supported")),
        };
        m = m.then(t);
    }
    Ok(m)
}

/// A path's segments, absolute.
#[derive(Clone, Debug)]
enum Seg {
    M(f64, f64),
    L(f64, f64),
    C(f64, f64, f64, f64, f64, f64),
    Q(f64, f64, f64, f64),
    A(f64, f64, f64, bool, bool, f64, f64),
    Z,
}

fn parse_d(d: &str) -> Result<Vec<Seg>, String> {
    let mut segs = Vec::new();
    let (mut cx, mut cy, mut sx, mut sy) = (0.0, 0.0, 0.0, 0.0);
    // The last control point, for S and T.
    let (mut last_c, mut last_q): (Option<(f64, f64)>, Option<(f64, f64)>) = (None, None);
    let mut chars = d.char_indices().peekable();
    let mut cmds: Vec<(char, Vec<f64>)> = Vec::new();
    while let Some((i, c)) = chars.next() {
        if c.is_ascii_alphabetic() {
            let start = i + c.len_utf8();
            let mut end = d.len();
            while let Some(&(j, n)) = chars.peek() {
                if n.is_ascii_alphabetic() && n != 'e' && n != 'E' {
                    end = j;
                    break;
                }
                chars.next();
            }
            cmds.push((c, numbers(d.get(start..end).unwrap_or(""))?));
        } else if !c.is_whitespace() && c != ',' {
            return Err(format!("path data must start with a command: `{d}`"));
        }
    }
    for (cmd, args) in cmds {
        let rel = cmd.is_ascii_lowercase();
        let arity = match cmd.to_ascii_uppercase() {
            'M' | 'L' | 'T' => 2,
            'H' | 'V' => 1,
            'C' => 6,
            'S' | 'Q' => 4,
            'A' => 7,
            'Z' => 0,
            other => return Err(format!("path command `{other}` is not supported")),
        };
        if arity == 0 {
            segs.push(Seg::Z);
            (cx, cy) = (sx, sy);
            (last_c, last_q) = (None, None);
            continue;
        }
        if args.is_empty() || args.len() % arity != 0 {
            return Err(format!("path command `{cmd}` has {} numbers", args.len()));
        }
        for (k, a) in args.chunks(arity).enumerate() {
            let v = |i: usize| a.get(i).copied().unwrap_or(0.0);
            let (ox, oy) = if rel { (cx, cy) } else { (0.0, 0.0) };
            let mut this_c = None;
            let mut this_q = None;
            match cmd.to_ascii_uppercase() {
                'M' if k == 0 => {
                    (cx, cy) = (ox + v(0), oy + v(1));
                    (sx, sy) = (cx, cy);
                    segs.push(Seg::M(cx, cy));
                }
                'M' | 'L' => {
                    (cx, cy) = (ox + v(0), oy + v(1));
                    segs.push(Seg::L(cx, cy));
                }
                'H' => {
                    cx = if rel { cx + v(0) } else { v(0) };
                    segs.push(Seg::L(cx, cy));
                }
                'V' => {
                    cy = if rel { cy + v(0) } else { v(0) };
                    segs.push(Seg::L(cx, cy));
                }
                'C' => {
                    let (x1, y1, x2, y2) = (ox + v(0), oy + v(1), ox + v(2), oy + v(3));
                    (cx, cy) = (ox + v(4), oy + v(5));
                    segs.push(Seg::C(x1, y1, x2, y2, cx, cy));
                    this_c = Some((x2, y2));
                }
                'S' => {
                    let (x1, y1) = last_c.map_or((cx, cy), |(px, py)| (2.0 * cx - px, 2.0 * cy - py));
                    let (x2, y2) = (ox + v(0), oy + v(1));
                    (cx, cy) = (ox + v(2), oy + v(3));
                    segs.push(Seg::C(x1, y1, x2, y2, cx, cy));
                    this_c = Some((x2, y2));
                }
                'Q' => {
                    let (x1, y1) = (ox + v(0), oy + v(1));
                    (cx, cy) = (ox + v(2), oy + v(3));
                    segs.push(Seg::Q(x1, y1, cx, cy));
                    this_q = Some((x1, y1));
                }
                'T' => {
                    let (x1, y1) = last_q.map_or((cx, cy), |(px, py)| (2.0 * cx - px, 2.0 * cy - py));
                    (cx, cy) = (ox + v(0), oy + v(1));
                    segs.push(Seg::Q(x1, y1, cx, cy));
                    this_q = Some((x1, y1));
                }
                _ => {
                    (cx, cy) = (ox + v(5), oy + v(6));
                    segs.push(Seg::A(v(0), v(1), v(2), v(3) != 0.0, v(4) != 0.0, cx, cy));
                }
            }
            (last_c, last_q) = (this_c, this_q);
        }
    }
    Ok(segs)
}

fn num(v: f64) -> String {
    let s = format!("{v:.3}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" || s.is_empty() { "0".into() } else { s.to_string() }
}

fn write_d(segs: &[Seg], m: M) -> Result<String, String> {
    let mut out = String::new();
    let p = |x: f64, y: f64| {
        let (x, y) = m.apply((x, y));
        format!("{} {}", num(x), num(y))
    };
    for s in segs {
        match *s {
            Seg::M(x, y) => {
                let _ = write!(out, "M{}", p(x, y));
            }
            Seg::L(x, y) => {
                let _ = write!(out, "L{}", p(x, y));
            }
            Seg::C(a, b, c, d, e, f) => {
                let _ = write!(out, "C{} {} {}", p(a, b), p(c, d), p(e, f));
            }
            Seg::Q(a, b, c, d) => {
                let _ = write!(out, "Q{} {}", p(a, b), p(c, d));
            }
            Seg::A(rx, ry, rot, large, sweep, x, y) => {
                // Arcs survive a map that keeps circles circles: a uniform scale, a turn, a mirror.
                let [a, b, c, d, _, _] = m.0;
                let s = m.scale();
                if (a - d * m.det().signum()).abs() > 1e-6 || (b + c * m.det().signum()).abs() > 1e-6 {
                    return Err("an arc under a skew or uneven scale".into());
                }
                let turn = b.atan2(a).to_degrees();
                let sweep = if m.det() < 0.0 { !sweep } else { sweep };
                let _ = write!(out, "A{} {} {} {} {} {}", num(rx * s), num(ry * s), num(rot + turn), u8::from(large), u8::from(sweep), p(x, y));
            }
            Seg::Z => out.push('Z'),
        }
    }
    Ok(out)
}

/// A shape element's outline as path segments.
fn outline(el: &El) -> Result<Vec<Seg>, String> {
    let n = |k: &str| el.get(k).and_then(|v| v.trim().parse::<f64>().ok()).unwrap_or(0.0);
    match el.name.as_str() {
        "path" => parse_d(el.get("d").ok_or("path without d")?),
        "rect" => {
            let (x, y, w, h) = (n("x"), n("y"), n("width"), n("height"));
            Ok(vec![Seg::M(x, y), Seg::L(x + w, y), Seg::L(x + w, y + h), Seg::L(x, y + h), Seg::Z])
        }
        "circle" | "ellipse" => {
            let (cx, cy) = (n("cx"), n("cy"));
            let (rx, ry) = if el.name == "circle" { (n("r"), n("r")) } else { (n("rx"), n("ry")) };
            Ok(vec![Seg::M(cx + rx, cy), Seg::A(rx, ry, 0.0, false, true, cx - rx, cy), Seg::A(rx, ry, 0.0, false, true, cx + rx, cy), Seg::Z])
        }
        other => Err(format!("<{other}> is not supported in an icon: outline it first")),
    }
}

/// What a master's paint attribute becomes, from an exported colour: a palette role's colour,
/// `currentColor` or `none`.
pub type PaintMap<'a> = &'a dyn Fn(&str) -> Result<String, String>;

struct Writer<'a> {
    defs: String,
    body: String,
    paint: PaintMap<'a>,
    defs_by_id: HashMap<String, El>,
    next: usize,
}

const SHAPES: &[&str] = &["path", "rect", "circle", "ellipse"];

impl Writer<'_> {
    fn def(&self, url: Option<&str>) -> Result<Option<&El>, String> {
        let Some(id) = url.and_then(|u| u.strip_prefix("url(#")).and_then(|u| u.strip_suffix(')')) else { return Ok(None) };
        self.defs_by_id.get(id).map(Some).ok_or_else(|| format!("unknown reference #{id}"))
    }

    /// A clip that leaves the whole box (or more) showing is no clip.
    fn clip_is_noop(&self, clip: &El, m: M) -> bool {
        let mut shapes = Vec::new();
        collect_shapes(clip, m, &mut shapes);
        let [(el, pm)] = shapes.as_slice() else { return false };
        let Ok(segs) = outline(el) else { return false };
        let pts: Vec<(f64, f64)> = segs
            .iter()
            .filter_map(|s| match *s {
                Seg::M(x, y) | Seg::L(x, y) => Some(pm.apply((x, y))),
                _ => None,
            })
            .collect();
        let (min_x, max_x) = pts.iter().fold((f64::MAX, f64::MIN), |(a, b), p| (a.min(p.0), b.max(p.0)));
        let (min_y, max_y) = pts.iter().fold((f64::MAX, f64::MIN), |(a, b), p| (a.min(p.1), b.max(p.1)));
        segs.len() <= 6 && min_x <= 0.01 && min_y <= 0.01 && max_x >= 63.99 && max_y >= 63.99
    }

    fn shape(&self, el: &El, m: M, in_mask: bool, mask: Option<&str>) -> Result<String, String> {
        let d = write_d(&outline(el)?, m)?;
        let mut out = format!("<path d=\"{d}\"");
        let op = |k: &str| el.get(k).and_then(|v| v.parse::<f64>().ok()).unwrap_or(1.0);
        let mut opacity = op("opacity");
        for (attr, own_opacity) in [("fill", "fill-opacity"), ("stroke", "stroke-opacity")] {
            let raw = el.get(attr).unwrap_or(if attr == "fill" && !in_mask { "#000000" } else { "none" });
            if raw.starts_with("url(") {
                return Err(format!("{attr} {raw}: gradients are not supported in an icon"));
            }
            let value = if raw == "none" {
                "none".to_string()
            } else if in_mask {
                let rgb = crate::design_tokens::hex(raw).unwrap_or([0, 0, 0, 255]);
                let luma = 0.2126 * f64::from(rgb[0]) + 0.7152 * f64::from(rgb[1]) + 0.0722 * f64::from(rgb[2]);
                if luma > 127.5 { "white".into() } else { "black".into() }
            } else {
                (self.paint)(raw)?
            };
            if value != "none" {
                opacity *= op(own_opacity);
            }
            if attr == "fill" || value != "none" {
                let _ = write!(out, " {attr}=\"{value}\"");
            }
        }
        if el.get("fill-rule") == Some("evenodd") {
            out.push_str(" fill-rule=\"evenodd\"");
        }
        if el.get("stroke").is_some_and(|s| s != "none") {
            let w = el.get("stroke-width").and_then(|w| w.parse::<f64>().ok()).unwrap_or(1.0) * m.scale();
            let _ = write!(out, " stroke-width=\"{}\" stroke-linejoin=\"round\" stroke-linecap=\"round\"", num(w));
            if let Some(dash) = el.get("stroke-dasharray").filter(|d| *d != "none") {
                let dashes: Vec<String> = numbers(dash)?.iter().map(|v| num(v * m.scale())).collect();
                let _ = write!(out, " stroke-dasharray=\"{}\"", dashes.join(" "));
            }
        }
        if (opacity - 1.0).abs() > 1e-6 {
            let _ = write!(out, " opacity=\"{}\"", num(opacity));
        }
        if let Some(mask) = mask {
            let _ = write!(out, " mask=\"url(#{mask})\"");
        }
        out.push_str("/>");
        Ok(out)
    }

    fn mask_def(&mut self, el: &El, m: M) -> Result<String, String> {
        self.next += 1;
        let id = format!("m{}", self.next);
        let mut shapes = Vec::new();
        collect_shapes(el, m, &mut shapes);
        let _ = writeln!(self.defs, "    <mask id=\"{id}\" maskUnits=\"userSpaceOnUse\" x=\"-32\" y=\"-32\" width=\"128\" height=\"128\">");
        for (s, pm) in shapes {
            let line = self.shape(&s, pm, true, None)?;
            let _ = writeln!(self.defs, "      {line}");
        }
        self.defs.push_str("    </mask>\n");
        Ok(id)
    }

    fn clip_def(&mut self, el: &El, m: M) -> Result<String, String> {
        self.next += 1;
        let id = format!("c{}", self.next);
        let mut shapes = Vec::new();
        collect_shapes(el, m, &mut shapes);
        let _ = writeln!(self.defs, "    <clipPath id=\"{id}\">");
        for (s, pm) in shapes {
            let d = write_d(&outline(&s)?, pm)?;
            let rule = if s.get("clip-rule").or(s.get("fill-rule")) == Some("evenodd") { " fill-rule=\"evenodd\"" } else { "" };
            let _ = writeln!(self.defs, "      <path d=\"{d}\"{rule}/>");
        }
        self.defs.push_str("    </clipPath>\n");
        Ok(id)
    }

    fn walk(&mut self, el: &El, m: M, mask: Option<&str>, depth: usize) -> Result<(), String> {
        if depth > 64 {
            return Err("groups nest too deep".into());
        }
        for child in &el.children {
            let cm = m.then(transform_attr(child.get("transform"))?);
            match child.name.as_str() {
                "defs" | "title" | "desc" | "metadata" | "clipPath" | "mask" => {}
                "g" => {
                    let mask_id = match self.def(child.get("mask"))?.cloned() {
                        Some(def) => Some(self.mask_def(&def, cm)?),
                        None => mask.map(str::to_string),
                    };
                    let clip = match self.def(child.get("clip-path"))?.cloned() {
                        Some(def) if !self.clip_is_noop(&def, cm) => Some(self.clip_def(&def, cm)?),
                        _ => None,
                    };
                    let opacity = child.get("opacity").and_then(|o| o.parse::<f64>().ok()).unwrap_or(1.0);
                    let wrap = clip.is_some() || (opacity - 1.0).abs() > 1e-6;
                    if wrap {
                        let mut open = String::from("  <g");
                        if let Some(c) = &clip {
                            let _ = write!(open, " clip-path=\"url(#{c})\"");
                        }
                        if (opacity - 1.0).abs() > 1e-6 {
                            let _ = write!(open, " opacity=\"{}\"", num(opacity));
                        }
                        let _ = writeln!(self.body, "{open}>");
                    }
                    self.walk(child, cm, mask_id.as_deref(), depth + 1)?;
                    if wrap {
                        self.body.push_str("  </g>\n");
                    }
                }
                name if SHAPES.contains(&name) => {
                    let own_mask = match self.def(child.get("mask"))?.cloned() {
                        Some(def) => Some(self.mask_def(&def, cm)?),
                        None => mask.map(str::to_string),
                    };
                    if self.def(child.get("clip-path"))?.is_some_and(|c| !self.clip_is_noop(c, cm)) {
                        return Err("a clip on a single shape: clip a group instead".into());
                    }
                    let line = self.shape(child, cm, false, own_mask.as_deref())?;
                    let _ = writeln!(self.body, "  {line}");
                }
                other => return Err(format!("<{other}> is not supported in an icon")),
            }
        }
        Ok(())
    }
}

/// Every shape under `el` (groups flattened) with its map.
fn collect_shapes(el: &El, m: M, out: &mut Vec<(El, M)>) {
    for child in &el.children {
        let Ok(t) = transform_attr(child.get("transform")) else { continue };
        let cm = m.then(t);
        if child.name == "g" {
            collect_shapes(child, cm, out);
        } else if SHAPES.contains(&child.name.as_str()) {
            out.push((child.clone(), cm));
        }
    }
}

fn collect_defs(el: &El, out: &mut HashMap<String, El>) {
    for child in &el.children {
        if let ("clipPath" | "mask", Some(id)) = (child.name.as_str(), child.get("id")) {
            out.insert(id.to_string(), child.clone());
        }
        collect_defs(child, out);
    }
}

/// `src` (an exported icon) as a master titled `title`; `paint` maps its colours.
pub fn normalise(src: &str, title: &str, paint: PaintMap<'_>) -> Result<String, String> {
    let svg = parse_tree(src)?;
    let view = svg.get("viewBox").map(numbers).transpose()?.unwrap_or_else(|| vec![0.0, 0.0, 64.0, 64.0]);
    let [vx, vy, vw, vh] = view.as_slice() else { return Err("bad viewBox".into()) };
    if *vw <= 0.0 || *vh <= 0.0 {
        return Err("bad viewBox".into());
    }
    let fit = M([64.0 / vw, 0.0, 0.0, 64.0 / vh, -vx * 64.0 / vw, -vy * 64.0 / vh]);
    let mut defs_by_id = HashMap::new();
    collect_defs(&svg, &mut defs_by_id);
    let mut w = Writer { defs: String::new(), body: String::new(), paint, defs_by_id, next: 0 };
    w.walk(&svg, fit, None, 0)?;
    let mut out = String::from("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 64 64\">\n");
    let _ = writeln!(out, "  <title>{title}</title>");
    if !w.defs.is_empty() {
        let _ = write!(out, "  <defs>\n{}  </defs>\n", w.defs);
    }
    out.push_str(&w.body);
    out.push_str("</svg>\n");
    Ok(out)
}

/// FNV-1a, to tell whether an exported icon changed since the last export.
pub fn fingerprint(text: &str) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in text.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paint(c: &str) -> Result<String, String> {
        match c.to_ascii_lowercase().as_str() {
            "#010101" => Ok("currentColor".into()),
            "#b8c3cf" => Ok("#b8c3cf".into()),
            other => Err(format!("{other} is not a palette colour")),
        }
    }

    #[test]
    fn transforms_are_baked_and_defs_come_first() {
        let src = r##"<svg width="64" height="64" viewBox="0 0 64 64" fill="none" xmlns="http://www.w3.org/2000/svg"><g><g mask="url(#mask2)"><g clip-path="url(#clip1)"><path transform="matrix(1 0 0 1 10 20)" d="M0 0L4 0L4 4Z" fill="#010101"/></g></g></g><defs><clipPath id="clip1"><path d="M0 0L64 0L64 64L0 64Z"/></clipPath><mask id="mask2" maskUnits="userSpaceOnUse"><g><path transform="matrix(1 0 0 1 -32 -32)" d="M0 0L128 0L128 128L0 128Z" fill="#FFFFFF"/></g></mask></defs></svg>"##;
        let out = normalise(src, "T", &paint).unwrap();
        assert!(out.contains("<path d=\"M10 20L14 20L14 24Z\" fill=\"currentColor\" mask=\"url(#m1)\"/>"), "{out}");
        assert!(out.find("<defs>").unwrap() < out.find("M10 20").unwrap(), "{out}");
        assert!(out.contains("fill=\"white\""), "{out}");
        assert!(!out.contains("clipPath"), "a clip of the whole box is dropped: {out}");
    }

    #[test]
    fn relative_and_shorthand_commands_become_absolute() {
        let segs = parse_d("m1 1h2v2l-1 1zM0 0c1 1 2 2 3 3s1 1 2 2q1 0 1 1t1 1").unwrap();
        let d = write_d(&segs, M::ID).unwrap();
        assert_eq!(d, "M1 1L3 1L3 3L2 4ZM0 0C1 1 2 2 3 3C4 4 4 4 5 5Q6 5 6 6Q6 7 7 7");
        let d = write_d(&parse_d("M0 0A2 2 0 0 1 4 0").unwrap(), M([2.0, 0.0, 0.0, 2.0, 1.0, 1.0])).unwrap();
        assert_eq!(d, "M1 1A4 4 0 0 1 9 1");
        assert!(write_d(&parse_d("M0 0A2 2 0 0 1 4 0").unwrap(), M([2.0, 0.0, 0.0, 1.0, 0.0, 0.0])).is_err());
    }

    #[test]
    fn foreign_colours_and_elements_are_refused() {
        let svg = |body: &str| format!("<svg viewBox=\"0 0 64 64\">{body}</svg>");
        assert!(normalise(&svg("<path d=\"M0 0L1 1\" fill=\"#123456\"/>"), "T", &paint).is_err());
        assert!(normalise(&svg("<text>x</text>"), "T", &paint).is_err());
        assert!(normalise(&svg("<path d=\"M0 0L1 1\" fill=\"url(#g)\"/>"), "T", &paint).is_err());
        assert!(normalise("<svg><g>", "T", &paint).is_err());
    }

    #[test]
    fn a_smaller_artboard_is_scaled_to_the_box() {
        let out =
            normalise("<svg viewBox=\"0 0 32 32\"><path d=\"M0 0L32 32\" fill=\"none\" stroke=\"#010101\" stroke-width=\"1\"/></svg>", "T", &paint)
                .unwrap();
        assert!(out.contains("d=\"M0 0L64 64\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"2\""), "{out}");
    }
}
