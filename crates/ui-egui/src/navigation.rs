//! Camera navigation. One entry point, [`handle`], reads a frame's pointer, wheel, trackpad and
//! modifier input over the viewport and moves the camera the way the chosen mouse mapping says.
//! Any future canvas must call it rather than reading drags or the wheel itself.
//!
//! Reduced port of SolidCraft's `crates/ui-egui/src/navigation.rs` (MIT OR Apache-2.0) at
//! 4bdffef0; see ATTRIBUTION.md. Left out for now: the 3D mouse, the navigation cube, macOS
//! inertia coasts, browser wheel workarounds, continuous orbit and orbit snapping.

use egui::{Pos2, Rect, Vec2, vec2};
use resin_engine::geom::Vec3;
use resin_engine::render::Camera;
use serde_json::json;

use crate::ResinApp;
use crate::prefs::{NavButton, NavMode, OrbitAround, TOUCHPAD_NAV, nav_preset};

/// Navigation state carried between frames.
#[derive(Default)]
pub struct NavState {
    /// A bracketed trackpad scroll (`Start` … `End`) is under way.
    trackpad_gesture: bool,
    /// What a left drag that began this press does (a preset's left binding, Ctrl+left orbit).
    primary_nav: Option<NavMode>,
    /// The right button has moved far enough that it can no longer be a click.
    right_drag: bool,
    orbit_pivot: Option<Vec3>,
    orbit_from: Option<OrbitFrom>,
    middle_clicked: bool,
}

impl NavState {
    /// A left press that navigates (a mapping's keys, Ctrl+left orbit) is under way or ending
    /// this frame: its click must not pick anything.
    pub fn primary_navigates(&self) -> bool {
        self.primary_nav.is_some()
    }
}

/// The camera an orbit drag began with and its whole turn so far.
#[derive(Clone, Copy)]
struct OrbitFrom {
    start: Camera,
    dx: f64,
    dy: f64,
    pivot: Option<Vec3>,
    last: Camera,
}

/// Raw wheel/trackpad input for one frame, split by source. Mouse wheels report lines or pages;
/// trackpads report points, bracketed by `Start` … `End` phases, which tells them apart.
struct RawWheel {
    /// Vertical mouse-wheel input in points (zooms).
    zoom: f32,
    /// Trackpad point input, routed like the wheel (zooms when panning is off).
    point_zoom: f32,
    /// Bracketed two-finger scroll in points (pans when panning is on).
    pan: Vec2,
    /// Bracketed two-finger scroll with Shift held (orbits with the Fusion mapping).
    shift_pan: Vec2,
    /// Bare horizontal input in points (thumb wheel: always pans sideways).
    mouse_pan: Vec2,
}

/// The wheel in egui's units and modifier routing, without its temporal smoothing.
fn immediate_wheel(input: &egui::InputState, options: &egui::InputOptions, page_height: f32, gesture: &mut bool) -> RawWheel {
    let mut out = RawWheel { zoom: 0.0, point_zoom: 0.0, pan: Vec2::ZERO, shift_pan: Vec2::ZERO, mouse_pan: Vec2::ZERO };
    for event in &input.events {
        let egui::Event::MouseWheel { unit, delta, modifiers, phase } = event else { continue };
        // Ctrl+wheel is egui's pinch path; `pinch_factor` reads it.
        if modifiers.matches_any(options.zoom_modifier) {
            continue;
        }
        let point = matches!(unit, egui::MouseWheelUnit::Point);
        match phase {
            egui::TouchPhase::Start => {
                *gesture |= point;
                continue;
            }
            egui::TouchPhase::End | egui::TouchPhase::Cancel => {
                if point {
                    *gesture = false;
                }
                continue;
            }
            egui::TouchPhase::Move => {}
        }
        let points = match unit {
            egui::MouseWheelUnit::Point => *delta,
            egui::MouseWheelUnit::Line => *delta * options.line_scroll_speed,
            egui::MouseWheelUnit::Page => *delta * page_height,
        };
        if !points.is_finite() {
            continue;
        }
        // egui turns a Shift+wheel sideways; the camera zooms by whichever axis moved.
        let vertical = if points.y != 0.0 {
            points.y
        } else if modifiers.shift {
            points.x
        } else {
            0.0
        };
        let shift = modifiers.shift && !modifiers.ctrl && !modifiers.command && !modifiers.alt;
        if point && *gesture {
            if shift {
                out.shift_pan += points.clamp(vec2(-1e4, -1e4), vec2(1e4, 1e4));
            } else {
                out.pan += points;
            }
            out.point_zoom += vertical;
        } else if point {
            // Bare moves are a mouse: vertical zooms, horizontal pans sideways. A wheel needs the
            // opposite sign of fingers for the same motion.
            out.zoom += vertical;
            if !modifiers.shift {
                out.mouse_pan += vec2(-points.x, 0.0);
            }
        } else {
            out.zoom += vertical;
        }
    }
    out
}

