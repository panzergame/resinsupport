//! `cargo xtask design-export`: builds `design/resinsupport.figcraft` from the code, once; from
//! then on the design file owns the tokens (`cargo xtask design-pull`).
//!
//! - Variables: the code's token regions (`design_tokens`): Colour (Light, Dark), Metrics, and
//!   Icon (Light, Dark, Source: the colour the masters write for each role).
//! - Icons: each SVG master, its colours bound to the Icon variables, a component `icon/<name>`.
//! - Screens: the design capture (`crates/ui-egui/src/design_capture.rs`) of every screen in both
//!   themes. Each control the kit marked becomes an instance of a component made of what that
//!   control painted (its own shapes, its children's left out), its texts as text properties;
//!   colours that are a Colour token in both themes are bound to it.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};

use crate::design_tokens::{self, CodeToken, LINE_SOURCE, SINGLE_MODE, SOURCE_MODE, Value as Tok};
use crate::figcraft::{Cli, named, reference, step};

pub const DESIGN_FILE: &str = "design/resinsupport.figcraft";
pub const ICONS_DIR: &str = "crates/ui-egui/icons";
/// The sentinel the bootstrap gives mask white, so it is never taken for the `gap` role's white.
const MASK_WHITE: &str = "#fefefe";

/// The work folder: captures, scripts and scratch files.
pub fn work_dir(root: &Path) -> PathBuf {
    root.join("target/design")
}

/// The kit function that draws each kind of control, for the components' descriptions.
const KINDS: &[(&str, &str)] = &[
    ("action_row", "ui_kit::action_row"),
    ("app_bar", "toolbar::app_bar"),
    ("app_bar_button", "toolbar::app_bar (a button)"),
    ("app_bar_menu", "toolbar::app_bar (the File menu's button)"),
    ("browser_header", "ui_kit::Card (header of a Buttons::Fold card)"),
    ("browser_row", "ui_kit::browser_row, ui_kit::rename_row"),
    ("button", "ui_kit::button"),
    ("caption", "ui_kit::caption, ui_kit::warning_caption"),
    ("card", "ui_kit::Card"),
    ("card_footer", "ui_kit::Card (footer)"),
    ("card_header", "ui_kit::Card (header)"),
    ("check_row", "ui_kit::check_row"),
    ("checkbox", "ui_kit::checkbox"),
    ("close_button", "ui_kit::Card (close button)"),
    ("confirm", "ui_kit::Confirm"),
    ("dropdown", "ui_kit::dropdown, ui_kit::dropdown_menu"),
    ("dropdown_item", "ui_kit::dropdown_item"),
    ("figure", "ui_kit::figure_rows"),
    ("fold_button", "ui_kit::Card (a Buttons::Fold card's chevron)"),
    ("hint", "ui_kit::hint"),
    ("icon_button", "ui_kit::icon_button"),
    ("menu", "ui_kit::menu_list"),
    ("menu_item", "ui_kit::MenuItem"),
    ("menu_separator", "ui_kit::menu_separator"),
    ("multiline_text_field", "ui_kit::multiline_text_field"),
    ("number_field", "ui_kit::number_field"),
    ("page", "ui_kit::page_list (a page)"),
    ("primary_button", "ui_kit::Card (footer's primary action)"),
    ("progress", "ui_kit::progress"),
    ("ribbon", "toolbar::ribbon"),
    ("ribbon_button", "toolbar::ribbon (a button)"),
    ("ribbon_group", "toolbar::ribbon (a group)"),
    ("ribbon_group_menu", "toolbar::ribbon (a group's menu)"),
    ("ribbon_tab", "toolbar::ribbon (a tab)"),
    ("row", "ui_kit::row, ui_kit::sized_row, ui_kit::value_row"),
    ("secondary_button", "ui_kit::Card (footer's Cancel or Restore defaults)"),
    ("section", "ui_kit::section"),
    ("segmented", "ui_kit::segmented"),
    ("slider", "ui_kit::slider"),
    ("status_line", "ui_kit::status_line"),
    ("text_field", "ui_kit::text_field"),
    ("workspace", "toolbar::ribbon (the workspace card)"),
];

/// How deep a kind sits when two marks share a rectangle: the lower contains the higher.
fn rank(kind: &str) -> u8 {
    match kind {
        "card" | "confirm" | "menu" | "app_bar" | "ribbon" => 0,
        "ribbon_group" | "figure" => 1,
        "card_header" | "card_footer" | "section" | "row" | "check_row" => 2,
        "icon_light" | "icon_dark" => 4,
        _ => 3,
    }
}

type Rect = [f64; 4];

fn rect_of(v: &Value) -> Option<Rect> {
    let a = v.as_array()?;
    let at = |i: usize| a.get(i).and_then(Value::as_f64);
    Some([at(0)?, at(1)?, at(2)?, at(3)?])
}

fn contains(outer: Rect, inner: Rect, slack: f64) -> bool {
    inner[0] >= outer[0] - slack && inner[1] >= outer[1] - slack && inner[2] <= outer[2] + slack && inner[3] <= outer[3] + slack
}

fn area(r: Rect) -> f64 {
    (r[2] - r[0]).max(0.0) * (r[3] - r[1]).max(0.0)
}

/// Where a shape sits, for placing it among the marks.
fn shape_rect(s: &Value) -> Option<Rect> {
    if let Some(r) = rect_of(&s["rect"]) {
        return Some(r);
    }
    let pts = s["points"].as_array()?;
    let mut r = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
    for p in pts {
        let (x, y) = (p.get(0)?.as_f64()?, p.get(1)?.as_f64()?);
        r = [r[0].min(x), r[1].min(y), r[2].max(x), r[3].max(y)];
    }
    (r[0] <= r[2]).then_some(r)
}

