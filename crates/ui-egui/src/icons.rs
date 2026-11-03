// Ported from SolidCraft (MIT OR Apache-2.0), crates/ui-egui/src/icon_svg.rs and
// crates/ui-egui/src/icons.rs at 4bdffef0, with the palette roles of
// assets/icons/ui/design/palette.toml; see ATTRIBUTION.md. Reduced: no accents, colour-vision
// variants, aliases or audit.

//! The UI icons: SVG masters in `crates/ui-egui/icons` (SolidCraft artwork), a small linted
//! subset (absolute `M L C Q A Z` paths, circles, groups with a clip or opacity, masks and clip
//! paths), rasterised with tiny-skia at the pixel size they are shown and cached as textures.
//! A master never carries a colour of its own: its colours are the palette roles' source values,
//! resolved to the theme's colour when drawn. `gap` erases earlier artwork to transparency.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use egui::{Color32, Painter, Rect, TextureHandle, TextureOptions, pos2};
use tiny_skia as sk;

/// Palette roles: name, the hex the SVG masters use, and the light and dark display colours.
const ROLES: &[(&str, &str, [u8; 3], [u8; 3])] = &[
    ("ink", "#34465b", [0x34, 0x46, 0x5b], [0x1f, 0x20, 0x21]),
    ("line", "#34465b", [0x34, 0x46, 0x5b], [0xdb, 0xe4, 0xee]),
    ("passive-1", "#e2e8ef", [0xe2, 0xe8, 0xef], [0x8d, 0xa3, 0xc3]),
    ("passive-2", "#b8c3cf", [0xb8, 0xc3, 0xcf], [0x6d, 0x81, 0xa2]),
    ("passive-3", "#8e9faf", [0x8e, 0x9f, 0xaf], [0x54, 0x67, 0x82]),
    ("gap", "#ffffff", [0xff, 0xff, 0xff], [0x1e, 0x25, 0x30]),
    ("guide", "#6f879c", [0x6f, 0x87, 0x9c], [0x80, 0x93, 0xa4]),
    ("active-1", "#b9ddf8", [0xb9, 0xdd, 0xf8], [0x4e, 0xa1, 0xdc]),
    ("active-2", "#328bd3", [0x32, 0x8b, 0xd3], [0x32, 0x87, 0xd1]),
    ("active-3", "#236ead", [0x23, 0x6e, 0xad], [0x29, 0x76, 0xb8]),
    ("component-orange-1", "#f1d2af", [0xf1, 0xd2, 0xaf], [0xba, 0x8f, 0x6a]),
    ("component-orange-2", "#c06f00", [0xc0, 0x6f, 0x00], [0xb8, 0x6f, 0x04]),
    ("component-orange-3", "#a45000", [0xa4, 0x50, 0x00], [0x95, 0x68, 0x2d]),
    ("component-yellow-1", "#e5d99b", [0xe5, 0xd9, 0x9b], [0xa9, 0x98, 0x5f]),
    ("component-yellow-2", "#a28200", [0xa2, 0x82, 0x00], [0x9a, 0x81, 0x00]),
    ("component-yellow-3", "#826700", [0x82, 0x67, 0x00], [0x85, 0x71, 0x00]),
];

/// Every icon by name.
const MASTERS: &[(&str, &str)] = &[
    ("close", include_str!("../icons/close.svg")),
    ("cone", include_str!("../icons/cone.svg")),
    ("delete", include_str!("../icons/delete.svg")),
    ("fit", include_str!("../icons/fit.svg")),
    ("home", include_str!("../icons/home.svg")),
    ("import", include_str!("../icons/import.svg")),
    ("move", include_str!("../icons/move.svg")),
    ("perspective", include_str!("../icons/perspective.svg")),
    ("redo", include_str!("../icons/redo.svg")),
    ("revolve", include_str!("../icons/revolve.svg")),
    ("scale", include_str!("../icons/scale.svg")),
    ("settings", include_str!("../icons/settings.svg")),
    ("undo", include_str!("../icons/undo.svg")),
    ("ws_design", include_str!("../icons/ws_design.svg")),
];