/// Combined pinch-to-zoom factor for one frame (1 = no pinch). Some input paths deliver pinch as
/// a zoom-modified wheel instead of `Event::Zoom`.
fn pinch_factor(input: &egui::InputState, options: &egui::InputOptions, page_height: f32) -> f32 {
    let mut factor = 1.0;
    for event in &input.events {
        match event {
            egui::Event::Zoom(f) if f.is_finite() && *f > 0.0 => factor *= f,
            egui::Event::MouseWheel { unit, delta, modifiers, phase: egui::TouchPhase::Move } if modifiers.matches_any(options.zoom_modifier) => {
                let points = match unit {
                    egui::MouseWheelUnit::Point => *delta,
                    egui::MouseWheelUnit::Line => *delta * options.line_scroll_speed,
                    egui::MouseWheelUnit::Page => *delta * page_height,
                };
                let f = (options.scroll_zoom_speed * (points.x + points.y)).exp();
                if f.is_finite() && f > 0.0 {
                    factor *= f;
                }
            }
            _ => {}
        }
    }
    factor
}

/// The viewport's camera and rectangle, for rays through pixels.
pub struct Proj {
    pub cam: Camera,
    pub rect: Rect,
}

impl Proj {
    pub fn ray(&self, p: Pos2) -> (Vec3, Vec3) {
        let (w, h) = (f64::from(self.rect.width()), f64::from(self.rect.height()));
        self.cam.ray(f64::from(p.x - self.rect.left()), f64::from(p.y - self.rect.top()), w, h)
    }
    /// Where a scene point shows on screen; None behind the eye.
    pub fn to_screen(&self, p: Vec3) -> Option<Pos2> {
        let (w, h) = (f64::from(self.rect.width().max(1.0)), f64::from(self.rect.height().max(1.0)));
        // The clip radius only sets depth, which a screen position doesn't need.
        let (x, y, _) = self.cam.to_screen(p, w, h, 1.0)?;
        (x.is_finite() && y.is_finite()).then(|| Pos2::new(self.rect.left() + x as f32, self.rect.top() + y as f32))
    }
}

/// The nearest model point under `p`.
pub fn pick(app: &ResinApp, proj: &Proj, p: Pos2) -> Option<Vec3> {
    let (o, d) = proj.ray(p);
    app.session.scene.ray_hit(o, d).map(|(_, h)| h.point)
}

/// Toward the model's surface under the cursor, else the target's depth.
fn zoom_point(app: &ResinApp, rect: Rect, p: Pos2) -> Vec3 {
    let proj = Proj { cam: app.session.camera, rect };
    if let Some(q) = pick(app, &proj, p) {
        return q;
    }
    let (o, d) = proj.ray(p);
    let cam = &app.session.camera;
    let n = cam.back();
    let den = d.dot(n);
    if den.abs() > 1e-9 { o + d * ((cam.target - o).dot(n) / den) } else { cam.target }
}

/// Zoom anchor: the point under the cursor when zooming to the cursor, else none (the centre).
fn zoom_anchor(app: &ResinApp, rect: Rect, hover: Option<Pos2>) -> Option<Vec3> {
    hover.filter(|_| app.prefs.zoom_to_cursor).map(|p| zoom_point(app, rect, p))
}