fn rgba(v: &Value) -> Option<[u8; 4]> {
    let a = v.as_array()?;
    let at = |i: usize| a.get(i).and_then(Value::as_u64).and_then(|c| u8::try_from(c).ok());
    Some([at(0)?, at(1)?, at(2)?, at(3)?])
}

struct Mark {
    kind: String,
    label: String,
    rect: Rect,
}

struct Layer {
    order: String,
    shapes: Vec<Value>,
    marks: Vec<Mark>,
}

struct Shot {
    name: String,
    dark: bool,
    size: (f64, f64),
    layers: Vec<Layer>,
}

/// egui paints a number field's value and its unit as two texts side by side: one text, so an
/// override changes both and keeps them together. Decided on geometry alone, so a screen's two
/// themes merge alike.
fn merge_texts(shapes: Vec<Value>) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::with_capacity(shapes.len());
    for s in shapes {
        if let (Some(prev), Some(r)) = (out.last_mut(), rect_of(&s["rect"]))
            && s["t"] == "text"
            && prev["t"] == "text"
            && prev["size"] == s["size"]
            && prev["family"] == s["family"]
            && let Some(p) = rect_of(&prev["rect"])
            && (p[1] - r[1]).abs() <= 0.5
            && (p[3] - r[3]).abs() <= 0.5
            && r[0] >= p[2] - 1.0
            && r[0] <= p[2] + 2.0
        {
            let text = format!("{}{}", prev["text"].as_str().unwrap_or_default(), s["text"].as_str().unwrap_or_default());
            prev["text"] = json!(text);
            prev["rect"] = json!([p[0], p[1], r[2], p[3]]);
            continue;
        }
        out.push(s);
    }
    out
}

fn read_shot(path: &Path) -> Result<Shot, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let v: Value = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut layers: Vec<(String, Layer)> = v["layers"]
        .as_array()
        .map(|ls| {
            ls.iter()
                .map(|l| {
                    let id = l["id"].as_str().unwrap_or_default().to_string();
                    let layer = Layer {
                        order: l["order"].as_str().unwrap_or_default().to_string(),
                        shapes: merge_texts(l["shapes"].as_array().cloned().unwrap_or_default()),
                        marks: Vec::new(),
                    };
                    (id, layer)
                })
                .collect()
        })
        .unwrap_or_default();
    for m in v["marks"].as_array().into_iter().flatten() {
        let Some(rect) = rect_of(&m["rect"]) else { continue };
        let id = m["layer"].as_str().unwrap_or_default();
        if let Some((_, layer)) = layers.iter_mut().find(|(l, _)| l == id) {
            layer.marks.push(Mark {
                kind: m["kind"].as_str().unwrap_or_default().into(),
                label: m["label"].as_str().unwrap_or_default().into(),
                rect,
            });
        }
    }
    Ok(Shot {
        name: v["name"].as_str().unwrap_or_default().to_string(),
        dark: v["theme"] == "Dark",
        size: (v["size"][0].as_f64().unwrap_or(1280.0), v["size"][1].as_f64().unwrap_or(800.0)),
        layers: layers.into_iter().map(|(_, l)| l).collect(),
    })
}

/// A layer's marks as a tree, and which node each shape belongs to.
struct Tree {
    /// Node 0 is the layer itself; node i + 1 is mark i.
    children: Vec<Vec<usize>>,
    shapes: Vec<Vec<usize>>,
}

fn tree(layer: &Layer) -> Tree {
    let n = layer.marks.len() + 1;
    let mut parent = vec![0; n];
    for (i, m) in layer.marks.iter().enumerate() {
        let mut best: Option<(usize, f64)> = None;
        for (j, o) in layer.marks.iter().enumerate() {
            if i == j || !contains(o.rect, m.rect, 0.5) {
                continue;
            }
            let (ao, am) = (area(o.rect), area(m.rect));
            let outer = ao > am + 0.5 || ((ao - am).abs() <= 0.5 && (rank(&o.kind) < rank(&m.kind) || (rank(&o.kind) == rank(&m.kind) && j < i)));
            if outer && best.is_none_or(|(_, a)| ao < a) {
                best = Some((j, ao));
            }
        }
        if let (Some(p), Some((j, _))) = (parent.get_mut(i + 1), best) {
            *p = j + 1;
        }
    }
    let mut children = vec![Vec::new(); n];
    for (i, p) in parent.iter().enumerate().skip(1) {
        if let Some(c) = children.get_mut(*p) {
            c.push(i);
        }
    }
    let mut shapes = vec![Vec::new(); n];
    for (si, s) in layer.shapes.iter().enumerate() {
        let Some(r) = shape_rect(s) else { continue };
        let centre = [(r[0] + r[2]) / 2.0, (r[1] + r[3]) / 2.0];
        let mut best: Option<(usize, f64)> = None;
        for (j, m) in layer.marks.iter().enumerate() {
            let inside = contains(m.rect, [centre[0], centre[1], centre[0], centre[1]], 0.5)
                && r[2] - r[0] <= m.rect[2] - m.rect[0] + 6.0
                && r[3] - r[1] <= m.rect[3] - m.rect[1] + 6.0;
            // Icons own only their picture.
            let fits = !m.kind.starts_with("icon_") || s["t"] == "image";
            if inside && fits && best.is_none_or(|(_, a)| area(m.rect) < a) {
                best = Some((j + 1, area(m.rect)));
            }
        }
        if let Some(list) = shapes.get_mut(best.map_or(0, |(j, _)| j)) {
            list.push(si);
        }
    }
    Tree { children, shapes }
}

