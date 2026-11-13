//! Braces (H in Support edition): struts between the bodies of the selected supports, a selected
//! segment standing for its support. The card sets the pattern, how supports are paired, the
//! angle, the struts' diameter, the gaps at the head and above the base, and how far apart braced
//! supports may stand; the struts show live, and the selection may change while it is open.
//! Enter (OK) runs `support.add_braces`, one step, in place of the braces already joining the
//! selected supports, so changing the pairing or a setting and pressing H again rebuilds them; Esc, Cancel or a mode change puts the tool away,
//! the scene and the undo history untouched. The settings are kept in the preferences.

use egui::vec2;
use resin_engine::geom::Vec3;
use resin_engine::{
    ANGLE_RANGE, BracePairing, BraceParams, BracePattern, DEFAULT_MAX_DISTANCE, DIAMETER_RATIO_RANGE, GAP_RANGE, MAX_BRACES, MAX_DISTANCE_RANGE,
    brace_pairs, brace_solids,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::theme::Tokens;
use crate::theme::metrics::{CONTROL_HEIGHT, FORM_CARD_WIDTH, SPACE_12};
use crate::ui_kit::{self, Action, Buttons, Card};
use crate::{EditMode, ResinApp, i18n};

/// What the preview was worked out from: the scene's revision, the routes' changes, the
/// supports, the settings, the pairing and the reach.
type Key = (u64, u64, Vec<u64>, BraceParams, BracePairing, f64);

/// The braces Enter would add.
#[derive(Default)]
pub struct Preview {
    key: Option<Key>,
    /// The supports to brace, each the one whose body it shares.
    pub supports: Vec<u64>,
    /// Each pair with struts to make, and those.
    pub braces: Vec<(u64, u64, Vec<(Vec3, Vec3)>)>,
    /// Pairs within reach with no strut to make.
    pub skipped: usize,
    /// Pairs within reach, when more than `support.add_braces` makes at once: none is worked out.
    pub too_many: Option<usize>,
    /// The braces already joining two of the supports, which Enter replaces.
    pub replaced: usize,
}

/// The tool and what it keeps from one use to the next.
pub struct BraceState {
    /// `Some` while the card is open.
    pub tool: Option<Preview>,
    pub params: BraceParams,
    pub pairing: BracePairing,
    pub max_distance: f64,
}

impl Default for BraceState {
    fn default() -> Self {
        BraceState { tool: None, params: BraceParams::default(), pairing: BracePairing::Tree, max_distance: DEFAULT_MAX_DISTANCE }
    }
}

/// The card's settings as the preferences keep them.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BraceSettings {
    /// `zigzag` or `cross`.
    pub pattern: String,
    /// `chain`, `tree` or `all`.
    pub pairing: String,
    pub max_distance: f64,
    pub angle: f64,
    pub diameter_ratio: f64,
    pub head_gap: f64,
    pub foot_gap: f64,
}

impl Default for BraceSettings {
    fn default() -> Self {
        BraceState::default().settings()
    }
}

impl BraceState {
    pub fn settings(&self) -> BraceSettings {
        let p = &self.params;
        BraceSettings {
            pattern: p.pattern.id().into(),
            pairing: self.pairing.id().into(),
            max_distance: self.max_distance,
            angle: p.angle,
            diameter_ratio: p.diameter_ratio,
            head_gap: p.head_gap,
            foot_gap: p.foot_gap,
        }
    }
    /// Take the stored settings, each one unknown or out of range keeping its default; those, by name.
    pub fn restore(&mut self, s: &BraceSettings) -> Vec<&'static str> {
        let mut bad = Vec::new();
        match BracePattern::parse(&s.pattern) {
            Some(v) => self.params.pattern = v,
            None => bad.push("pattern"),
        }
        match BracePairing::parse(&s.pairing) {
            Some(v) => self.pairing = v,
            None => bad.push("pairing"),
        }
        let numbers: [(&'static str, f64, &mut f64, &std::ops::RangeInclusive<f64>); 5] = [
            ("max_distance", s.max_distance, &mut self.max_distance, &MAX_DISTANCE_RANGE),
            ("angle", s.angle, &mut self.params.angle, &ANGLE_RANGE),
            ("diameter_ratio", s.diameter_ratio, &mut self.params.diameter_ratio, &DIAMETER_RATIO_RANGE),
            ("head_gap", s.head_gap, &mut self.params.head_gap, &GAP_RANGE),
            ("foot_gap", s.foot_gap, &mut self.params.foot_gap, &GAP_RANGE),
        ];
        for (name, stored, field, range) in numbers {
            if range.contains(&stored) {
                *field = stored;
            } else {
                bad.push(name);
            }
        }
        bad
    }
}