/// What an orbit turns about over empty space: the middle of the models, else the target.
fn pivot_under(app: &ResinApp, rect: Rect, at: Pos2) -> Vec3 {
    if let Some(p) = pick(app, &Proj { cam: app.session.camera, rect }, at) {
        return p;
    }
    let b = app.session.scene.bounds();
    if b.is_empty() { app.session.camera.target } else { b.center() }
}

/// Orbit by pixel deltas about the configured centre. The drag's whole turn so far is applied to
/// the camera it began with, so fast or erratic drags can't drift; anything else that moves the
/// camera meanwhile (a wheel zoom) starts the sum again from there.
fn orbit(app: &mut ResinApp, rect: Rect, dx: f64, dy: f64, press: Option<Pos2>) {
    let pivot = match app.prefs.orbit_around {
        OrbitAround::ViewCentre => None,
        OrbitAround::Origin => Some(Vec3::ZERO),
        OrbitAround::Cursor => Some(match app.nav.orbit_pivot {
            Some(p) => p,
            None => {
                let p = pivot_under(app, rect, press.unwrap_or(rect.center()));
                app.nav.orbit_pivot = Some(p);
                p
            }
        }),
    };
    let cam = app.session.camera;
    let from = match app.nav.orbit_from {
        Some(f) if f.last == cam && f.pivot == pivot => f,
        _ => OrbitFrom { start: cam, dx: 0.0, dy: 0.0, pivot, last: cam },
    };
    let (dx, dy) = (from.dx + dx, from.dy + dy);
    let mut next = from.start;
    match pivot {
        Some(p) => next.orbit_about(dx, dy, p),
        None => next.orbit(dx, dy),
    }
    app.session.camera = next;
    app.nav.orbit_from = Some(OrbitFrom { dx, dy, last: next, ..from });
}

/// A middle double-click goes home or fits, as the preferences say.
fn middle_double_click(app: &mut ResinApp, ui: &egui::Ui, inside: bool, rect: Rect) {
    use egui::PointerButton::{Extra1, Extra2, Middle, Primary, Secondary};
    let (double, releases, middle, other, dragging) = ui.input(|i| {
        (
            i.pointer.button_double_clicked(Middle),
            i.events.iter().filter(|e| matches!(e, egui::Event::PointerButton { button: Middle, pressed: false, .. })).count(),
            i.pointer.button_clicked(Middle),
            [Primary, Secondary, Extra1, Extra2].into_iter().any(|b| i.pointer.button_clicked(b)),
            i.pointer.is_decidedly_dragging(),
        )
    });
    // egui pairs clicks of any buttons; the first click must have been the middle one too (or
    // both arrive in this frame).
    let go = double && inside && !other && (app.nav.middle_clicked || releases >= 2);
    app.nav.middle_clicked = if other || dragging || go { false } else { middle || app.nav.middle_clicked };
    if !go {
        return;
    }
    let aspect = f64::from(rect.aspect_ratio());
    match app.prefs.middle_double_click.as_str() {
        "home" => app.animate_home(aspect),
        "fit" => app.animate_command("view.fit", json!({ "aspect": aspect })),
        _ => {}
    }
}

/// Modifier-only motion (the Touchpad mapping): the pointer's movement with Shift or Alt held.
fn modifier_motion(app: &ResinApp, ui: &egui::Ui, inside: bool) -> Option<(NavMode, Vec2)> {
    let (mods, delta, any_down) = ui.input(|i| (i.modifiers, i.pointer.delta(), i.pointer.any_down()));
    if !inside || any_down || ui.ctx().text_edit_focused() {
        return None;
    }
    nav_preset(&app.prefs.nav).motion_mode(mods).map(|m| (m, delta))
}