/// Colour tokens, to bind a captured colour to the variable that gives it in both themes.
struct Palette {
    /// (name, light, dark), in the code's order.
    colours: Vec<(String, [u8; 4], [u8; 4])>,
}

impl Palette {
    fn bind(&self, light: [u8; 4], dark: [u8; 4]) -> Option<&str> {
        let near = |a: [u8; 4], b: [u8; 4]| a.iter().zip(b).all(|(x, y)| x.abs_diff(y) <= 1);
        self.colours.iter().find(|(_, l, d)| near(*l, light) && near(*d, dark)).map(|(n, _, _)| n.as_str())
    }
}

/// A paint for FigCraft: the bound variable, or the colour.
fn paint(c: [u8; 4], partner: Option<[u8; 4]>, dark: bool, palette: &Palette) -> Value {
    let bound = partner.and_then(|p| if dark { palette.bind(p, c) } else { palette.bind(c, p) });
    match bound {
        Some(name) => json!({ "$var": name }),
        None => json!(design_tokens::hex_of(c)),
    }
}

/// What a shape is, for telling components apart: kind, colours (variable or literal), corners,
/// type size.
fn signature(s: &Value, partner: Option<&Value>, dark: bool, palette: &Palette) -> String {
    let p = |k: &str, sub: Option<&str>| {
        let get = |v: &Value| match sub {
            Some(sub) => rgba(&v[k][sub]),
            None => rgba(&v[k]),
        };
        get(s).map_or("-".into(), |c| paint(c, partner.and_then(get), dark, palette).to_string())
    };
    match s["t"].as_str().unwrap_or_default() {
        "text" => format!("text {} {} {}", s["size"], s["family"], p("colour", None)),
        t => format!("{t} {} {} {} {}", p("fill", None), p("stroke", Some("colour")), s["radius"], s["stroke"]["width"]),
    }
}

/// FigCraft's constraint for a child at `lo..hi` of a parent `0..size`.
fn constraint(lo: f64, hi: f64, size: f64, start: &str, end: &str, both: &str) -> String {
    let (gap_lo, gap_hi) = (lo, size - hi);
    if gap_lo <= 2.0 && gap_hi <= 2.0 {
        both.into()
    } else if (gap_lo - gap_hi).abs() <= 1.5 && gap_lo > 2.0 {
        "center".into()
    } else if lo > size / 2.0 {
        end.into()
    } else {
        start.into()
    }
}

struct Script {
    steps: Vec<Value>,
    next: usize,
}

impl Script {
    fn fresh(&mut self, prefix: &str) -> String {
        self.next += 1;
        format!("{prefix}{}", self.next)
    }
    fn push(&mut self, command: &str, params: Value) {
        self.steps.push(step(command, params));
    }
    /// `node.create` with these params; the new node's name in the script.
    fn create(&mut self, params: Value) -> String {
        let name = self.fresh("n");
        self.steps.push(named(&name, "node.create", params));
        name
    }
}

/// The messages' English text → their ids, to name text layers by the message they show.
fn message_ids(root: &Path) -> HashMap<String, String> {
    let text = std::fs::read_to_string(root.join("assets/locales/en.json")).unwrap_or_default();
    let catalog: BTreeMap<String, String> = serde_json::from_str(&text).unwrap_or_default();
    let mut ids = HashMap::new();
    for (id, msg) in catalog {
        ids.entry(msg).or_insert(id);
    }
    ids
}

struct Ctx<'a> {
    palette: &'a Palette,
    messages: &'a HashMap<String, String>,
    /// The viewport's gradient, light and dark.
    viewport: [([u8; 4], [u8; 4]); 2],
    /// Icon components by name: (script name, its node id).
    icons: &'a HashMap<String, u64>,
}

