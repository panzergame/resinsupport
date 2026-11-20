//! The design capture: the app driven headlessly into each screen of the design file, in both
//! themes, recording what every layer painted (egui's shapes) and which kit control painted it
//! ([`crate::ui_kit::mark`]). `cargo xtask design-export` runs `design_capture` (ignored
//! otherwise) and builds `design/resinsupport.figcraft` from the files it writes.

use std::cell::RefCell;
use std::path::PathBuf;

use egui::epaint::{ColorMode, Shape};
use egui::{Color32, Event, LayerId, Pos2, Rect, pos2, vec2};
use serde_json::{Value, json};

use crate::prefs::{PAGES, Prefs};
use crate::{EditMode, ResinApp, layer_preview, object_label, support_brace, support_line};

thread_local! {
    static MARKS: RefCell<Option<Vec<Value>>> = const { RefCell::new(None) };
}

/// The window the screens are captured in.
const SCREEN: egui::Vec2 = vec2(1280.0, 800.0);

fn layer_key(layer: LayerId) -> String {
    format!("{:?}:{:016x}", layer.order, layer.id.value())
}

fn rect_json(r: Rect) -> Value {
    json!([r.min.x, r.min.y, r.max.x, r.max.y])
}

fn colour(c: Color32) -> Value {
    json!(c.to_srgba_unmultiplied())
}

/// Records a kit control while a capture frame runs.
pub(crate) fn mark(layer: LayerId, kind: &'static str, rect: Rect, label: &str) {
    MARKS.with(|m| {
        if let Some(marks) = m.borrow_mut().as_mut() {
            marks.push(json!({ "layer": layer_key(layer), "kind": kind, "rect": rect_json(rect), "label": label }));
        }
    });
}

fn stroke(width: f32, c: Color32) -> Value {
    if width > 0.0 && c.a() > 0 { json!({ "width": width, "colour": colour(c) }) } else { Value::Null }
}

fn fill(c: Color32) -> Value {
    if c.a() > 0 { colour(c) } else { Value::Null }
}

/// The text a galley shows, as laid out: wrapped rows on their own lines, an elided row with its
/// ellipsis.
fn shown_text(galley: &egui::Galley) -> String {
    let rows: Vec<String> = galley.rows.iter().map(|r| r.row.glyphs.iter().map(|g| g.chr).collect()).collect();
    rows.join("\n")
}

fn shape_json(shape: &Shape, clip: Rect, out: &mut Vec<Value>) {
    let bounds = shape.visual_bounding_rect();
    if bounds.is_positive() && !clip.intersects(bounds) {
        return;
    }
    match shape {
        Shape::Noop => {}
        Shape::Vec(shapes) => shapes.iter().for_each(|s| shape_json(s, clip, out)),
        Shape::Rect(r) => out.push(json!({
            "t": "rect",
            "rect": rect_json(r.rect),
            "fill": fill(r.fill),
            "stroke": stroke(r.stroke.width, r.stroke.color),
            "stroke_kind": format!("{:?}", r.stroke_kind),
            "radius": [r.corner_radius.nw, r.corner_radius.ne, r.corner_radius.se, r.corner_radius.sw],
            "blur": r.blur_width,
        })),
        Shape::Circle(c) => out.push(json!({
            "t": "ellipse",
            "rect": rect_json(Rect::from_center_size(c.center, vec2(c.radius * 2.0, c.radius * 2.0))),
            "fill": fill(c.fill),
            "stroke": stroke(c.stroke.width, c.stroke.color),
        })),
        Shape::Ellipse(e) => out.push(json!({
            "t": "ellipse",
            "rect": rect_json(Rect::from_center_size(e.center, e.radius * 2.0)),
            "fill": fill(e.fill),
            "stroke": stroke(e.stroke.width, e.stroke.color),
        })),
        Shape::LineSegment { points, stroke: s } => out.push(json!({
            "t": "path",
            "points": points.iter().map(|p| [p.x, p.y]).collect::<Vec<_>>(),
            "closed": false,
            "fill": Value::Null,
            "stroke": stroke(s.width, s.color),
        })),
        Shape::Path(p) => {
            let colour = match &p.stroke.color {
                ColorMode::Solid(c) => *c,
                ColorMode::UV(_) => Color32::TRANSPARENT,
            };
            out.push(json!({
                "t": "path",
                "points": p.points.iter().map(|p| [p.x, p.y]).collect::<Vec<_>>(),
                "closed": p.closed,
                "fill": fill(p.fill),
                "stroke": stroke(p.stroke.width, colour),
            }));
        }
        Shape::QuadraticBezier(b) => {
            out.push(json!({ "t": "path", "points": b.flatten(None).iter().map(|p| [p.x, p.y]).collect::<Vec<_>>(), "closed": b.closed,
                "fill": fill(b.fill), "stroke": match &b.stroke.color { ColorMode::Solid(c) => stroke(b.stroke.width, *c), ColorMode::UV(_) => Value::Null } }));
        }
        Shape::CubicBezier(b) => {
            out.push(json!({ "t": "path", "points": b.flatten(None).iter().map(|p| [p.x, p.y]).collect::<Vec<_>>(), "closed": b.closed,
                "fill": fill(b.fill), "stroke": match &b.stroke.color { ColorMode::Solid(c) => stroke(b.stroke.width, *c), ColorMode::UV(_) => Value::Null } }));
        }
        Shape::Text(t) => {
            let section = t.galley.job.sections.first();
            let format = section.map(|s| &s.format);
            let mut ink = t.override_text_color.or(format.map(|f| f.color)).unwrap_or(t.fallback_color);
            if ink == Color32::PLACEHOLDER {
                ink = t.fallback_color;
            }
            let font = format.map(|f| f.font_id.clone()).unwrap_or_default();
            let wrap = t.galley.job.wrap.max_width;
            out.push(json!({
                "t": "text",
                "rect": rect_json(t.galley.rect.translate(t.pos.to_vec2())),
                "text": shown_text(&t.galley),
                "size": font.size,
                "family": format!("{:?}", font.family),
                "colour": colour(ink.gamma_multiply(t.opacity_factor)),
                "spacing": format.map_or(0.0, |f| f.extra_letter_spacing),
                "wrap": if wrap.is_finite() { json!(wrap) } else { Value::Null },
                "elided": t.galley.elided,
            }));
        }
        Shape::Mesh(m) => {
            let r = m.calc_bounds();
            let tint = m.vertices.first().map_or(Color32::WHITE, |v| v.color);
            out.push(json!({ "t": "image", "rect": rect_json(r), "texture": format!("{:?}", m.texture_id), "tint": colour(tint) }));
        }
        Shape::Callback(c) => out.push(json!({ "t": "callback", "rect": rect_json(c.rect) })),
    }
}