/// Camera input for one frame over `rect`: the mouse mapping's buttons and modifiers, the wheel,
/// trackpad and pinch zoom (to the cursor), a held Space to pan, and the middle double-click.
pub fn handle(app: &mut ResinApp, ui: &mut egui::Ui, rect: Rect, resp: &egui::Response) {
    let (hover, mods, middle, secondary_down, primary_down, primary_pressed, primary_released, delta) = ui.input(|i| {
        (
            i.pointer.hover_pos(),
            i.modifiers,
            i.pointer.middle_down(),
            i.pointer.secondary_down(),
            i.pointer.primary_down(),
            i.pointer.primary_pressed(),
            i.pointer.primary_released(),
            i.pointer.delta(),
        )
    });
    // Floating windows and menus own input over the model; the pointer must be in the viewport
    // with nothing above it.
    let inside = ui.is_enabled() && resp.contains_pointer() && ui.rect_contains_pointer(rect) && hover.is_some();
    middle_double_click(app, ui, inside, rect);

    // ---- wheel, trackpad, pinch ----
    let trackpad_pan = app.prefs.trackpad_pans();
    let options = ui.ctx().options(|o| o.input_options);
    let page_height = ui.ctx().content_rect().height();
    let trackpad_orbit = trackpad_pan && app.prefs.nav == "fusion";
    let gesture = &mut app.nav.trackpad_gesture;
    let raw = ui.input(|i| immediate_wheel(i, &options, page_height, gesture));
    if !ui.input(|i| i.is_scrolling()) {
        // With no scroll activity no gesture can be mid-flight (its End was lost); a stuck flag
        // would pan future mouse input.
        app.nav.trackpad_gesture = false;
    }
    let pan = if trackpad_pan { raw.pan + if trackpad_orbit { Vec2::ZERO } else { raw.shift_pan } } else { Vec2::ZERO } + raw.mouse_pan;
    let scroll = raw.zoom + if trackpad_pan { 0.0 } else { raw.point_zoom };
    let pinch = ui.input(|i| pinch_factor(i, &options, page_height));
    let h = f64::from(rect.height().max(1.0));
    if inside && pinch != 1.0 {
        // Spread zooms in: the camera distance shrinks by the pinch factor.
        let f = f64::from(pinch).powf(-app.prefs.zoom_sign());
        let anchor = zoom_anchor(app, rect, hover);
        app.cancel_view_animation();
        app.session.camera.zoom_at(f, anchor);
    }
    if inside && scroll.is_finite() && scroll != 0.0 {
        let f = (-f64::from(scroll) * 0.0015 * f64::from(app.prefs.wheel_zoom_speed) * app.prefs.zoom_sign()).exp();
        let anchor = zoom_anchor(app, rect, hover);
        app.cancel_view_animation();
        app.session.camera.zoom_at(f, anchor);
    }
    // Two-finger scroll pans the model with the fingers.
    if inside && pan.is_finite() && pan != Vec2::ZERO {
        app.cancel_view_animation();
        app.session.camera.pan(f64::from(pan.x), f64::from(pan.y), h);
    }
    if inside && trackpad_orbit && raw.shift_pan != Vec2::ZERO {
        app.cancel_view_animation();
        orbit(app, rect, f64::from(raw.shift_pan.x), f64::from(raw.shift_pan.y), hover);
    }

    // ---- buttons ----
    // A left press with the mapping's keys (Alias: Shift+Alt) navigates until the button is let
    // go (kept through the release frame, so the end of the drag doesn't select either). Ctrl+left
    // drag orbits in every mapping (Cmd on macOS); a plain left click picks supports
    // (`supports::handle`).
    if primary_pressed && inside && !app.gizmo_grab {
        let preset = app.prefs.nav_mode(NavButton::Left, mods);
        let ctrl_orbit = (mods.ctrl || mods.command) && !mods.shift && !mods.alt;
        app.nav.primary_nav = preset.or(ctrl_orbit.then_some(NavMode::Orbit));
    } else if !primary_down && !primary_released {
        app.nav.primary_nav = None;
    }
    // Hold Space for a momentary pan, the hand tool of design apps. Space still types in a text
    // field.
    let space_pan = (inside || primary_down) && !ui.ctx().text_edit_focused() && ui.input(|i| i.key_down(egui::Key::Space));
    if space_pan {
        ui.ctx().set_cursor_icon(if primary_down { egui::CursorIcon::Grabbing } else { egui::CursorIcon::Grab });
    }
    let primary_nav = if space_pan { Some(NavMode::Pan) } else { app.nav.primary_nav };
    // The right button also clicks (a future context menu), so it navigates only once the press
    // can no longer be a click. On that frame the drag catches up with the whole way from the
    // press, so none of it is lost.
    let (right_drag, travel) = ui.input(|i| {
        let drag = secondary_down && !i.pointer.could_any_button_be_click();
        (drag, i.pointer.press_origin().zip(i.pointer.latest_pos()).map(|(from, to)| to - from))
    });
    let delta = match travel {
        Some(travel) if right_drag && !app.nav.right_drag && !middle && travel.is_finite() => travel,
        _ => delta,
    };
    app.nav.right_drag = right_drag;
    let button = if middle {
        Some(NavButton::Middle)
    } else if right_drag {
        Some(NavButton::Right)
    } else {
        None
    };
    // An orbit started on the viewport owns its button until release.
    let captured = app.nav.orbit_from.is_some() && button.is_some();
    let dragging = resp.dragged() || (inside && button.is_some()) || captured;
    let motion = modifier_motion(app, ui, inside);
    let mode = motion.map(|(m, _)| m).or_else(|| {
        if !dragging {
            return None;
        }
        match button {
            Some(b) => app.prefs.nav_mode(b, mods),
            None if primary_down => primary_nav,
            None => None,
        }
    });
    let (mode_delta, press) = match motion {
        Some((_, d)) => (d, hover),
        None => (delta, ui.input(|i| i.pointer.press_origin())),
    };
    if mode_delta != Vec2::ZERO {
        match mode {
            Some(NavMode::Orbit) => {
                app.cancel_view_animation();
                orbit(app, rect, f64::from(mode_delta.x), f64::from(mode_delta.y), press);
            }
            Some(NavMode::Pan) => {
                app.cancel_view_animation();
                app.session.camera.pan(f64::from(mode_delta.x), f64::from(mode_delta.y), h);
            }
            Some(NavMode::Zoom) => {
                app.cancel_view_animation();
                // Drag-zoom holds the press point: following the moving pointer would slide the
                // zoom away from what was pressed on.
                let anchor = press.or(hover).map(|p| zoom_point(app, rect, p)).filter(|_| app.prefs.zoom_to_cursor);
                let sign = if nav_preset(&app.prefs.nav).reverse_drag_zoom { -1.0 } else { 1.0 };
                app.session.camera.zoom_at((f64::from(mode_delta.y) * 0.01 * sign).exp(), anchor);
            }
            None => {}
        }
    }
    if !dragging && motion.is_none() && !(trackpad_orbit && app.nav.trackpad_gesture) {
        app.nav.orbit_pivot = None;
        app.nav.orbit_from = None;
    }
    if inside && app.prefs.nav == TOUCHPAD_NAV && motion.is_some() {
        ui.ctx().request_repaint();
    }
    if !app.session.camera.is_valid() {
        app.session.camera = Camera::default();
        app.session.camera.fit(&app.session.scene.bounds());
    }
}