/// Open the card, in Support edition.
pub fn open(app: &mut ResinApp) {
    if app.mode != EditMode::Support {
        app.status = Some((i18n::format("tool.needs_support_edition", &[("tool", &i18n::tr("tool.brace"))]), true));
        return;
    }
    app.supports.branching = None;
    app.supports.moving = None;
    app.line.tool = None;
    app.status = None;
    app.brace.tool = Some(Preview::default());
}

/// The selected supports and those with selected parts, each the one whose body it shares.
fn selected_roots(app: &ResinApp) -> Vec<u64> {
    let scene = &app.session.scene;
    let mut ids: Vec<u64> =
        scene.selected.iter().copied().chain(scene.selected_parts.iter().map(|(id, _)| *id)).filter_map(|id| scene.root_of(id)).collect();
    ids.sort_unstable();
    ids.dedup();
    ids
}

/// Once a frame: put the card away outside Support edition, and work the preview out afresh when
/// the selection, a route or a setting changed. Uses the routes as drawn, so it waits on none.
pub fn refresh(app: &mut ResinApp) {
    if app.brace.tool.is_none() {
        return;
    }
    if app.mode != EditMode::Support {
        app.brace.tool = None;
        return;
    }
    let supports = selected_roots(app);
    let b = &app.brace;
    let key = (app.session.scene.revision(), app.routes.changes(), supports.clone(), b.params, b.pairing, b.max_distance);
    if app.brace.tool.as_ref().is_some_and(|t| t.key.as_ref() == Some(&key)) {
        return;
    }
    let scene = &app.session.scene;
    let preset = app.session.support_settings().current();
    let replaced = scene.braces().iter().filter(|b| supports.contains(&b.a) && supports.contains(&b.b)).count();
    let feet: Vec<(u64, Vec3)> = supports
        .iter()
        .filter_map(|id| app.routes.route(*id).filter(|r| r.reachable() && r.foot).and_then(|r| r.body.last()).map(|f| (*id, *f)))
        .collect();
    let pairs = brace_pairs(&feet, app.brace.pairing, app.brace.max_distance);
    // The command refuses these; working out their struts here would stall the window first.
    if pairs.len() > MAX_BRACES {
        app.brace.tool = Some(Preview { key: Some(key), supports, too_many: Some(pairs.len()), replaced, ..Default::default() });
        return;
    }
    let mut braces = Vec::new();
    let mut skipped = 0;
    for (a, b) in pairs {
        let (Some(ra), Some(rb)) = (app.routes.route(a), app.routes.route(b)) else { continue };
        let (struts, _) = brace_solids(scene, ra, rb, &app.brace.params, preset);
        if struts.is_empty() {
            skipped += 1;
        } else {
            braces.push((a, b, struts));
        }
    }
    app.brace.tool = Some(Preview { key: Some(key), supports, braces, skipped, too_many: None, replaced });
}

/// The struts Enter would add, to draw.
pub fn preview_struts(app: &ResinApp) -> impl Iterator<Item = (Vec3, Vec3)> + '_ {
    app.brace.tool.iter().flat_map(|t| t.braces.iter().flat_map(|(.., s)| s.iter().copied()))
}

/// A number field in a row: `value` within `range`, shown with `suffix`.
fn number(ui: &mut egui::Ui, t: &Tokens, label: &str, tip: &str, value: &mut f64, range: std::ops::RangeInclusive<f64>, suffix: &str) {
    ui_kit::row(ui, t, label, |ui| {
        ui.add_sized([ui.available_width(), CONTROL_HEIGHT], egui::DragValue::new(value).speed(0.1).range(range).suffix(suffix).max_decimals(2))
            .on_hover_text(tip)
    });
}