/// How a shape is painted.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Paint {
    None,
    /// Erase earlier artwork; never paint a theme background into the texture.
    Knockout,
    /// `currentColor`: the `line` role.
    Line,
    /// A palette role (index into `ROLES`).
    Role(usize),
    /// A fixed colour (mask artwork).
    Fixed([u8; 3]),
}

#[derive(Debug)]
struct Node {
    path: sk::Path,
    fill: Paint,
    rule: sk::FillRule,
    stroke: Paint,
    width: f32,
    dash: Vec<f32>,
    opacity: f32,
    mask: Option<usize>,
}

#[derive(Debug)]
enum Item {
    Node(Node),
    Group { clip: Option<usize>, opacity: f32, items: Vec<Item> },
}

/// A parsed master.
#[derive(Debug)]
pub struct Doc {
    items: Vec<Item>,
    masks: Vec<Vec<Node>>,
    clips: Vec<Vec<Node>>,
}

// ---------------------------------------------------------------------------------------------
// Parsing

struct Tag<'a> {
    name: &'a str,
    attrs: Vec<(&'a str, &'a str)>,
    close: bool,
}

impl Tag<'_> {
    fn get(&self, k: &str) -> Option<&str> {
        self.attrs.iter().find(|(n, _)| *n == k).map(|(_, v)| *v)
    }
    fn num(&self, k: &str) -> Result<f32, String> {
        self.get(k).ok_or_else(|| format!("missing {k}"))?.trim().parse::<f32>().map_err(|e| format!("{k}: {e}"))
    }
}

fn tags(src: &str) -> Result<Vec<Tag<'_>>, String> {
    let mut out = Vec::new();
    let mut rest = src;
    while let Some(i) = rest.find('<') {
        let after = rest.get(i + 1..).ok_or("truncated")?;
        let j = after.find('>').ok_or("unterminated tag")?;
        let body = after.get(..j).ok_or("truncated")?;
        rest = after.get(j + 1..).ok_or("truncated")?;
        if body.starts_with('?') || body.starts_with('!') {
            continue;
        }
        let (close, body) = match body.strip_prefix('/') {
            Some(b) => (true, b),
            None => (false, body),
        };
        let body = body.strip_suffix('/').unwrap_or(body).trim();
        let name_end = body.find(char::is_whitespace).unwrap_or(body.len());
        let name = body.get(..name_end).ok_or("bad tag")?;
        let mut attrs = Vec::new();
        let mut a = body.get(name_end..).unwrap_or("");
        while let Some(eq) = a.find('=') {
            let key = a.get(..eq).ok_or("bad attribute")?.trim();
            let v = a.get(eq + 1..).ok_or("bad attribute")?.trim_start();
            let q = v.chars().next().filter(|c| *c == '"' || *c == '\'').ok_or("unquoted attribute")?;
            let v = v.get(1..).ok_or("bad attribute")?;
            let end = v.find(q).ok_or("unterminated attribute")?;
            attrs.push((key, v.get(..end).ok_or("bad attribute")?));
            a = v.get(end + 1..).unwrap_or("");
        }
        out.push(Tag { name, attrs, close });
    }
    Ok(out)
}