/// What the pointer did to a flat view this frame: a pan in points, a zoom factor (above 1
/// zooms in) and the point it zooms toward.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Motion2d {
    pub pan: Vec2,
    pub zoom: f64,
    pub at: Option<Pos2>,
}

/// Input for a flat view over `rect` (the 2D layer view), read as [`handle`] reads it for the
/// camera: the wheel, trackpad and pinch zoom toward the pointer, two-finger scroll pans, and a
/// drag does what the mouse mapping binds its button to. A flat view does not turn, so an orbit
/// binding pans it.
pub fn handle_2d(app: &mut ResinApp, ui: &egui::Ui, rect: Rect, resp: &egui::Response) -> Motion2d {
    let (hover, mods) = ui.input(|i| (i.pointer.hover_pos(), i.modifiers));
    let inside = ui.is_enabled() && resp.contains_pointer() && ui.rect_contains_pointer(rect) && hover.is_some();
    let mut m = Motion2d { pan: Vec2::ZERO, zoom: 1.0, at: hover };
    let options = ui.ctx().options(|o| o.input_options);
    let page_height = ui.ctx().content_rect().height();
    let gesture = &mut app.nav.trackpad_gesture;
    let raw = ui.input(|i| immediate_wheel(i, &options, page_height, gesture));
    if !ui.input(|i| i.is_scrolling()) {
        app.nav.trackpad_gesture = false;
    }
    let sign = app.prefs.zoom_sign();
    if inside {
        let trackpad_pan = app.prefs.trackpad_pans();
        let pan = if trackpad_pan { raw.pan + raw.shift_pan } else { Vec2::ZERO } + raw.mouse_pan;
        let scroll = raw.zoom + if trackpad_pan { 0.0 } else { raw.point_zoom };
        if scroll.is_finite() && scroll != 0.0 {
            m.zoom *= (f64::from(scroll) * 0.0015 * f64::from(app.prefs.wheel_zoom_speed) * sign).exp();
        }
        let pinch = ui.input(|i| pinch_factor(i, &options, page_height));
        if pinch != 1.0 {
            m.zoom *= f64::from(pinch).powf(sign);
        }
        if pan.is_finite() {
            m.pan += pan;
        }
    }
    let delta = resp.drag_delta();
    if delta != Vec2::ZERO && delta.is_finite() {
        let space = !ui.ctx().text_edit_focused() && ui.input(|i| i.key_down(egui::Key::Space));
        let mode = if resp.dragged_by(egui::PointerButton::Middle) {
            app.prefs.nav_mode(NavButton::Middle, mods)
        } else if resp.dragged_by(egui::PointerButton::Secondary) {
            app.prefs.nav_mode(NavButton::Right, mods)
        } else if resp.dragged_by(egui::PointerButton::Primary) {
            let ctrl = (mods.ctrl || mods.command) && !mods.shift && !mods.alt;
            if space { Some(NavMode::Pan) } else { app.prefs.nav_mode(NavButton::Left, mods).or(ctrl.then_some(NavMode::Orbit)) }
        } else {
            None
        };
        match mode {
            Some(NavMode::Pan | NavMode::Orbit) => m.pan += delta,
            Some(NavMode::Zoom) => {
                let reverse = if nav_preset(&app.prefs.nav).reverse_drag_zoom { -1.0 } else { 1.0 };
                m.zoom *= (-f64::from(delta.y) * 0.01 * reverse).exp();
                m.at = ui.input(|i| i.pointer.press_origin()).or(hover);
            }
            None => {}
        }
    }
    m
}