/// Every layer's shapes this pass, back to front.
fn layers(ctx: &egui::Context) -> Vec<Value> {
    let mut ids: Vec<LayerId> = vec![LayerId::background()];
    ids.extend(ctx.memory(|m| m.layer_ids().collect::<Vec<_>>()));
    let mut seen = std::collections::HashSet::new();
    ids.retain(|l| seen.insert(*l));
    ids.sort_by_key(|l| l.order as u8);
    ctx.graphics(|g| {
        ids.iter()
            .filter_map(|layer| {
                let list = g.get(*layer)?;
                let mut shapes = Vec::new();
                for clipped in list.all_entries() {
                    shape_json(&clipped.shape, clipped.clip_rect, &mut shapes);
                }
                (!shapes.is_empty()).then(|| json!({ "id": layer_key(*layer), "order": format!("{:?}", layer.order), "shapes": shapes }))
            })
            .collect()
    })
}

/// The app in one theme, driven frame by frame.
struct Driver {
    ctx: egui::Context,
    app: ResinApp,
    time: f64,
    pointer: Pos2,
    marks: Vec<Value>,
    layers: Vec<Value>,
}

impl Driver {
    /// The app with a 10 mm cube, selected, in English.
    fn new(dark: bool) -> Driver {
        let ctx = egui::Context::default();
        crate::theme::apply(&ctx);
        ctx.set_theme(if dark { egui::Theme::Dark } else { egui::Theme::Light });
        let prefs = Prefs { language: "en".into(), ..Prefs::default() };
        let mut app = ResinApp::build(prefs, &[]);
        let dir = std::env::temp_dir().join(format!("resin-design-capture-{}", std::process::id()));
        std::fs::create_dir_all(&dir).ok();
        let path = dir.join("cube.stl");
        let v = |i: usize| format!("{} {} {}", (i & 1) * 10, (i >> 1 & 1) * 10, (i >> 2 & 1) * 10);
        let mut text = String::from("solid cube\n");
        for [a, b, c, d] in [[0, 2, 3, 1], [4, 5, 7, 6], [0, 1, 5, 4], [2, 6, 7, 3], [0, 4, 6, 2], [1, 3, 7, 5]] {
            for [p, q, r] in [[a, b, c], [a, c, d]] {
                text += &format!("facet normal 0 0 0\nouter loop\nvertex {}\nvertex {}\nvertex {}\nendloop\nendfacet\n", v(p), v(q), v(r));
            }
        }
        text += "endsolid cube\n";
        std::fs::write(&path, text).ok();
        app.import(&path);
        std::fs::remove_dir_all(dir).ok();
        let mut d = Driver { ctx, app, time: 0.0, pointer: pos2(-10.0, -10.0), marks: Vec::new(), layers: Vec::new() };
        d.select_all();
        d.settle(4);
        d
    }