/// Draw `s` under `parent`, offset by `origin`; `size` is the parent's, for constraints in a
/// component (`None` outside one). `slots`: the component's texts are each centred in a slot
/// that grows with it (a segmented strip's segments). The new node's name, if it drew one.
fn draw_shape(
    sc: &mut Script,
    cx: &Ctx,
    s: &Value,
    partner: Option<&Value>,
    dark: bool,
    parent: &Value,
    origin: (f64, f64),
    size: Option<(f64, f64)>,
    slots: bool,
) -> Option<String> {
    let colour = |k: &str| rgba(&s[k]);
    let partner_colour = |k: &str| partner.and_then(|p| rgba(&p[k]));
    let pt = |k: &str| paint(colour(k).unwrap_or([0, 0, 0, 0]), partner_colour(k), dark, cx.palette);
    let stroke_colour = rgba(&s["stroke"]["colour"]);
    let partner_stroke = partner.and_then(|p| rgba(&p["stroke"]["colour"]));
    let r = shape_rect(s)?;
    let (x, y, w, h) = (r[0] - origin.0, r[1] - origin.1, r[2] - r[0], r[3] - r[1]);
    let mut props = serde_json::Map::new();
    if let Some((pw, ph)) = size {
        let mut horizontal = constraint(x, x + w, pw, "left", "right", "leftRight");
        // A segment's fill and dividers move with the segments.
        if slots && horizontal != "leftRight" {
            horizontal = "scale".into();
        }
        let vertical = constraint(y, y + h, ph, "top", "bottom", "topBottom");
        props.insert("constraints".into(), json!({ "horizontal": horizontal, "vertical": vertical }));
    }
    let set_stroke = |props: &mut serde_json::Map<String, Value>| {
        if let Some(c) = stroke_colour {
            props.insert("stroke".into(), paint(c, partner_stroke, dark, cx.palette));
            props.insert("strokeWeight".into(), s["stroke"]["width"].clone());
        }
    };
    let t = s["t"].as_str().unwrap_or_default();
    let params = match t {
        "rect" | "ellipse" => {
            props.insert("fill".into(), if colour("fill").is_some() { pt("fill") } else { json!("none") });
            set_stroke(&mut props);
            if t == "rect" {
                let radius: Vec<f64> = s["radius"].as_array().map(|a| a.iter().filter_map(Value::as_f64).collect()).unwrap_or_default();
                if radius.iter().any(|r| *r > 0.0) {
                    props.insert("cornerRadius".into(), json!(radius));
                }
                let kind = match s["stroke_kind"].as_str() {
                    Some("Outside") => "outside",
                    Some("Middle") => "center",
                    _ => "inside",
                };
                props.insert("strokeAlign".into(), json!(kind));
                if let Some(blur) = s["blur"].as_f64().filter(|b| *b > 0.0) {
                    props.insert("effects".into(), json!([{ "type": "layerBlur", "radius": blur / 2.0 }]));
                }
            }
            let name = if s["blur"].as_f64().is_some_and(|b| b > 0.0) { "shadow" } else { t };
            json!({ "type": if t == "rect" { "rectangle" } else { "ellipse" }, "name": name, "x": x, "y": y, "width": w.max(0.01), "height": h.max(0.01), "parent": parent, "props": props, "select": false })
        }
        "path" => {
            let pts: Vec<[f64; 2]> =
                s["points"].as_array()?.iter().filter_map(|p| Some([p.get(0)?.as_f64()? - origin.0, p.get(1)?.as_f64()? - origin.1])).collect();
            if pts.len() < 2 {
                return None;
            }
            props.insert("fill".into(), if colour("fill").is_some() { pt("fill") } else { json!("none") });
            set_stroke(&mut props);
            json!({ "type": "vector", "name": "line", "points": pts, "closed": s["closed"], "x": 0, "y": 0, "parent": parent, "props": props, "select": false })
        }
        "text" => {
            let text = s["text"].as_str().unwrap_or_default();
            if text.trim().is_empty() {
                return None;
            }
            let rows = text.lines().count().max(1) as f64;
            let (family, style) = if s["family"] == "Monospace" { ("Hack", "Regular") } else { ("Ubuntu", "Light") };
            props.insert("fontFamily".into(), json!(family));
            props.insert("fontStyle".into(), json!(style));
            props.insert("fontSize".into(), s["size"].clone());
            props.insert("lineHeight".into(), json!((h / rows).max(1.0)));
            if let Some(spacing) = s["spacing"].as_f64().filter(|v| *v != 0.0) {
                props.insert("letterSpacing".into(), json!(spacing));
            }
            props.insert("fill".into(), pt("colour"));
            let mut params = json!({ "type": "text", "text": text, "x": x, "y": y, "parent": parent, "select": false,
                "name": cx.messages.get(text).cloned().unwrap_or_else(|| "text".into()) });
            match size {
                // Centred in a slot: as wide as it can be around that centre, which scales with the
                // component.
                Some((pw, _)) if slots => {
                    let centre = x + w / 2.0;
                    let half = centre.min(pw - centre).max(w / 2.0);
                    props.insert("textAlign".into(), json!("center"));
                    props.insert("textAutoResize".into(), json!("height"));
                    props.insert("constraints".into(), json!({ "horizontal": "scale", "vertical": "center" }));
                    params["x"] = json!(centre - half);
                    params["width"] = json!(half * 2.0);
                }
                // Centred in a component: across it, centred, so a longer override stays centred.
                Some((pw, _)) if ((x + w / 2.0) - pw / 2.0).abs() <= 1.5 && x > 2.0 => {
                    props.insert("textAlign".into(), json!("center"));
                    props.insert("textAutoResize".into(), json!("height"));
                    props.insert(
                        "constraints".into(),
                        json!({ "horizontal": "leftRight", "vertical": props.get("constraints").map_or(json!("center"), |c| c["vertical"].clone()) }),
                    );
                    params["x"] = json!(0);
                    params["width"] = json!(pw);
                }
                _ if rows > 1.0 => {
                    props.insert("textAutoResize".into(), json!("height"));
                    params["width"] = json!(w + 1.0);
                }
                // Right-aligned in a component's right half (a menu's shortcut): a longer override
                // grows leftwards.
                Some((pw, _)) if x > pw / 2.0 => {
                    props.insert("textAlign".into(), json!("right"));
                    props.insert("textAutoResize".into(), json!("height"));
                    params["x"] = json!(0);
                    params["width"] = json!(x + w);
                }
                _ => {
                    props.insert("textAutoResize".into(), json!("widthAndHeight"));
                }
            }
            params["props"] = Value::Object(props);
            params
        }
        "image" => {
            props.insert("fill".into(), json!("#80808040"));
            json!({ "type": "rectangle", "name": "image", "x": x, "y": y, "width": w.max(0.01), "height": h.max(0.01), "parent": parent, "props": props, "select": false })
        }
        "callback" => {
            let (top, bottom) = cx.viewport[usize::from(dark)];
            props.insert(
                "fills".into(),
                json!([{ "type": "linearGradient", "stops": [design_tokens::hex_of(top), design_tokens::hex_of(bottom)], "from": [0.5, 0], "to": [0.5, 1] }]),
            );
            json!({ "type": "rectangle", "name": "viewport (wgpu: the model, supports and plate)", "x": x, "y": y, "width": w, "height": h, "parent": parent, "props": props, "select": false })
        }
        _ => return None,
    };
    Some(sc.create(params))
}