/// An SVG elliptical arc as cubic Béziers appended to `pb`.
fn arc(pb: &mut sk::PathBuilder, x1: f32, y1: f32, rx: f32, ry: f32, rot_deg: f32, large: bool, sweep: bool, x2: f32, y2: f32) {
    let (mut rx, mut ry) = (rx.abs(), ry.abs());
    if rx < 1e-6 || ry < 1e-6 || ((x1 - x2).abs() < 1e-6 && (y1 - y2).abs() < 1e-6) {
        pb.line_to(x2, y2);
        return;
    }
    let phi = rot_deg.to_radians();
    let (sp, cp) = phi.sin_cos();
    let (dx, dy) = ((x1 - x2) / 2.0, (y1 - y2) / 2.0);
    let (x1p, y1p) = (cp * dx + sp * dy, -sp * dx + cp * dy);
    let lam = (x1p * x1p) / (rx * rx) + (y1p * y1p) / (ry * ry);
    if lam > 1.0 {
        let s = lam.sqrt();
        rx *= s;
        ry *= s;
    }
    let num = rx * rx * ry * ry - rx * rx * y1p * y1p - ry * ry * x1p * x1p;
    let den = rx * rx * y1p * y1p + ry * ry * x1p * x1p;
    let sign = if large == sweep { -1.0 } else { 1.0 };
    let coef = sign * (num / den).max(0.0).sqrt();
    let (cxp, cyp) = (coef * rx * y1p / ry, -coef * ry * x1p / rx);
    let (cx, cy) = (cp * cxp - sp * cyp + (x1 + x2) / 2.0, sp * cxp + cp * cyp + (y1 + y2) / 2.0);
    let ang = |ux: f32, uy: f32, vx: f32, vy: f32| (ux * vy - uy * vx).atan2(ux * vx + uy * vy);
    let (ux, uy) = ((x1p - cxp) / rx, (y1p - cyp) / ry);
    let (vx, vy) = ((-x1p - cxp) / rx, (-y1p - cyp) / ry);
    let theta = ang(1.0, 0.0, ux, uy);
    let mut dtheta = ang(ux, uy, vx, vy);
    if !sweep && dtheta > 0.0 {
        dtheta -= std::f32::consts::TAU;
    } else if sweep && dtheta < 0.0 {
        dtheta += std::f32::consts::TAU;
    }
    let n = ((dtheta.abs() / std::f32::consts::FRAC_PI_2).ceil() as usize).clamp(1, 8);
    let delta = dtheta / n as f32;
    let t = 4.0 / 3.0 * (delta / 4.0).tan();
    let map = |x: f32, y: f32| (cx + rx * cp * x - ry * sp * y, cy + rx * sp * x + ry * cp * y);
    for i in 0..n {
        let a1 = theta + delta * i as f32;
        let a2 = a1 + delta;
        let (s1, c1) = a1.sin_cos();
        let (s2, c2) = a2.sin_cos();
        let (p1x, p1y) = map(c1 - t * s1, s1 + t * c1);
        let (p2x, p2y) = map(c2 + t * s2, s2 - t * c2);
        let (ex, ey) = map(c2, s2);
        pb.cubic_to(p1x, p1y, p2x, p2y, ex, ey);
    }
}