    fn select_all(&mut self) {
        let ids: Vec<u64> = self.app.session.scene.models().iter().map(|m| m.id).collect();
        self.app.run("model.select", json!({ "ids": ids }));
    }

    /// One frame with these events; its marks and shapes are kept.
    fn frame(&mut self, events: Vec<Event>) {
        self.time += 1.0 / 30.0;
        let raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, SCREEN)), time: Some(self.time), events, ..Default::default() };
        let (app, marks, layers) = (&mut self.app, &mut self.marks, &mut self.layers);
        let mut out = self.ctx.run_ui(raw, |ui| {
            MARKS.with(|m| *m.borrow_mut() = Some(Vec::new()));
            app.frame(ui);
            *marks = MARKS.with(|m| m.borrow_mut().take()).unwrap_or_default();
            *layers = self::layers(ui.ctx());
        });
        // Nothing renders these frames.
        out.textures_delta.clear();
    }

    fn settle(&mut self, frames: usize) {
        for _ in 0..frames {
            self.frame(Vec::new());
        }
    }

    /// The centre of the last frame's first `kind` mark, labelled `label` if given.
    fn find(&self, kind: &str, label: Option<&str>) -> Option<Pos2> {
        self.marks.iter().find(|m| m["kind"] == kind && label.is_none_or(|l| m["label"] == l)).and_then(|m| {
            let r = m["rect"].as_array()?;
            let at = |i: usize| r.get(i).and_then(Value::as_f64).map(|v| v as f32);
            Some(pos2((at(0)? + at(2)?) / 2.0, (at(1)? + at(3)?) / 2.0))
        })
    }

    fn move_to(&mut self, at: Pos2) {
        self.pointer = at;
        self.frame(vec![Event::PointerMoved(at)]);
    }

    /// Click the `kind` mark labelled `label`; whether there was one.
    fn click(&mut self, kind: &str, label: Option<&str>) -> bool {
        let Some(at) = self.find(kind, label) else { return false };
        self.move_to(at);
        let button = |pressed| Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
        self.frame(vec![button(true)]);
        self.frame(vec![button(false)]);
        // Away from what was clicked, so no hover fill shows; popups stay open.
        self.move_to(pos2(SCREEN.x - 2.0, SCREEN.y - 2.0));
        self.settle(3);
        true
    }

    /// Rest the pointer on the `kind` mark labelled `label` until its tooltip shows.
    fn hover(&mut self, kind: &str, label: Option<&str>) -> bool {
        let Some(at) = self.find(kind, label) else { return false };
        self.move_to(at);
        for _ in 0..40 {
            self.frame(Vec::new());
        }
        true
    }

    /// Wait (frames and wall time) until the background slice is done; whether it was.
    fn sliced(&mut self) -> bool {
        for _ in 0..600 {
            if self.app.session.job().is_some() && !self.app.slicer.is_running() {
                self.settle(4);
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
            self.frame(Vec::new());
        }
        false
    }

    fn capture(&mut self, name: &str, dark: bool) -> Value {
        // Long enough for a window's fade-in to end: its colours are then the tokens'.
        self.settle(20);
        json!({
            "name": name,
            "theme": if dark { "Dark" } else { "Light" },
            "size": [SCREEN.x, SCREEN.y],
            "layers": self.layers,
            "marks": self.marks,
        })
    }
}

/// How to reach a screen from the app with a selected cube; false when it could not be reached.
type Setup = fn(&mut Driver) -> bool;

fn support_scene(d: &mut Driver) -> bool {
    d.app.set_mode(EditMode::Support);
    let Some((model, low)) = d.app.session.scene.models().first().map(|m| (m.id, m.bounds.min)) else { return false };
    for x in [2.0, 8.0] {
        let point = [low.x + x, low.y + 5.0, low.z];
        if d.app.run("support.add", json!({ "model": model, "point": point, "normal": [0, 0, -1] })).is_none() {
            return false;
        }
    }
    d.settle(3);
    true
}

fn slice_scene(d: &mut Driver) -> bool {
    d.app.set_mode(EditMode::Slice);
    d.sliced()
}

