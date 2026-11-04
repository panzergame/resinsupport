// The lenient reading of stored settings follows SolidCraft's `hole_presets::Library` (MIT OR
// Apache-2.0), crates/ui-egui/src/hole_presets.rs at 4bdffef0; see ATTRIBUTION.md.

//! The settings for manually placed supports: three presets (Light, Medium, Heavy), each sizing
//! a support's tip, body, base and raft, and which one is in use. They live on the session beside
//! the camera, outside the scene, so changing them is never an undo step.
//!
//! The ranges and the cross-field rules are in one place, [`SupportPreset::validate`]: the
//! `support.*` commands and the stored preferences go through them alike. Stored settings are read
//! leniently ([`SupportSettings::from_stored`]): a field that does not read falls back to its
//! default on its own.

use std::ops::RangeInclusive;

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use crate::{EngineError, Result};

/// One of the three presets.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PresetId {
    Light,
    #[default]
    Medium,
    Heavy,
}

impl PresetId {
    pub const ALL: [PresetId; 3] = [PresetId::Light, PresetId::Medium, PresetId::Heavy];

    pub fn id(self) -> &'static str {
        match self {
            PresetId::Light => "light",
            PresetId::Medium => "medium",
            PresetId::Heavy => "heavy",
        }
    }

    pub fn parse(s: &str) -> Option<PresetId> {
        PresetId::ALL.into_iter().find(|p| p.id() == s)
    }

    /// The built-in values: what Restore defaults puts back.
    pub fn defaults(self) -> SupportPreset {
        let (contact, penetration, length, upper, body, segments, base, base_h, raft, margin) = match self {
            PresetId::Light => (0.25, 0.1, 1.5, 0.5, 0.7, 8, 3.0, 0.6, 0.6, 1.5),
            PresetId::Medium => (0.35, 0.15, 2.0, 0.7, 1.0, 12, 4.2, 0.8, 1.0, 2.0),
            PresetId::Heavy => (0.5, 0.25, 2.6, 1.0, 1.4, 16, 5.6, 1.0, 1.4, 2.5),
        };
        SupportPreset {
            tip: Tip { contact_shape: ContactShape::Cone, contact_diameter: contact, penetration, length, upper_diameter: upper },
            body: Body { shape: BodyShape::Cylinder, diameter: body, max_angle: 40.0, clearance: 0.5, segments },
            base: Base { diameter: base, height: base_h },
            raft: Raft { enabled: true, thickness: raft, margin, chamfer_angle: 45.0 },
        }
    }
}

/// What touches the model.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ContactShape {
    Cone,
    Sphere,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum BodyShape {
    Cylinder,
    Prism,
}

impl ContactShape {
    pub const ALL: [ContactShape; 2] = [ContactShape::Cone, ContactShape::Sphere];
    pub fn id(self) -> &'static str {
        match self {
            ContactShape::Cone => "cone",
            ContactShape::Sphere => "sphere",
        }
    }
}

impl BodyShape {
    pub const ALL: [BodyShape; 2] = [BodyShape::Cylinder, BodyShape::Prism];
    pub fn id(self) -> &'static str {
        match self {
            BodyShape::Cylinder => "cylinder",
            BodyShape::Prism => "prism",
        }
    }
}

