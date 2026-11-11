// The lenient reading of stored settings follows SolidCraft's `hole_presets::Library` (MIT OR
// Apache-2.0), crates/ui-egui/src/hole_presets.rs at 4bdffef0; see ATTRIBUTION.md.

//! The print configuration: the printer profile and the print settings ([`resin_slice`]). It
//! lives on the session beside the camera and the support settings, outside the scene, so
//! changing it is never an undo step.
//!
//! One JSON shape serves `print.configure`'s parameters, `print.settings`' answer and the stored
//! preferences: `{ printer, layers, exposure, motion, slicing, estimates }`, each an object of
//! fields. Ranges and cross-field rules are [`resin_slice::validate`]'s. Stored settings are read
//! leniently ([`PrintConfig::from_stored`]): a field that does not read falls back to its default
//! on its own.

use resin_slice::{GROUPS, PrintSettings, PrinterProfile, field};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use crate::{EngineError, Result};

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct PrintConfig {
    pub printer: PrinterProfile,
    #[serde(flatten)]
    pub settings: PrintSettings,
}

/// Why one field was not set.
enum FieldError {
    /// No such field: a typo in parameters, a field from another version in stored settings.
    Unknown,
    Bad(String),
}

impl PrintConfig {
    /// The rules the commands and the stored preferences share.
    pub fn validate(&self) -> Result<()> {
        resin_slice::validate(&self.printer, &self.settings).map_err(EngineError::BadParams)
    }

    /// Set field `group.key` to `v`, checking its type and range (not the other fields).
    fn set(&mut self, group: &str, key: &str, v: &Value) -> std::result::Result<(), FieldError> {
        let bad = |what: &str| FieldError::Bad(format!("`{group}.{key}` must be {what}"));
        match (group, key) {
            ("printer", "name") => self.printer.name = v.as_str().ok_or_else(|| bad("a string"))?.to_owned(),
            ("printer", "mirror_x") => self.printer.mirror_x = v.as_bool().ok_or_else(|| bad("true or false"))?,
            ("printer", "mirror_y") => self.printer.mirror_y = v.as_bool().ok_or_else(|| bad("true or false"))?,
            _ => {
                let f = field(group, key).ok_or(FieldError::Unknown)?;
                let x = v.as_f64().ok_or_else(|| FieldError::Bad(f.describe()))?;
                if !f.set(&mut self.printer, &mut self.settings, x) {
                    return Err(FieldError::Bad(f.describe()));
                }
            }
        }
        Ok(())
    }

    /// Merge `print.configure`'s parameters: partial `printer`, `layers`, `exposure`, `motion`,
    /// `slicing`, `estimates`. All or nothing: the result is validated as a whole before it
    /// replaces this configuration.
    pub(crate) fn configure(&mut self, p: &Value) -> Result<()> {
        let empty = serde_json::Map::new();
        let o = match p {
            Value::Null => &empty,
            Value::Object(o) => o,
            _ => return Err(EngineError::BadParams("parameters must be an object".into())),
        };
        if let Some(k) = o.keys().find(|k| !GROUPS.contains(&k.as_str())) {
            return Err(EngineError::BadParams(format!("unknown parameter `{k}`")));
        }
        let mut next = self.clone();
        for group in GROUPS {
            match o.get(group) {
                None | Some(Value::Null) => {}
                Some(Value::Object(fields)) => {
                    for (key, v) in fields {
                        next.set(group, key, v).map_err(|e| match e {
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

    /// Put the print settings back to their built-in values; the printer profile stays.
    pub(crate) fn reset(&mut self) {
        self.settings = PrintSettings::default();
    }

    /// A configuration read leniently from storage, and what could not be read
    /// (`exposure.normal`, `printer` for a whole group), which fell back to defaults. Unknown
    /// keys are ignored. Fields that read but don't fit together fall back a group at a time:
    /// the layers, then the printer, then everything.
    pub fn from_stored(v: &Value) -> (PrintConfig, Vec<String>) {
        let mut c = PrintConfig::default();
        let mut problems = Vec::new();
        let Some(o) = v.as_object() else {
            if !v.is_null() {
                problems.push("print_config".into());
            }
            return (c, problems);
        };
        for group in GROUPS {
            match o.get(group) {
                None => {}
                Some(Value::Object(fields)) => {
                    for (key, value) in fields.iter().take(64) {
                        if let Err(FieldError::Bad(_)) = c.set(group, key, value) {
                            problems.push(format!("{group}.{key}"));
                        }
                    }
                }
                Some(_) => problems.push(group.into()),
            }
        }
        if c.validate().is_ok() {
            return (c, problems);
        }
        let layers = PrintConfig { settings: PrintSettings { layers: PrintSettings::default().layers, ..c.settings }, ..c.clone() };
        if layers.validate().is_ok() {
            problems.push("layers".into());
            return (layers, problems);
        }
        let printer = PrintConfig { printer: PrinterProfile::default(), ..c };
        if printer.validate().is_ok() {
            problems.push("printer".into());
            return (printer, problems);
        }
        problems.push("print_config".into());
        (PrintConfig::default(), problems)
    }
}

/// Never fails: what doesn't read falls back to its default, with a warning in the log.
impl<'de> Deserialize<'de> for PrintConfig {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        let v = Value::deserialize(deserializer).unwrap_or(Value::Null);
        let (c, problems) = PrintConfig::from_stored(&v);
        for p in problems {
            log::warn!("print settings: `{p}` could not be read; using its default");
        }
        Ok(c)
    }
}