fn parse_path(d: &str) -> Result<sk::Path, String> {
    let mut toks: Vec<&str> = Vec::new();
    let b = d.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let c = *b.get(i).ok_or("path")?;
        if c.is_ascii_alphabetic() {
            toks.push(d.get(i..=i).ok_or("path")?);
            i += 1;
        } else if c == b'-' || c == b'.' || c.is_ascii_digit() {
            let s = i;
            i += 1;
            while let Some(&n) = b.get(i) {
                if n.is_ascii_digit()
                    || n == b'.'
                    || ((n == b'-' || n == b'+') && matches!(b.get(i - 1), Some(b'e' | b'E')))
                    || n == b'e'
                    || n == b'E'
                {
                    i += 1;
                } else {
                    break;
                }
            }
            toks.push(d.get(s..i).ok_or("path")?);
        } else {
            i += 1;
        }
    }
    let mut pb = sk::PathBuilder::new();
    let (mut k, mut cmd) = (0usize, ' ');
    let (mut x, mut y) = (0.0f32, 0.0f32);
    let mut open = false;
    let num = |k: &mut usize| -> Result<f32, String> {
        let t = toks.get(*k).ok_or("path: missing number")?;
        *k += 1;
        t.parse::<f32>().map_err(|e| format!("path: {e}"))
    };
    while k < toks.len() {
        let t = *toks.get(k).ok_or("path")?;
        if t.len() == 1 && t.as_bytes().first().is_some_and(u8::is_ascii_alphabetic) {
            cmd = t.chars().next().ok_or("path")?;
            k += 1;
        }
        match cmd {
            'M' => {
                (x, y) = (num(&mut k)?, num(&mut k)?);
                pb.move_to(x, y);
                open = true;
                cmd = 'L';
            }
            'L' => {
                (x, y) = (num(&mut k)?, num(&mut k)?);
                pb.line_to(x, y);
            }
            'C' => {
                let v = [num(&mut k)?, num(&mut k)?, num(&mut k)?, num(&mut k)?, num(&mut k)?, num(&mut k)?];
                pb.cubic_to(v[0], v[1], v[2], v[3], v[4], v[5]);
                (x, y) = (v[4], v[5]);
            }
            'Q' => {
                let v = [num(&mut k)?, num(&mut k)?, num(&mut k)?, num(&mut k)?];
                pb.quad_to(v[0], v[1], v[2], v[3]);
                (x, y) = (v[2], v[3]);
            }
            'A' => {
                let (rx, ry, rot) = (num(&mut k)?, num(&mut k)?, num(&mut k)?);
                let (fa, fs) = (num(&mut k)? != 0.0, num(&mut k)? != 0.0);
                let (ex, ey) = (num(&mut k)?, num(&mut k)?);
                arc(&mut pb, x, y, rx, ry, rot, fa, fs, ex, ey);
                (x, y) = (ex, ey);
            }
            'Z' => {
                if open {
                    pb.close();
                }
                // A number after Z is not a command of its own.
                if toks.get(k).is_some_and(|t| !t.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)) {
                    return Err("path: number after Z".into());
                }
            }
            other => return Err(format!("path command {other} is not supported")),
        }
    }
    pb.finish().ok_or_else(|| "empty path".to_string())
}

fn role(name: &str) -> Option<usize> {
    ROLES.iter().position(|r| r.0 == name)
}

fn parse_paint(v: Option<&str>, in_mask: bool) -> Result<Paint, String> {
    let Some(v) = v else { return Ok(Paint::None) };
    match v {
        "none" => Ok(Paint::None),
        "currentColor" => Ok(Paint::Line),
        "white" if in_mask => Ok(Paint::Fixed([255, 255, 255])),
        "black" if in_mask => Ok(Paint::Fixed([0, 0, 0])),
        _ => {
            let lower = v.to_ascii_lowercase();
            ROLES
                .iter()
                .position(|r| r.1 == lower)
                .map(|i| if ROLES.get(i).is_some_and(|r| r.0 == "gap") { Paint::Knockout } else { Paint::Role(i) })
                .ok_or_else(|| format!("colour {v} is not a palette colour"))
        }
    }
}

fn parse_dash(v: Option<&str>) -> Vec<f32> {
    let mut d: Vec<f32> = v.map(|s| s.split([' ', ',']).filter_map(|n| n.parse().ok()).collect()).unwrap_or_default();
    if d.len() % 2 == 1 {
        d.extend(d.clone());
    }
    d
}

fn url_ref(v: &str) -> Option<&str> {
    v.strip_prefix("url(#")?.strip_suffix(')')
}

enum Frame {
    Group { clip: Option<usize>, opacity: f32, items: Vec<Item> },
    Mask(usize),
    Clip(usize),
    Defs,
}

