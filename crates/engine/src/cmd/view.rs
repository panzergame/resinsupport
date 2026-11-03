use resin_render::StandardView;
use serde_json::{Value, json};

use super::{num_param, str_param};
use crate::{EngineError, Result, Session};

/// Vertical field of view in perspective.
pub const PERSPECTIVE_FOV: f64 = 45.0 * std::f64::consts::PI / 180.0;

fn camera(s: &Session) -> Value {
    json!({ "camera": s.camera })
}

pub(crate) fn fit(s: &mut Session, p: &Value) -> Result<Value> {
    let aspect = num_param(p, "aspect")?.unwrap_or(1.0);
    let b = s.scene.bounds();
    // An empty scene frames the origin's neighbourhood, as a fresh view does.
    let b = if b.is_empty() {
        resin_geom::Aabb3 { min: resin_geom::Vec3::new(-50.0, -50.0, 0.0), max: resin_geom::Vec3::new(50.0, 50.0, 50.0) }
    } else {
        b
    };
    s.camera.fit_aspect(&b, aspect);
    Ok(camera(s))
}

pub(crate) fn standard(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "view")?;
    let v = StandardView::parse(name).ok_or_else(|| EngineError::BadParams(format!("unknown view `{name}`")))?;
    s.camera.set_view(v);
    Ok(camera(s))
}

pub(crate) fn projection(s: &mut Session, p: &Value) -> Result<Value> {
    let perspective = match str_param(p, "mode")? {
        "perspective" => true,
        "orthographic" => false,
        "toggle" => !(s.camera.fov > 0.0),
        other => return Err(EngineError::BadParams(format!("unknown projection `{other}`"))),
    };
    s.camera.fov = if perspective { PERSPECTIVE_FOV } else { 0.0 };
    Ok(camera(s))
}

pub(crate) fn turn(s: &mut Session, p: &Value) -> Result<Value> {
    let degrees = num_param(p, "degrees")?.unwrap_or(15.0);
    if degrees.abs() > 360.0 {
        return Err(EngineError::BadParams("`degrees` must be within ±360".into()));
    }
    // The same turn a pointer drag in that direction makes: the model follows the arrow.
    let (dx, dy) = match str_param(p, "direction")? {
        "left" => (-1.0, 0.0),
        "right" => (1.0, 0.0),
        "up" => (0.0, -1.0),
        "down" => (0.0, 1.0),
        other => return Err(EngineError::BadParams(format!("unknown direction `{other}`"))),
    };
    // `Camera::orbit` turns 0.008 radians per pixel.
    let px = degrees.to_radians() / 0.008;
    s.camera.orbit(dx * px, dy * px);
    Ok(camera(s))
}