/// View keys: the arrows turn the view 15° (with Shift a quarter turn) and Ctrl+arrows pan it a
/// tenth of the view. Not while a widget has the keyboard. Returns whether the key was used.
pub fn view_key(app: &mut ResinApp, ctx: &egui::Context, key: egui::Key, mods: egui::Modifiers) -> bool {
    use egui::Key;
    if mods.alt || ctx.text_edit_focused() {
        return false;
    }
    let (direction, dx, dy): (&str, f32, f32) = match key {
        Key::ArrowLeft => ("left", -1.0, 0.0),
        Key::ArrowRight => ("right", 1.0, 0.0),
        Key::ArrowUp => ("up", 0.0, -1.0),
        Key::ArrowDown => ("down", 0.0, 1.0),
        _ => return false,
    };
    if mods.ctrl || mods.command {
        if let Some(rect) = app.viewport_rect {
            // The model moves with the arrow, as it does under a pan drag.
            let step = rect.height() * 0.1;
            app.cancel_view_animation();
            app.session.camera.pan(f64::from(dx * step), f64::from(dy * step), f64::from(rect.height().max(1.0)));
        }
    } else {
        let degrees = if mods.shift { 90.0 } else { 15.0 };
        app.animate_command("view.turn", json!({ "direction": direction, "degrees": degrees }));
    }
    true
}