impl Doc {
    pub fn parse(src: &str) -> Result<Doc, String> {
        let mut doc = Doc { items: Vec::new(), masks: Vec::new(), clips: Vec::new() };
        let (mut mask_ids, mut clip_ids): (HashMap<String, usize>, HashMap<String, usize>) = (HashMap::new(), HashMap::new());
        let mut stack: Vec<Frame> = Vec::new();
        for t in tags(src)? {
            match (t.name, t.close) {
                ("svg" | "title", _) => {}
                ("defs", false) => stack.push(Frame::Defs),
                ("mask", false) => {
                    doc.masks.push(Vec::new());
                    mask_ids.insert(t.get("id").ok_or("mask without id")?.to_string(), doc.masks.len() - 1);
                    stack.push(Frame::Mask(doc.masks.len() - 1));
                }
                ("clipPath", false) => {
                    doc.clips.push(Vec::new());
                    clip_ids.insert(t.get("id").ok_or("clipPath without id")?.to_string(), doc.clips.len() - 1);
                    stack.push(Frame::Clip(doc.clips.len() - 1));
                }
                ("g", false) => {
                    let clip = match t.get("clip-path").and_then(url_ref) {
                        Some(id) => Some(*clip_ids.get(id).ok_or("unknown clip")?),
                        None => None,
                    };
                    let opacity = t.get("opacity").and_then(|o| o.parse().ok()).unwrap_or(1.0);
                    stack.push(Frame::Group { clip, opacity, items: Vec::new() });
                }
                ("defs" | "mask" | "clipPath" | "g", true) => match stack.pop() {
                    Some(Frame::Group { clip, opacity, items }) => {
                        let item = Item::Group { clip, opacity, items };
                        match stack.last_mut() {
                            Some(Frame::Group { items, .. }) => items.push(item),
                            _ => doc.items.push(item),
                        }
                    }
                    Some(_) => {}
                    None => return Err("unbalanced tags".into()),
                },
                ("path" | "circle", _) => {
                    let in_mask = matches!(stack.last(), Some(Frame::Mask(_)));
                    let path = if t.name == "path" {
                        parse_path(t.get("d").ok_or("path without d")?)?
                    } else {
                        sk::PathBuilder::from_circle(t.num("cx")?, t.num("cy")?, t.num("r")?).ok_or("bad circle")?
                    };
                    let mask = match t.get("mask").and_then(url_ref) {
                        Some(id) => Some(*mask_ids.get(id).ok_or("unknown mask")?),
                        None => None,
                    };
                    let dash = parse_dash(t.get("stroke-dasharray"));
                    let mut stroke = parse_paint(t.get("stroke"), in_mask)?;
                    let visible_stroke = match stroke {
                        Paint::Line => true,
                        Paint::Role(r) => !matches!(ROLES.get(r).map(|r| r.0), Some("gap" | "active-1" | "active-2" | "active-3")),
                        _ => false,
                    };
                    // Non-blue dotted/dashed references share the guide colour.
                    if !dash.is_empty() && visible_stroke && !in_mask {
                        stroke = Paint::Role(role("guide").ok_or("missing guide colour")?);
                    }
                    let node = Node {
                        path,
                        fill: if t.get("fill").is_none() && in_mask { Paint::None } else { parse_paint(t.get("fill").or(Some("none")), in_mask)? },
                        rule: if t.get("fill-rule") == Some("evenodd") { sk::FillRule::EvenOdd } else { sk::FillRule::Winding },
                        stroke,
                        width: t.get("stroke-width").and_then(|w| w.parse().ok()).unwrap_or(1.0),
                        dash,
                        opacity: t.get("opacity").and_then(|o| o.parse().ok()).unwrap_or(1.0),
                        mask,
                    };
                    match stack.last_mut() {
                        Some(Frame::Mask(i)) => doc.masks.get_mut(*i).ok_or("mask")?.push(node),
                        Some(Frame::Clip(i)) => doc.clips.get_mut(*i).ok_or("clip")?.push(node),
                        Some(Frame::Group { items, .. }) => items.push(Item::Node(node)),
                        _ => doc.items.push(Item::Node(node)),
                    }
                }
                (other, _) => return Err(format!("element <{other}> is not supported")),
            }
        }
        if !stack.is_empty() {
            return Err("unbalanced tags".into());
        }
        Ok(doc)
    }
}

// ---------------------------------------------------------------------------------------------
// Rendering