/// The card: the settings, and what Enter would add.
pub fn card(app: &mut ResinApp, ctx: &egui::Context) {
    let Some(tool) = app.brace.tool.as_ref() else { return };
    let t = Tokens::for_dark(ctx.global_style().visuals.dark_mode);
    let (adds, skipped, enough, too_many, replaced) = (tool.braces.len(), tool.skipped, tool.supports.len() >= 2, tool.too_many, tool.replaced);
    let mut p = app.brace.params;
    let mut pattern = p.pattern.id().to_string();
    let mut pairing = app.brace.pairing.id().to_string();
    let mut percent = p.diameter_ratio * 100.0;
    let mut reach = app.brace.max_distance;
    let card = Card {
        id: egui::Id::new("support_brace"),
        title: i18n::tr("tool.brace"),
        icon: "cone",
        at: egui::Pos2::ZERO,
        bounds: app.viewport_rect,
        width: FORM_CARD_WIDTH,
        // Making none in place of some is a change too: it clears the selection's braces.
        buttons: Buttons::OkCancel { ok_enabled: too_many.is_none() && (adds > 0 || replaced > 0) },
    };
    // Pinned to the view's top right, as the tip line's card.
    let action = card.show_anchored(ctx, &t, egui::Align2::RIGHT_TOP, vec2(-SPACE_12, SPACE_12), |ui| {
        let patterns = [("zigzag", "tool.brace.zigzag"), ("cross", "tool.brace.cross")];
        ui_kit::choice_row(ui, &t, i18n::tr("tool.brace.pattern"), "support_brace_pattern", &mut pattern, &patterns);
        let pairings = [("chain", "tool.brace.chain"), ("tree", "tool.brace.tree"), ("all", "tool.brace.all")];
        ui_kit::choice_row(ui, &t, i18n::tr("tool.brace.pairing"), "support_brace_pairing", &mut pairing, &pairings);
        number(ui, &t, i18n::tr("tool.brace.max_distance"), i18n::tr("tool.brace.max_distance_tip"), &mut reach, MAX_DISTANCE_RANGE, " mm");
        number(ui, &t, i18n::tr("tool.brace.angle"), i18n::tr("tool.brace.angle_tip"), &mut p.angle, ANGLE_RANGE, "°");
        let percents = DIAMETER_RATIO_RANGE.start() * 100.0..=DIAMETER_RATIO_RANGE.end() * 100.0;
        number(ui, &t, i18n::tr("tool.brace.diameter"), i18n::tr("tool.brace.diameter_tip"), &mut percent, percents, " %");
        number(ui, &t, i18n::tr("tool.brace.head_gap"), i18n::tr("tool.brace.head_gap_tip"), &mut p.head_gap, GAP_RANGE, " mm");
        number(ui, &t, i18n::tr("tool.brace.foot_gap"), i18n::tr("tool.brace.foot_gap_tip"), &mut p.foot_gap, GAP_RANGE, " mm");
        if let Some(n) = too_many {
            let msg = i18n::format("tool.brace.too_many", &[("n", &n), ("max", &MAX_BRACES)]);
            ui.label(egui::RichText::new(msg).color(t.warning));
        } else if enough && replaced > 0 {
            ui_kit::hint(ui, &t, &i18n::count("tool.brace.replaces", adds, &[("old", &replaced)]));
        } else if enough {
            ui_kit::hint(ui, &t, &i18n::count("tool.brace.adds", adds, &[]));
        } else {
            ui_kit::hint(ui, &t, i18n::tr("tool.brace.select"));
        }
        if skipped > 0 {
            ui.label(egui::RichText::new(i18n::count("tool.brace.skipped", skipped, &[])).color(t.warning));
        }
    });
    p.pattern = BracePattern::parse(&pattern).unwrap_or(p.pattern);
    p.diameter_ratio = (percent / 100.0).clamp(*DIAMETER_RATIO_RANGE.start(), *DIAMETER_RATIO_RANGE.end());
    p.angle = p.angle.clamp(*ANGLE_RANGE.start(), *ANGLE_RANGE.end());
    p.head_gap = p.head_gap.clamp(*GAP_RANGE.start(), *GAP_RANGE.end());
    p.foot_gap = p.foot_gap.clamp(*GAP_RANGE.start(), *GAP_RANGE.end());
    app.brace.params = p;
    app.brace.pairing = BracePairing::parse(&pairing).unwrap_or(app.brace.pairing);
    app.brace.max_distance = reach.clamp(*MAX_DISTANCE_RANGE.start(), *MAX_DISTANCE_RANGE.end());
    match action {
        Action::Ok => make(app),
        Action::Cancel => app.brace.tool = None,
        Action::Reset | Action::None => {}
    }
}