/// Millimetres unless named otherwise.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Tip {
    pub contact_shape: ContactShape,
    pub contact_diameter: f64,
    /// How deep the contact sinks into the model.
    pub penetration: f64,
    pub length: f64,
    /// Where the tip meets the body.
    pub upper_diameter: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Body {
    pub shape: BodyShape,
    pub diameter: f64,
    /// Degrees from vertical.
    pub max_angle: f64,
    /// The gap the body and base keep from every model; only the tip touches one.
    pub clearance: f64,
    /// Sides of a prism, facets of a cylinder.
    pub segments: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Base {
    pub diameter: f64,
    pub height: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Raft {
    pub enabled: bool,
    pub thickness: f64,
    /// How far the raft reaches past the bases.
    pub margin: f64,
    /// Degrees of the raft's edge from the plate; 90 is a straight wall.
    pub chamfer_angle: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct SupportPreset {
    pub tip: Tip,
    pub body: Body,
    pub base: Base,
    pub raft: Raft,
}

/// The groups of a preset, as named in parameters and stored settings.
pub const GROUPS: [&str; 4] = ["tip", "body", "base", "raft"];

/// Every number's group, key and accepted range: beyond them a value is a typo, not an intent.
const NUMBERS: &[(&str, &str, f64, f64)] = &[
    ("tip", "contact_diameter", 0.05, 2.0),
    ("tip", "penetration", 0.0, 1.0),
    ("tip", "length", 0.2, 10.0),
    ("tip", "upper_diameter", 0.1, 5.0),
    ("body", "diameter", 0.2, 10.0),
    ("body", "max_angle", 0.0, 80.0),
    ("body", "clearance", 0.0, 5.0),
    ("base", "diameter", 0.5, 30.0),
    ("base", "height", 0.1, 5.0),
    ("raft", "thickness", 0.1, 5.0),
    ("raft", "margin", 0.0, 20.0),
    ("raft", "chamfer_angle", 15.0, 90.0),
];

pub const SEGMENTS: RangeInclusive<u32> = 3..=64;

/// The accepted range of number `group.key`.
pub fn range(group: &str, key: &str) -> Option<RangeInclusive<f64>> {
    NUMBERS.iter().find(|(g, k, ..)| *g == group && *k == key).map(|(.., lo, hi)| *lo..=*hi)
}

/// Why one field was not set.
enum FieldError {
    /// No such field: a typo in parameters, a field from another version in stored settings.
    Unknown,
    Bad(String),
}

impl SupportPreset {
    fn number_mut(&mut self, group: &str, key: &str) -> Option<&mut f64> {
        Some(match (group, key) {
            ("tip", "contact_diameter") => &mut self.tip.contact_diameter,
            ("tip", "penetration") => &mut self.tip.penetration,
            ("tip", "length") => &mut self.tip.length,
            ("tip", "upper_diameter") => &mut self.tip.upper_diameter,
            ("body", "diameter") => &mut self.body.diameter,
            ("body", "max_angle") => &mut self.body.max_angle,
            ("body", "clearance") => &mut self.body.clearance,
            ("base", "diameter") => &mut self.base.diameter,
            ("base", "height") => &mut self.base.height,
            ("raft", "thickness") => &mut self.raft.thickness,
            ("raft", "margin") => &mut self.raft.margin,
            ("raft", "chamfer_angle") => &mut self.raft.chamfer_angle,
            _ => return None,
        })
    }

    /// Set field `group.key` to `v`, checking its type and range (not the other fields).
    fn set(&mut self, group: &str, key: &str, v: &Value) -> std::result::Result<(), FieldError> {
        let bad = |what: &str| FieldError::Bad(format!("`{group}.{key}` must be {what}"));
        match (group, key) {
            ("tip", "contact_shape") => {
                self.tip.contact_shape = match v.as_str() {
                    Some("cone") => ContactShape::Cone,
                    Some("sphere") => ContactShape::Sphere,
                    _ => return Err(bad("\"cone\" or \"sphere\"")),
                }
            }
            ("body", "shape") => {
                self.body.shape = match v.as_str() {
                    Some("cylinder") => BodyShape::Cylinder,
                    Some("prism") => BodyShape::Prism,
                    _ => return Err(bad("\"cylinder\" or \"prism\"")),
                }
            }
            ("body", "segments") => {
                self.body.segments = v
                    .as_u64()
                    .and_then(|n| u32::try_from(n).ok())
                    .filter(|n| SEGMENTS.contains(n))
                    .ok_or_else(|| bad(&format!("an integer from {} to {}", SEGMENTS.start(), SEGMENTS.end())))?
            }
            ("raft", "enabled") => self.raft.enabled = v.as_bool().ok_or_else(|| bad("true or false"))?,
            _ => {
                let r = range(group, key).ok_or(FieldError::Unknown)?;
                let x = v.as_f64().filter(|x| r.contains(x)).ok_or_else(|| bad(&format!("a number from {} to {}", r.start(), r.end())))?;
                *self.number_mut(group, key).ok_or(FieldError::Unknown)? = x;
            }
        }
        Ok(())
    }

    /// Every field in its range, and the sizes in an order a support can be built in.
    pub fn validate(&self) -> std::result::Result<(), String> {
        let mut copy = *self;
        for (group, key, lo, hi) in NUMBERS {
            let x = copy.number_mut(group, key).map_or(f64::NAN, |x| *x);
            if !(x >= *lo && x <= *hi) {
                return Err(format!("`{group}.{key}` must be a number from {lo} to {hi}"));
            }
        }
        if !SEGMENTS.contains(&self.body.segments) {
            return Err(format!("`body.segments` must be from {} to {}", SEGMENTS.start(), SEGMENTS.end()));
        }
        let (tip, body, base) = (&self.tip, &self.body, &self.base);
        if tip.contact_diameter > tip.upper_diameter {
            return Err("the contact diameter must not exceed the tip's upper diameter".into());
        }
        if tip.upper_diameter > body.diameter {
            return Err("the tip's upper diameter must not exceed the body diameter".into());
        }
        if body.diameter > base.diameter {
            return Err("the body diameter must not exceed the base diameter".into());
        }
        if tip.penetration > tip.length {
            return Err("the penetration must not exceed the tip length".into());
        }
        Ok(())
    }

    /// A stored preset, read field by field over `id`'s defaults: a field that doesn't read keeps
    /// its default and is named in `problems` (`light.tip.length`); a preset whose fields read
    /// but don't fit together falls back whole.
    fn from_stored(id: PresetId, v: &Value, problems: &mut Vec<String>) -> SupportPreset {
        let mut p = id.defaults();
        let Some(o) = v.as_object() else {
            problems.push(id.id().into());
            return p;
        };
        for group in GROUPS {
            match o.get(group) {
                None => {}
                Some(Value::Object(fields)) => {
                    for (key, value) in fields.iter().take(64) {
                        if let Err(FieldError::Bad(_)) = p.set(group, key, value) {
                            problems.push(format!("{}.{group}.{key}", id.id()));
                        }
                    }
                }
                Some(_) => problems.push(format!("{}.{group}", id.id())),
            }
        }
        if p.validate().is_err() {
            problems.push(id.id().into());
            return id.defaults();
        }
        p
    }
}

/// The three presets and which one is in use.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct SupportSettings {
    pub active: PresetId,
    pub light: SupportPreset,
    pub medium: SupportPreset,
    pub heavy: SupportPreset,
}

impl Default for SupportSettings {
    fn default() -> Self {
        SupportSettings {
            active: PresetId::default(),
            light: PresetId::Light.defaults(),
            medium: PresetId::Medium.defaults(),
            heavy: PresetId::Heavy.defaults(),
        }
    }
}

impl SupportSettings {
    pub fn preset(&self, id: PresetId) -> &SupportPreset {
        match id {
            PresetId::Light => &self.light,
            PresetId::Medium => &self.medium,
            PresetId::Heavy => &self.heavy,
        }
    }

    fn preset_mut(&mut self, id: PresetId) -> &mut SupportPreset {
        match id {
            PresetId::Light => &mut self.light,
            PresetId::Medium => &mut self.medium,
            PresetId::Heavy => &mut self.heavy,
        }
    }

    /// The preset in use.
    pub fn current(&self) -> &SupportPreset {
        self.preset(self.active)
    }

    /// The rules the commands and the stored preferences share.
    pub fn validate(&self) -> Result<()> {
        for id in PresetId::ALL {
            self.preset(id).validate().map_err(|why| EngineError::BadParams(format!("{} preset: {why}", id.id())))?;
        }
        Ok(())
    }

    /// Merge `support.configure`'s parameters: `preset?`, `active?` and partial `tip?`, `body?`,
    /// `base?`, `raft?`. All or nothing: the result is validated as a whole before it replaces
    /// these settings. `preset` defaults to the active preset (the new one when `active` is given).
    pub(crate) fn configure(&mut self, p: &Value) -> Result<()> {
        let empty = serde_json::Map::new();
        let o = match p {
            Value::Null => &empty,
            Value::Object(o) => o,
            _ => return Err(EngineError::BadParams("parameters must be an object".into())),
        };
        if let Some(k) = o.keys().find(|k| !matches!(k.as_str(), "preset" | "active") && !GROUPS.contains(&k.as_str())) {
            return Err(EngineError::BadParams(format!("unknown parameter `{k}`")));
        }
        let mut next = *self;
        if let Some(active) = preset_param(p, "active")? {
            next.active = active;
        }
        let target = preset_param(p, "preset")?.unwrap_or(next.active);
        let preset = next.preset_mut(target);
        for group in GROUPS {
            match o.get(group) {
                None | Some(Value::Null) => {}
                Some(Value::Object(fields)) => {
                    for (key, v) in fields {
                        preset.set(group, key, v).map_err(|e| match e {
                            FieldError::Unknown => EngineError::BadParams(format!("unknown setting `{group}.{key}`")),
                            FieldError::Bad(why) => EngineError::BadParams(why),
                        })?;
                    }
                }
                Some(_) => return Err(EngineError::BadParams(format!("`{group}` must be an object of settings"))),
            }
        }
        next.validate()?;
        *self = next;
        Ok(())
    }

    /// Put preset `id` back to its built-in values.
    pub(crate) fn reset(&mut self, id: PresetId) {
        *self.preset_mut(id) = id.defaults();
    }

    /// Settings read leniently from storage, and what could not be read (`active`,
    /// `light.tip.length`, `heavy` for a whole preset), which fell back to defaults. Unknown keys
    /// are ignored.
    pub fn from_stored(v: &Value) -> (SupportSettings, Vec<String>) {
        let mut s = SupportSettings::default();
        let mut problems = Vec::new();
        let Some(o) = v.as_object() else {
            if !v.is_null() {
                problems.push("support_settings".into());
            }
            return (s, problems);
        };
        if let Some(a) = o.get("active") {
            match a.as_str().and_then(PresetId::parse) {
                Some(id) => s.active = id,
                None => problems.push("active".into()),
            }
        }
        for id in PresetId::ALL {
            if let Some(pv) = o.get(id.id()) {
                *s.preset_mut(id) = SupportPreset::from_stored(id, pv, &mut problems);
            }
        }
        (s, problems)
    }
}

/// Never fails: what doesn't read falls back to its default, with a warning in the log.
impl<'de> Deserialize<'de> for SupportSettings {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        let v = Value::deserialize(deserializer).unwrap_or(Value::Null);
        let (s, problems) = SupportSettings::from_stored(&v);
        for p in problems {
            log::warn!("support settings: `{p}` could not be read; using its default");
        }
        Ok(s)
    }
}

/// An optional preset name.
pub(crate) fn preset_param(p: &Value, key: &str) -> Result<Option<PresetId>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => {
            v.as_str().and_then(PresetId::parse).map(Some).ok_or_else(|| EngineError::BadParams(format!("`{key}` must be light, medium or heavy")))
        }
    }
}