fn resolve(p: Paint, dark: bool) -> Option<[u8; 3]> {
    let rgb = |i: usize| ROLES.get(i).map(|r| if dark { r.3 } else { r.2 });
    match p {
        Paint::None | Paint::Knockout => None,
        Paint::Fixed(c) => Some(c),
        Paint::Role(r) => rgb(r),
        Paint::Line => role("line").and_then(rgb),
    }
}

fn paint_of(c: [u8; 3], alpha: f32) -> sk::Paint<'static> {
    let mut p = sk::Paint::default();
    p.set_color_rgba8(c[0], c[1], c[2], (alpha.clamp(0.0, 1.0) * 255.0).round() as u8);
    p.anti_alias = true;
    p
}

fn shape_paint(paint: Paint, dark: bool, alpha: f32) -> Option<sk::Paint<'static>> {
    if paint == Paint::Knockout {
        let mut paint = paint_of([0, 0, 0], alpha);
        paint.blend_mode = sk::BlendMode::DestinationOut;
        Some(paint)
    } else {
        resolve(paint, dark).map(|colour| paint_of(colour, alpha))
    }
}

fn draw_node(pm: &mut sk::Pixmap, n: &Node, dark: bool, tf: sk::Transform, doc: &Doc, clip: Option<&sk::Mask>, opacity: f32) {
    let own;
    let mask = if let Some(m) = n.mask {
        own = doc.masks.get(m).and_then(|nodes| {
            let mut sub = sk::Pixmap::new(pm.width(), pm.height())?;
            for node in nodes {
                draw_node(&mut sub, node, dark, tf, doc, None, 1.0);
            }
            let mut own = sk::Mask::from_pixmap(sub.as_ref(), sk::MaskType::Luminance);
            if let Some(parent) = clip {
                for (value, parent) in own.data_mut().iter_mut().zip(parent.data()) {
                    *value = ((u16::from(*value) * u16::from(*parent) + 127) / 255) as u8;
                }
            }
            Some(own)
        });
        own.as_ref()
    } else {
        clip
    };
    let alpha = n.opacity * opacity;
    if let Some(paint) = shape_paint(n.fill, dark, alpha) {
        pm.fill_path(&n.path, &paint, n.rule, tf, mask);
    }
    if let Some(paint) = shape_paint(n.stroke, dark, alpha) {
        let stroke = sk::Stroke {
            width: n.width,
            line_cap: sk::LineCap::Round,
            line_join: sk::LineJoin::Round,
            dash: if n.dash.is_empty() { None } else { sk::StrokeDash::new(n.dash.clone(), 0.0) },
            ..sk::Stroke::default()
        };
        pm.stroke_path(&n.path, &paint, &stroke, tf, mask);
    }
}

fn draw_items(pm: &mut sk::Pixmap, items: &[Item], dark: bool, tf: sk::Transform, doc: &Doc, clip: Option<&sk::Mask>, opacity: f32) {
    for item in items {
        match item {
            Item::Node(n) => draw_node(pm, n, dark, tf, doc, clip, opacity),
            Item::Group { clip: c, opacity: o, items } => {
                let mask = c.and_then(|c| doc.clips.get(c)).and_then(|nodes| {
                    let mut m = sk::Mask::new(pm.width(), pm.height())?;
                    for n in nodes {
                        m.fill_path(&n.path, n.rule, true, tf);
                    }
                    Some(m)
                });
                draw_items(pm, items, dark, tf, doc, mask.as_ref().or(clip), opacity * o);
            }
        }
    }
}

/// Rasterise `doc` (a 64 × 64 master) into a square `px` × `px` premultiplied RGBA image.
pub fn render(doc: &Doc, dark: bool, px: u32) -> Option<sk::Pixmap> {
    let mut pm = sk::Pixmap::new(px, px)?;
    let s = px as f32 / 64.0;
    draw_items(&mut pm, &doc.items, dark, sk::Transform::from_scale(s, s), doc, None, 1.0);
    Some(pm)
}

// ---------------------------------------------------------------------------------------------
// Drawing