/// Run `support.add_braces` on the supports the preview showed, and put the card away.
fn make(app: &mut ResinApp) {
    let Some(tool) = app.brace.tool.as_ref() else { return };
    let p = app.brace.params;
    let params = json!({
        "ids": tool.supports,
        "pattern": p.pattern.id(),
        "pairing": app.brace.pairing.id(),
        "max_distance": app.brace.max_distance,
        "angle": p.angle,
        "diameter_ratio": p.diameter_ratio,
        "head_gap": p.head_gap,
        "foot_gap": p.foot_gap,
    });
    if let Some(v) = app.run("support.add_braces", params) {
        let n = v["ids"].as_array().map_or(0, Vec::len);
        let replaced = v["replaced"].as_u64().unwrap_or(0);
        let msg =
            if replaced > 0 { i18n::count("status.braces_replaced", n, &[("old", &replaced)]) } else { i18n::count("status.braces_added", n, &[]) };
        app.status = Some((msg, false));
        app.brace.tool = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prefs::Prefs;
    use egui::{Event, Modifiers, Pos2, Rect};

    /// H with supports selected opens the card and shows the struts Enter adds in one step; Esc
    /// puts it away with no step; the settings stay for the next time.
    #[test]
    fn h_braces_the_selected_supports_and_enter_adds_them() {
        let dir = std::env::temp_dir().join(format!("resin-ui-brace-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("slab.stl");
        // A square slab floating 20 mm up, facing down.
        let text = "solid p\nfacet normal 0 0 0\nouter loop\nvertex 0 0 20\nvertex 0 20 20\nvertex 20 20 20\nendloop\nendfacet\n\
                    facet normal 0 0 0\nouter loop\nvertex 0 0 20\nvertex 20 20 20\nvertex 20 0 20\nendloop\nendfacet\nendsolid p\n";
        std::fs::write(&path, text).unwrap();
        let mut app = ResinApp::build(Prefs::default(), &[]);
        let m = app.run("file.import_stl", json!({ "path": path, "fit": false })).unwrap()["id"].as_u64().unwrap();
        std::fs::remove_dir_all(dir).ok();
        let add = |app: &mut ResinApp, x: f64| {
            app.run("support.add", json!({ "model": m, "point": [x, 10, 20], "normal": [0, 0, -1] })).unwrap()["id"].as_u64().unwrap()
        };
        let (a, b, c) = (add(&mut app, 2.0), add(&mut app, 10.0), add(&mut app, 18.0));
        let preset = *app.session.support_settings().current();
        while app.routes.update(&app.session.scene, &preset) {}
        let rect = Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0));
        app.viewport_rect = Some(rect);
        let ctx = egui::Context::default();
        let mut time = 0.0;
        let mut frame = |app: &mut ResinApp, events: Vec<Event>| {
            time += 0.5;
            let raw = egui::RawInput { screen_rect: Some(rect), time: Some(time), events, ..Default::default() };
            let mut out = ctx.run_ui(raw, |ui| {
                app.keys(ui.ctx());
                refresh(app);
                card(app, ui.ctx());
            });
            out.textures_delta.clear();
        };
        let key = |key: egui::Key| {
            [true, false].map(|pressed| Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE }).to_vec()
        };

        frame(&mut app, key(egui::Key::H));
        assert!(app.brace.tool.is_none(), "Object edition: H does nothing but say so");
        app.mode = EditMode::Support;
        // `a` itself, and `b` through a selected segment.
        app.run("support.select", json!({ "ids": [a], "parts": [{ "support": b, "segment": 0 }] }));
        frame(&mut app, key(egui::Key::H));
        frame(&mut app, vec![]);
        let preview = app.brace.tool.as_ref().expect("the card is open");
        assert_eq!(preview.supports, vec![a, b]);
        assert_eq!(preview.braces.iter().map(|(x, y, _)| (*x, *y)).collect::<Vec<_>>(), vec![(a, b)]);
        assert!(preview_struts(&app).count() > 0);

        // Esc: no step, nothing added.
        let depth = app.session.undo_depth();
        frame(&mut app, key(egui::Key::Escape));
        assert!(app.brace.tool.is_none() && app.session.scene.braces().is_empty());
        assert_eq!(app.session.undo_depth(), depth);

        // Again with all three, crossed: Enter adds two braces in one step.
        app.brace.params.pattern = BracePattern::Cross;
        app.run("support.select", json!({ "ids": [a, b, c] }));
        frame(&mut app, key(egui::Key::H));
        frame(&mut app, vec![]);
        frame(&mut app, key(egui::Key::Enter));
        let braces = app.session.scene.braces();
        assert_eq!(braces.iter().map(|x| (x.a, x.b)).collect::<Vec<_>>(), vec![(a, b), (b, c)]);
        assert!(braces.iter().all(|x| x.params.pattern == BracePattern::Cross));
        assert_eq!(app.session.undo_depth(), depth + 1);
        assert!(app.brace.tool.is_none());
        assert_eq!(app.brace.params.pattern, BracePattern::Cross, "the settings stay");

        // Every pair in reach, then H again: the tree's two braces give way to three.
        app.brace.pairing = BracePairing::All;
        app.brace.max_distance = 20.0;
        app.brace.params.angle = 30.0;
        frame(&mut app, key(egui::Key::H));
        frame(&mut app, vec![]);
        let preview = app.brace.tool.as_ref().expect("the card is open");
        assert_eq!((preview.braces.len(), preview.replaced), (3, 2));
        frame(&mut app, key(egui::Key::Enter));
        assert_eq!(app.session.scene.braces().len(), 3);
        // Back to the tree: rebuilt, not added to.
        app.brace.pairing = BracePairing::Tree;
        frame(&mut app, key(egui::Key::H));
        frame(&mut app, vec![]);
        frame(&mut app, key(egui::Key::Enter));
        let braces = app.session.scene.braces();
        assert_eq!(braces.iter().map(|x| (x.a, x.b)).collect::<Vec<_>>(), vec![(a, b), (b, c)]);
        assert_eq!(app.session.undo_depth(), depth + 3);
    }
}