/// A component per distinct look of a kind of control.
struct Components {
    /// signature → (script name of the main, its text shapes' count, the main's texts).
    by_sig: HashMap<String, (String, Vec<String>)>,
    /// Kind → how many variants so far, for names and placing.
    count: BTreeMap<String, usize>,
    /// Where the next row of each kind goes on the Components page.
    rows: BTreeMap<String, (f64, f64)>,
    next_row: f64,
}

/// Emit `node` of `tree` (and its children) under `parent`, the parent node's top left being
/// `origin`.
#[allow(clippy::too_many_arguments)]
fn emit(
    sc: &mut Script,
    cx: &Ctx,
    comps: &mut Components,
    page_components: u64,
    layer: &Layer,
    partner: Option<&Layer>,
    tr: &Tree,
    node: usize,
    dark: bool,
    parent: &Value,
    origin: (f64, f64),
) {
    let Some(mark) = node.checked_sub(1).and_then(|i| layer.marks.get(i)) else { return };
    let r = mark.rect;
    let (x, y, w, h) = (r[0] - origin.0, r[1] - origin.1, r[2] - r[0], r[3] - r[1]);
    if w <= 0.0 || h <= 0.0 {
        return;
    }
    if let Some(theme) = mark.kind.strip_prefix("icon_") {
        let Some(id) = cx.icons.get(&mark.label) else { return };
        let inst = sc.fresh("i");
        sc.steps.push(named(&inst, "instance.create", json!({ "component": id, "x": x, "y": y, "parent": parent })));
        sc.push("transform.scale", json!({ "ids": [reference(&inst)], "factor": w.max(h) / 64.0 }));
        let mode = if theme == "dark" { "Dark" } else { "Light" };
        sc.push("variable.setMode", json!({ "ids": [reference(&inst)], "collection": "Icon", "mode": mode }));
        let tint = tr.shapes.get(node).and_then(|s| s.first()).and_then(|i| layer.shapes.get(*i)).and_then(|s| rgba(&s["tint"]));
        if let Some(alpha) = tint.map(|t| t[3]).filter(|a| *a < 250) {
            sc.push("node.set", json!({ "ids": [reference(&inst)], "props": { "opacity": f64::from(alpha) / 255.0 } }));
        }
        return;
    }
    let own: Vec<usize> = tr.shapes.get(node).cloned().unwrap_or_default();
    let kids: Vec<usize> = tr.children.get(node).cloned().unwrap_or_default();
    let title = if mark.label.is_empty() { mark.kind.clone() } else { format!("{}: {}", mark.kind, mark.label) };
    let pair = |i: usize| (layer.shapes.get(i), partner.and_then(|p| p.shapes.get(i)));
    // The control's own look, as an instance.
    let instance = if own.is_empty() {
        None
    } else {
        let sig: Vec<String> = own.iter().filter_map(|i| pair(*i).0.map(|s| signature(s, pair(*i).1, dark, cx.palette))).collect();
        let sig = format!("{}|{}", mark.kind, sig.join(";"));
        if !comps.by_sig.contains_key(&sig) {
            let n = comps.count.entry(mark.kind.clone()).or_insert(0);
            *n += 1;
            let comp_name = format!("{}/{}", mark.kind, n);
            let next_row = comps.next_row;
            let (cx_pos, cy_pos) = *comps.rows.entry(mark.kind.clone()).or_insert((0.0, next_row));
            if cx_pos == 0.0 {
                comps.next_row += h.max(28.0) + 48.0;
            }
            comps.rows.insert(mark.kind.clone(), (cx_pos + w + 32.0, cy_pos));
            let description = KINDS.iter().find(|(k, _)| *k == mark.kind).map_or("", |(_, f)| *f);
            sc.push("page.select", json!({ "id": page_components }));
            let main = sc.create(json!({ "type": "component", "name": comp_name, "x": cx_pos, "y": cy_pos, "width": w, "height": h,
                "props": { "fill": "none", "clip": false, "description": format!("Drawn by {description}.") }, "select": false }));
            let mut texts = Vec::new();
            for i in &own {
                let (Some(s), p) = pair(*i) else { continue };
                if let Some(node_name) = draw_shape(sc, cx, s, p, dark, &reference(&main), (r[0], r[1]), Some((w, h)), mark.kind == "segmented")
                    && s["t"] == "text"
                {
                    let prop = format!("text {}", texts.len() + 1);
                    sc.push("component.addProperty", json!({ "id": reference(&main), "name": prop, "type": "text", "layer": reference(&node_name) }));
                    texts.push(s["text"].as_str().unwrap_or_default().to_string());
                }
            }
            comps.by_sig.insert(sig.clone(), (main, texts));
        }
        comps.by_sig.get(&sig).map(|(main, texts)| (main.clone(), texts.clone()))
    };
    let shown_texts: Vec<String> = own
        .iter()
        .filter_map(|i| layer.shapes.get(*i))
        .filter(|s| s["t"] == "text" && s["text"].as_str().is_some_and(|t| !t.trim().is_empty()))
        .filter_map(|s| s["text"].as_str().map(String::from))
        .collect();
    let place_instance = |sc: &mut Script, parent: &Value, x: f64, y: f64, name: &str| {
        let Some((main, texts)) = &instance else { return };
        let inst = sc.fresh("i");
        sc.steps.push(named(&inst, "instance.create", json!({ "component": reference(main), "x": x, "y": y, "parent": parent })));
        sc.push("node.set", json!({ "ids": [reference(&inst)], "props": { "width": w, "height": h, "name": name } }));
        let overrides: serde_json::Map<String, Value> = shown_texts
            .iter()
            .zip(texts)
            .enumerate()
            .filter(|(_, (shown, main))| shown != main)
            .map(|(i, (shown, _))| (format!("text {}", i + 1), json!(shown)))
            .collect();
        if !overrides.is_empty() {
            sc.push("instance.setProperties", json!({ "ids": [reference(&inst)], "properties": overrides }));
        }
    };
    if kids.is_empty() {
        place_instance(sc, parent, x, y, &title);
        return;
    }
    let frame = sc.create(json!({ "type": "frame", "name": title, "x": x, "y": y, "width": w, "height": h, "parent": parent,
        "props": { "fill": "none", "clip": false }, "select": false }));
    place_instance(sc, &reference(&frame), 0.0, 0.0, &mark.kind);
    for k in kids {
        emit(sc, cx, comps, page_components, layer, partner, tr, k, dark, &reference(&frame), (r[0], r[1]));
    }
}