fn screens() -> Vec<(String, Setup)> {
    let mut s: Vec<(String, Setup)> = vec![
        ("object".into(), |_| true),
        ("object-move".into(), |d| d.click("ribbon_button", Some("Move"))),
        ("object-rotate".into(), |d| d.click("ribbon_button", Some("Rotate"))),
        ("object-scale".into(), |d| d.click("ribbon_button", Some("Scale"))),
        ("object-label".into(), |d| {
            object_label::open(&mut d.app);
            d.settle(3);
            d.app.tool.is_some()
        }),
        ("menu-file".into(), |d| d.click("app_bar_menu", None)),
        ("menu-view".into(), |d| d.click("ribbon_group_menu", Some("View"))),
        ("tooltip-ribbon".into(), |d| d.hover("ribbon_button", Some("Move"))),
        ("support".into(), support_scene),
        ("support-line".into(), |d| {
            support_scene(d) && {
                support_line::open(&mut d.app);
                d.settle(3);
                d.app.tool.is_some()
            }
        }),
        ("support-brace".into(), |d| {
            support_scene(d) && {
                support_brace::open(&mut d.app);
                d.settle(3);
                d.app.tool.is_some()
            }
        }),
        ("support-reset".into(), |d| {
            support_scene(d) && {
                d.app.confirm_reset = true;
                d.settle(3);
                true
            }
        }),
        ("slice".into(), slice_scene),
        ("slice-dropdown".into(), |d| slice_scene(d) && d.click("dropdown", None)),
        ("slice-object".into(), |d| {
            slice_scene(d) && {
                d.app.object_slicing = true;
                d.settle(3);
                true
            }
        }),
        ("slice-layer".into(), |d| {
            slice_scene(d) && {
                d.app.preview.flat = true;
                d.settle(3);
                layer_preview::active(&d.app)
            }
        }),
        ("slice-reset".into(), |d| {
            slice_scene(d) && {
                d.app.confirm_print_reset = true;
                d.settle(3);
                true
            }
        }),
        ("open-confirm".into(), |d| {
            d.app.pending_open = Some((PathBuf::from("plate.rsup"), Vec::new()));
            d.settle(3);
            true
        }),
    ];
    for tab in 1..crate::support_settings::TABS.len() {
        let setup: Setup = match tab {
            1 => |d| support_scene(d) && tab_to(d, |a| a.settings_tab = 1),
            2 => |d| support_scene(d) && tab_to(d, |a| a.settings_tab = 2),
            _ => |d| support_scene(d) && tab_to(d, |a| a.settings_tab = 3),
        };
        s.push((format!("support-tab-{tab}"), setup));
    }
    for tab in 1..crate::print_settings::TABS.len() {
        let setup: Setup = match tab {
            1 => |d| slice_scene(d) && tab_to(d, |a| a.print_tab = 1),
            2 => |d| slice_scene(d) && tab_to(d, |a| a.print_tab = 2),
            3 => |d| slice_scene(d) && tab_to(d, |a| a.print_tab = 3),
            _ => |d| slice_scene(d) && tab_to(d, |a| a.print_tab = 4),
        };
        s.push((format!("slice-tab-{tab}"), setup));
    }
    for page in 0..PAGES.len() {
        let setup: Setup = match page {
            0 => |d| tab_to(d, |a| (a.prefs_open, a.prefs_page) = (true, 0)),
            1 => |d| tab_to(d, |a| (a.prefs_open, a.prefs_page) = (true, 1)),
            2 => |d| tab_to(d, |a| (a.prefs_open, a.prefs_page) = (true, 2)),
            3 => |d| tab_to(d, |a| (a.prefs_open, a.prefs_page) = (true, 3)),
            4 => |d| tab_to(d, |a| (a.prefs_open, a.prefs_page) = (true, 4)),
            _ => |d| tab_to(d, |a| (a.prefs_open, a.prefs_page) = (true, 5)),
        };
        s.push((format!("prefs-{page}"), setup));
    }
    s
}

fn tab_to(d: &mut Driver, set: impl FnOnce(&mut ResinApp)) -> bool {
    set(&mut d.app);
    d.settle(4);
    true
}

/// Writes `<dir>/<screen>-<theme>.json` for every screen, `dir` being `DESIGN_CAPTURE_DIR` or
/// `target/design-capture`. A screen it cannot reach fails the run.
#[test]
#[ignore = "run by cargo xtask design-export"]
fn design_capture() {
    let dir = std::env::var_os("DESIGN_CAPTURE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/design-capture"));
    std::fs::create_dir_all(&dir).unwrap();
    let only = std::env::var("DESIGN_CAPTURE_ONLY").ok();
    let mut missed = Vec::new();
    for (name, setup) in screens() {
        if only.as_deref().is_some_and(|o| !o.split(',').any(|n| n == name)) {
            continue;
        }
        for dark in [false, true] {
            let mut d = Driver::new(dark);
            if !setup(&mut d) {
                missed.push(format!("{name} ({})", if dark { "dark" } else { "light" }));
                continue;
            }
            let shot = d.capture(&name, dark);
            let file = dir.join(format!("{name}-{}.json", if dark { "dark" } else { "light" }));
            std::fs::write(&file, serde_json::to_string(&shot).unwrap()).unwrap();
        }
    }
    assert!(missed.is_empty(), "screens not reached: {missed:?}");
}