/// The masters, parsed once. One that fails to parse (a bug in the artwork) is logged and draws
/// nothing.
fn docs() -> &'static HashMap<&'static str, Doc> {
    static DOCS: OnceLock<HashMap<&'static str, Doc>> = OnceLock::new();
    DOCS.get_or_init(|| {
        MASTERS
            .iter()
            .filter_map(|(name, src)| match Doc::parse(src) {
                Ok(doc) => Some((*name, doc)),
                Err(e) => {
                    log::error!("icon {name}: {e}");
                    None
                }
            })
            .collect()
    })
}

/// Whether there is an icon called `name`.
pub fn exists(name: &str) -> bool {
    docs().contains_key(name)
}

#[derive(Clone, Default)]
struct Cache(Arc<Mutex<HashMap<(&'static str, bool, u32), TextureHandle>>>);

/// Draw icon `name` filling `r`, for a dark or light surface, at `opacity` (0 to 1).
pub fn paint(p: &Painter, r: Rect, name: &str, dark: bool, opacity: f32) {
    let Some((key, doc)) = docs().get_key_value(name) else { return };
    let px = (r.width().max(r.height()) * p.ctx().pixels_per_point()).round().clamp(1.0, 512.0) as u32;
    let cache = p.ctx().data_mut(|d| d.get_temp_mut_or_insert_with(egui::Id::new("resin_icon_cache"), Cache::default).clone());
    let Ok(mut textures) = cache.0.lock() else { return };
    let texture = match textures.get(&(*key, dark, px)) {
        Some(t) => t.clone(),
        None => {
            let Some(pm) = render(doc, dark, px) else { return };
            // tiny-skia stores premultiplied RGBA, as egui's texture upload expects.
            let image = egui::ColorImage::from_rgba_premultiplied([px as usize, px as usize], pm.data());
            let t = p.ctx().load_texture(format!("icon-{key}-{dark}-{px}"), image, TextureOptions::LINEAR);
            textures.insert((*key, dark, px), t.clone());
            t
        }
    };
    let tint = Color32::from_white_alpha((opacity.clamp(0.0, 1.0) * 255.0).round() as u8);
    p.image(texture.id(), r, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), tint);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn coverage(pm: &sk::Pixmap) -> usize {
        pm.data().chunks(4).filter(|p| p.get(3).is_some_and(|a| *a > 0)).count()
    }

    #[test]
    fn every_master_parses_and_draws_in_both_themes() {
        for (name, src) in MASTERS {
            let doc = Doc::parse(src).unwrap_or_else(|e| panic!("{name}: {e}"));
            for dark in [false, true] {
                for px in [16, 26, 30] {
                    let pm = render(&doc, dark, px).unwrap();
                    assert!(coverage(&pm) > 0, "{name} at {px} px draws nothing");
                }
            }
        }
        assert_eq!(docs().len(), MASTERS.len());
    }

    #[test]
    fn dark_changes_colour_not_shape() {
        let doc = Doc::parse(include_str!("../icons/move.svg")).unwrap();
        let (l, d) = (render(&doc, false, 32).unwrap(), render(&doc, true, 32).unwrap());
        assert_eq!(coverage(&l), coverage(&d));
        assert_ne!(l.data(), d.data());
    }

    #[test]
    fn hostile_artwork_is_an_error_not_a_panic() {
        for src in [
            "<svg><path d=\"M0 0L\"/></svg>",
            "<svg><path d=\"X1 2\"/></svg>",
            "<svg><path fill=\"#123456\" d=\"M0 0L1 1Z\"/></svg>",
            "<svg><g>",
            "<svg><circle cx=\"a\" cy=\"1\" r=\"1\"/></svg>",
            "<svg><path d=\"M0 0L1 1\" mask=\"url(#nope)\"/></svg>",
            "<svg><rect/></svg>",
            "<svg><path d='M0 0L1 1",
            "<svg><path d=\"\"/></svg>",
        ] {
            assert!(Doc::parse(src).is_err(), "{src}");
        }
    }
}