/// The layer's name: its card's title, else what kind of layer it is.
fn layer_name(layer: &Layer, tr: &Tree) -> String {
    let top = tr.children.first().into_iter().flatten().filter_map(|c| c.checked_sub(1).and_then(|i| layer.marks.get(i)));
    for m in top {
        if matches!(m.kind.as_str(), "card" | "confirm") {
            return format!("{}: {}", m.kind, m.label);
        }
        if m.kind == "menu" {
            return "menu".into();
        }
    }
    match layer.order.as_str() {
        "Background" => "window".into(),
        "Tooltip" => "tooltip".into(),
        "Foreground" => "popup".into(),
        other => other.to_lowercase(),
    }
}

/// The code's viewport gradient, light then dark (outside the token regions: the viewport is
/// drawn by the GPU, not from the design file).
fn viewport_colours(root: &Path) -> [([u8; 4], [u8; 4]); 2] {
    let text = std::fs::read_to_string(root.join("crates/ui-egui/src/theme.rs")).unwrap_or_default();
    let find = |key: &str| -> Vec<[u8; 4]> {
        text.lines().filter_map(|l| l.trim().strip_prefix(key)).filter_map(|v| design_tokens::parse_colour(v.trim().trim_end_matches(','))).collect()
    };
    let (top, bottom) = (find("viewport_top:"), find("viewport_bottom:"));
    let at = |v: &Vec<[u8; 4]>, i: usize| v.get(i).copied().unwrap_or([128, 128, 128, 255]);
    // `for_dark` spells the dark theme first.
    [(at(&top, 1), at(&bottom, 1)), (at(&top, 0), at(&bottom, 0))]
}

fn variables_script(tokens: &[CodeToken]) -> Vec<Value> {
    let mut steps = vec![
        step("variable.createCollection", json!({ "name": "Colour", "modes": ["Light", "Dark"] })),
        step("variable.createCollection", json!({ "name": "Metrics", "modes": [SINGLE_MODE] })),
        step("variable.createCollection", json!({ "name": "Icon", "modes": ["Light", "Dark", SOURCE_MODE] })),
    ];
    for t in tokens {
        let values: serde_json::Map<String, Value> = t
            .values
            .iter()
            .map(|(mode, v)| {
                let v = match v {
                    Tok::Colour(c) => json!(design_tokens::hex_of(*c)),
                    Tok::Number(n) => json!(n),
                };
                (mode.clone(), v)
            })
            .collect();
        let kind = if matches!(t.values.first(), Some((_, Tok::Number(_)))) { "number" } else { "color" };
        let description = match (&t.ty, &t.source) {
            (Some(ty), _) => format!("theme::metrics::{} ({ty})", t.name),
            (_, Some(src)) => format!("Icon role; the masters write {src}{}", if t.name == "line" { " as currentColor" } else { "" }),
            _ => format!("theme::Tokens::{}", t.name),
        };
        steps.push(step(
            "variable.create",
            json!({ "collection": t.collection, "name": t.name, "type": kind, "values": values, "description": description }),
        ));
    }
    steps
}

/// The tokens page: every variable, shown with what it holds, in each mode.
fn tokens_page(sc: &mut Script, tokens: &[CodeToken]) {
    let mut column = 0.0;
    for (collection, modes) in [("Colour", vec!["Light", "Dark"]), ("Icon", vec!["Light", "Dark", SOURCE_MODE]), ("Metrics", vec![SINGLE_MODE])] {
        let list: Vec<&CodeToken> = tokens.iter().filter(|t| t.collection == collection).collect();
        for mode in modes {
            let height = 48.0 + list.len() as f64 * 28.0;
            let frame = sc
                .create(json!({ "type": "frame", "name": format!("{collection} · {mode}"), "x": column, "y": 0, "width": 280, "height": height,
                "props": { "fill": if mode == "Dark" { "#1c1f24" } else { "#ffffff" } }, "select": false }));
            sc.push("variable.setMode", json!({ "ids": [reference(&frame)], "collection": collection, "mode": mode }));
            let ink = if mode == "Dark" { "#dee2e8" } else { "#1b2430" };
            sc.create(
                json!({ "type": "text", "text": format!("{collection} · {mode}"), "x": 12, "y": 12, "parent": reference(&frame), "select": false,
                "props": { "fontFamily": "Ubuntu", "fontStyle": "Medium", "fontSize": 14, "fill": ink } }),
            );
            for (i, t) in list.iter().enumerate() {
                let y = 44.0 + i as f64 * 28.0;
                let swatch = if collection == "Metrics" {
                    let bar = sc.create(json!({ "type": "rectangle", "x": 12, "y": y + 6.0, "width": 10, "height": 8, "parent": reference(&frame),
                        "props": { "fill": "#3a88ee" }, "select": false }));
                    sc.push("variable.bind", json!({ "ids": [reference(&bar)], "field": "width", "variable": t.name }));
                    104.0
                } else {
                    sc.create(json!({ "type": "rectangle", "x": 12, "y": y, "width": 20, "height": 20, "parent": reference(&frame),
                        "props": { "fill": { "$var": t.name }, "stroke": "#80808080", "strokeWeight": 1, "cornerRadius": 4 }, "select": false }));
                    40.0
                };
                sc.create(json!({ "type": "text", "text": t.name, "x": swatch, "y": y + 3.0, "parent": reference(&frame), "select": false,
                    "props": { "fontFamily": "Hack", "fontStyle": "Regular", "fontSize": 11, "fill": ink } }));
            }
            column += 300.0;
        }
    }
}

fn run_capture(root: &Path, dir: &Path) -> Result<(), String> {
    eprintln!("  capturing the screens (cargo test design_capture)");
    let status = Command::new(env!("CARGO"))
        .args(["test", "--workspace", "--lib", "-q", "design_capture", "--", "--ignored", "--exact", "design_capture::design_capture"])
        .env("DESIGN_CAPTURE_DIR", dir)
        .current_dir(root)
        .status()
        .map_err(|e| e.to_string())?;
    if status.success() { Ok(()) } else { Err("the design capture failed".into()) }
}

fn mkdir(p: &Path) -> Result<(), String> {
    std::fs::create_dir_all(p).map_err(|e| format!("{}: {e}", p.display()))
}

pub fn design_export(root: &Path, force: bool) -> Vec<String> {
    match export(root, force) {
        Ok(()) => Vec::new(),
        Err(e) => vec![e],
    }
}

fn export(root: &Path, force: bool) -> Result<(), String> {
    let file = root.join(DESIGN_FILE);
    if file.exists() && !force {
        return Err(format!(
            "{DESIGN_FILE} exists and owns the tokens: edit it, then `cargo xtask design-pull` (--force rebuilds it from the code, losing its edits)"
        ));
    }
    let cli = Cli::find()?;
    let work = work_dir(root);
    let (capture, icons_in, scripts) = (work.join("capture"), work.join("icons-in"), work.join("scripts"));
    for d in [&capture, &icons_in, &scripts] {
        let _ = std::fs::remove_dir_all(d);
        mkdir(d)?;
    }
    mkdir(&root.join("design"))?;
    let tokens = design_tokens::code_tokens(root).map_err(|p| p.join("\n  "))?;
    run_capture(root, &capture)?;

    // Phase A: variables, pages, the icons imported, and their colours read back.
    let mut a: Vec<Value> = variables_script(&tokens);
    a.push(step("page.rename", json!({ "name": "Tokens" })));
    for (name, page) in
        [("page_icons", "Icons"), ("page_components", "Components"), ("page_screens", "Screens · Light"), ("page_dark", "Screens · Dark")]
    {
        a.push(named(name, "page.add", json!({ "name": page })));
    }
    a.push(step("page.select", json!({ "id": reference("page_icons") })));
    let mut icon_names: Vec<String> = std::fs::read_dir(root.join(ICONS_DIR))
        .map_err(|e| format!("{ICONS_DIR}: {e}"))?
        .filter_map(|e| e.ok()?.path().file_stem().map(|s| s.to_string_lossy().into_owned()))
        .collect();
    icon_names.sort();
    let mut icon_steps = Vec::new();
    for (i, name) in icon_names.iter().enumerate() {
        let src = std::fs::read_to_string(root.join(ICONS_DIR).join(format!("{name}.svg"))).map_err(|e| format!("{name}.svg: {e}"))?;
        let line = design_tokens::hex_of(LINE_SOURCE);
        let pre = src.replace("currentColor", &line).replace("fill=\"white\"", &format!("fill=\"{MASK_WHITE}\""));
        let path = icons_in.join(format!("{name}.svg"));
        std::fs::write(&path, pre).map_err(|e| format!("{}: {e}", path.display()))?;
        let at = (f64::from(i as u32 % 10) * 96.0, f64::from(i as u32 / 10) * 112.0);
        icon_steps.push(a.len());
        a.push(named(&format!("icon_{i}"), "file.import", json!({ "path": path, "x": at.0, "y": at.1 })));
        a.push(step("selection.colors", json!({ "ids": [reference(&format!("icon_{i}"))] })));
    }
    let stage = work.join("stage.figcraft");
    let results = cli.run(&scripts, "export-a", &a, None, Some(&stage), &[root], &[&work])?;
    let page_id = |name: &str| a.iter().position(|s| s["as"] == name).and_then(|i| results.get(i)).and_then(|r| r["id"].as_u64());
    let pages: Vec<(String, u64)> =
        ["page_icons", "page_components", "page_screens", "page_dark"].iter().filter_map(|n| Some((n.to_string(), page_id(n)?))).collect();

    // Phase B: icon components, kit components, screens.
    let roles: HashMap<String, String> = tokens
        .iter()
        .filter(|t| t.collection == "Icon")
        .filter_map(|t| {
            t.values
                .iter()
                .find(|(m, _)| m == SOURCE_MODE)
                .and_then(|(_, v)| if let Tok::Colour(c) = v { Some((design_tokens::hex_of(*c), t.name.clone())) } else { None })
        })
        .collect();
    // Phase B runs in a fresh session: the pages are known by id.
    let page = |name: &str| pages.iter().find(|(n, _)| n == name).map(|(_, id)| *id).ok_or_else(|| format!("page {name} was not made"));
    let (page_components, page_light, page_dark) = (page("page_components")?, page("page_screens")?, page("page_dark")?);
    let mut b: Vec<Value> = Vec::new();
    let mut icons: HashMap<String, u64> = HashMap::new();
    for (name, at) in icon_names.iter().zip(&icon_steps) {
        let id = results.get(*at).and_then(|r| r["id"].as_u64()).ok_or_else(|| format!("icon {name}: not imported"))?;
        let colours = results.get(at + 1).and_then(|r| r["colors"].as_array()).cloned().unwrap_or_default();
        for c in colours {
            let col = &c["paint"]["color"];
            let ch = |k: &str| col[k].as_f64().map(|v| (v * 255.0).round() as u8);
            let (Some(r), Some(g), Some(bl)) = (ch("r"), ch("g"), ch("b")) else { continue };
            let hex = design_tokens::hex_of([r, g, bl, 255]);
            if hex == MASK_WHITE || hex == "#000000" {
                continue;
            }
            let role = roles.get(&hex).ok_or_else(|| format!("icon {name}: {hex} is not an icon role's colour"))?;
            b.push(step("selection.bindColorVariable", json!({ "targets": c["targets"], "variable": role })));
        }
        b.push(step("node.set", json!({ "ids": [id], "props": { "name": format!("icon/{name}") } })));
        b.push(step("component.create", json!({ "ids": [id] })));
        icons.insert(name.clone(), id);
    }

    let palette = Palette {
        colours: tokens
            .iter()
            .filter(|t| t.collection == "Colour")
            .filter_map(|t| {
                let get =
                    |m: &str| t.values.iter().find(|(mode, _)| mode == m).and_then(|(_, v)| if let Tok::Colour(c) = v { Some(*c) } else { None });
                Some((t.name.clone(), get("Light")?, get("Dark")?))
            })
            .collect(),
    };
    let messages = message_ids(root);
    let cx = Ctx { palette: &palette, messages: &messages, viewport: viewport_colours(root), icons: &icons };
    let mut sc = Script { steps: b, next: 0 };
    let mut shots: Vec<Shot> = std::fs::read_dir(&capture)
        .map_err(|e| e.to_string())?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .map(|p| read_shot(&p))
        .collect::<Result<_, _>>()?;
    shots.sort_by(|x, y| (x.dark, &x.name).cmp(&(y.dark, &y.name)));
    let mut comps = Components { by_sig: HashMap::new(), count: BTreeMap::new(), rows: BTreeMap::new(), next_row: 0.0 };
    let mut placed = [0usize; 2];
    for shot in &shots {
        let partner = shots.iter().find(|o| o.name == shot.name && o.dark != shot.dark);
        let slot = usize::from(shot.dark);
        let i = placed.get(slot).copied().unwrap_or(0);
        if let Some(p) = placed.get_mut(slot) {
            *p += 1;
        }
        let page = if shot.dark { page_dark } else { page_light };
        sc.push("page.select", json!({ "id": page }));
        let screen = sc.create(json!({ "type": "frame", "name": shot.name, "x": f64::from(i as u32 % 3) * (shot.size.0 + 160.0),
            "y": f64::from(i as u32 / 3) * (shot.size.1 + 200.0), "width": shot.size.0, "height": shot.size.1,
            "props": { "fill": "none", "clip": true }, "select": false }));
        let mode = if shot.dark { "Dark" } else { "Light" };
        sc.push("variable.setMode", json!({ "ids": [reference(&screen)], "collection": "Colour", "mode": mode }));
        sc.push("variable.setMode", json!({ "ids": [reference(&screen)], "collection": "Icon", "mode": mode }));
        for (li, layer) in shot.layers.iter().enumerate() {
            let partner_layer = partner.and_then(|p| p.layers.get(li));
            let tr = tree(layer);
            let group = sc
                .create(json!({ "type": "frame", "name": layer_name(layer, &tr), "x": 0, "y": 0, "width": shot.size.0, "height": shot.size.1,
                "parent": reference(&screen), "props": { "fill": "none", "clip": false }, "select": false }));
            // Shapes no control claimed, then the controls, in paint order of their first shape.
            for si in tr.shapes.first().cloned().unwrap_or_default() {
                if let Some(s) = layer.shapes.get(si) {
                    draw_shape(&mut sc, &cx, s, partner_layer.and_then(|p| p.shapes.get(si)), shot.dark, &reference(&group), (0.0, 0.0), None, false);
                }
            }
            for k in tr.children.first().cloned().unwrap_or_default() {
                emit(&mut sc, &cx, &mut comps, page_components, layer, partner_layer, &tr, k, shot.dark, &reference(&group), (0.0, 0.0));
            }
        }
    }
    sc.push("page.select", json!({ "id": 1 }));
    tokens_page(&mut sc, &tokens);
    eprintln!("  building {DESIGN_FILE}: {} steps, {} components", sc.steps.len(), comps.by_sig.len());
    cli.run(&scripts, "export-b", &sc.steps, Some(&stage), Some(&file), &[root], &[&work, &root.join("design")])?;

    // The design file now owns the tokens: export them, and the icons' fingerprints.
    crate::design_pull::pull(root, &cli, false)?;
    let problems = design_tokens::design_tokens(root, false);
    if problems.is_empty() { Ok(()) } else { Err(problems.join("\n  ")) }
}
