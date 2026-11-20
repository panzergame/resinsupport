// The lenient reading of stored presets follows SolidCraft's `hole_presets::Library` (MIT OR
// Apache-2.0), crates/ui-egui/src/hole_presets.rs at 4bdffef0; see ATTRIBUTION.md.

//! Presets: the user's own named sets of a card's settings, across projects. One library per kind
//! ([`PresetKind`]: labels today), all of them in [`PresetLibraries`] on the session beside the
//! support settings, outside the scene, so a change to a library is never an undo step.
//!
//! What follows a preset (a model's label) names it and keeps the values itself, so a project
//! reads the same where the preset is unknown. Changing a followed preset's values through one
//! follower changes every follower of the scene in the same step ([`edited`],
//! [`Follow::relink`]); undoing that step puts the followers back but leaves the library alone.
//! Saving, renaming and deleting a preset are no undo step: what they do to followers (the link,
//! the new name) reaches every scene of the history, so no undo takes it back.
//! Names are compared ignoring case ([`same_preset_name`]); a library is sorted by name.
//!
//! A new kind: a [`PresetKind`] for its settings, a [`Follow`] for what follows it, a field of
//! [`PresetLibraries`], and its four commands (`<area>.preset.{delete,list,rename,save}`, run by
//! `cmd::preset`).

use std::fmt::Debug;

use resin_geom::{check_preset_name, same_preset_name};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

use crate::label_presets::LabelPresets;
use crate::{EngineError, Result, Session};

/// The most presets of one kind.
pub const MAX_PRESETS: usize = 64;

/// A kind of preset: what one holds.
pub trait PresetKind: 'static {
    /// The kind's name: its key in [`PresetLibraries`]' stored form, its commands' area's name.
    const KIND: &'static str;
    /// What a preset sets, stored beside its name.
    type Settings: Clone + Debug + PartialEq + Serialize + DeserializeOwned;
    /// Why `settings` cannot be a preset's, if they cannot.
    fn check(settings: &Self::Settings) -> std::result::Result<(), String>;
}

/// What follows a kind of preset, in the session: the half of a kind the commands need.
pub trait Follow: PresetKind + Sized {
    /// The parameters [`Follow::target`] reads, besides the commands' own.
    const TARGET_PARAMS: &'static [&'static str];
    /// What follows a preset: a model's id for a label.
    type Target: Serialize;
    fn library(s: &Session) -> &Presets<Self>;
    fn library_mut(s: &mut Session) -> &mut Presets<Self>;
    /// What `p` names (`id?` for a label, the one selected model by default).
    fn target(s: &Session, p: &Value) -> Result<Self::Target>;
    /// The target's settings now, refused when it has none (a model without a label).
    fn settings(s: &Session, t: &Self::Target) -> Result<Self::Settings>;
    /// Make the target, which has `preset`'s settings, follow it; no undo step, so a follower in
    /// the scene follows it in every scene of the history where it has those settings
    /// ([`Session::every_scene`]).
    fn link(s: &mut Session, t: &Self::Target, preset: &Preset<Self::Settings>) -> Result<()>;
    /// Make everything following the preset called `old` follow it as `new`, keeping its
    /// settings; no undo step, so in every scene of the history. The ids of those of the scene
    /// that changed.
    fn rename(s: &mut Session, old: &str, new: &str) -> Result<Vec<u64>>;
    /// Give everything following the preset called `old` `preset`'s name and settings, in the
    /// scene alone (part of the follower's edit, its undo step); the ids of those that changed.
    fn relink(s: &mut Session, old: &str, preset: &Preset<Self::Settings>) -> Result<Vec<u64>>;
}

/// A named set of settings.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Preset<S> {
    pub name: String,
    #[serde(flatten)]
    pub settings: S,
}

impl<S> Preset<S> {
    /// Whether this preset is the one called `name`.
    pub fn named(&self, name: &str) -> bool {
        same_preset_name(&self.name, name)
    }
}

/// One kind's presets, sorted by name ignoring case, their names unique ignoring case.
pub struct Presets<K: PresetKind>(Vec<Preset<K::Settings>>);

impl<K: PresetKind> Presets<K> {
    pub fn presets(&self) -> &[Preset<K::Settings>] {
        &self.0
    }

    pub fn find(&self, name: &str) -> Option<&Preset<K::Settings>> {
        self.0.iter().find(|p| p.named(name))
    }

    /// Add `preset`, refused when it does not validate, its name is taken or there are
    /// [`MAX_PRESETS`] already.
    pub(crate) fn add(&mut self, preset: Preset<K::Settings>) -> Result<()> {
        validate::<K>(&preset)?;
        if self.find(&preset.name).is_some() {
            return Err(EngineError::BadParams(format!("there is a {} preset named {} already", K::KIND, preset.name)));
        }
        if self.0.len() >= MAX_PRESETS {
            return Err(EngineError::BadParams(format!("there can be at most {MAX_PRESETS} {} presets", K::KIND)));
        }
        self.0.push(preset);
        self.sort();
        Ok(())
    }

    /// Give the preset called `name` `settings`, keeping its name as it was written; the preset.
    pub(crate) fn update(&mut self, name: &str, settings: K::Settings) -> Result<Preset<K::Settings>> {
        K::check(&settings).map_err(EngineError::BadParams)?;
        let p = self.0.iter_mut().find(|p| p.named(name)).ok_or_else(|| missing::<K>(name))?;
        p.settings = settings;
        Ok(p.clone())
    }

    /// Call the preset called `old` `new`, refused when another preset is called `new` (a change
    /// of case alone is a rename); the preset.
    pub(crate) fn rename(&mut self, old: &str, new: &str) -> Result<Preset<K::Settings>> {
        check_preset_name(new).map_err(EngineError::BadParams)?;
        let i = self.0.iter().position(|p| p.named(old)).ok_or_else(|| missing::<K>(old))?;
        if self.0.iter().enumerate().any(|(j, p)| j != i && p.named(new)) {
            return Err(EngineError::BadParams(format!("there is a {} preset named {new} already", K::KIND)));
        }
        let mut renamed = self.0.remove(i);
        renamed.name = new.into();
        self.0.push(renamed.clone());
        self.sort();
        Ok(renamed)
    }

    pub(crate) fn remove(&mut self, name: &str) -> Result<Preset<K::Settings>> {
        let i = self.0.iter().position(|p| p.named(name)).ok_or_else(|| missing::<K>(name))?;
        Ok(self.0.remove(i))
    }

    pub fn validate(&self) -> Result<()> {
        let mut checked = Presets::<K>::default();
        for p in &self.0 {
            checked.add(p.clone())?;
        }
        if checked != *self {
            return Err(EngineError::BadParams(format!("the {} presets must be sorted by name", K::KIND)));
        }
        Ok(())
    }

    /// Presets read leniently from storage, and what could not be read, under `path`
    /// (`presets.label.3` for the fourth row), which was left out: a row that does not read or
    /// validate, a name taken by an earlier row, rows past [`MAX_PRESETS`]. Unknown keys in a row
    /// are ignored.
    pub fn from_stored(v: &Value, path: &str) -> (Self, Vec<String>) {
        let mut presets = Self::default();
        let mut problems = Vec::new();
        let Some(rows) = v.as_array() else {
            if !v.is_null() {
                problems.push(path.into());
            }
            return (presets, problems);
        };
        for (i, row) in rows.iter().enumerate() {
            let read = serde_json::from_value::<Preset<K::Settings>>(row.clone()).map_err(|e| EngineError::BadParams(e.to_string()));
            if read.and_then(|p| presets.add(p)).is_err() {
                problems.push(format!("{path}.{i}"));
            }
        }
        (presets, problems)
    }

    fn sort(&mut self) {
        self.0.sort_by_key(|p| p.name.to_lowercase());
    }
}

fn validate<K: PresetKind>(p: &Preset<K::Settings>) -> Result<()> {
    check_preset_name(&p.name).and_then(|()| K::check(&p.settings)).map_err(EngineError::BadParams)
}

fn missing<K: PresetKind>(name: &str) -> EngineError {
    EngineError::BadParams(format!("there is no {} preset named {name}", K::KIND))
}

impl<K: PresetKind> Default for Presets<K> {
    fn default() -> Self {
        Presets(Vec::new())
    }
}

impl<K: PresetKind> Clone for Presets<K> {
    fn clone(&self) -> Self {
        Presets(self.0.clone())
    }
}

impl<K: PresetKind> PartialEq for Presets<K> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl<K: PresetKind> Debug for Presets<K> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Presets").field(&K::KIND).field(&self.0).finish()
    }
}

impl<K: PresetKind> Serialize for Presets<K> {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

/// `follower`'s preset, changed to `settings` when it follows a known one whose settings they are
/// not, and every other follower with it: what a follower's edit does. The ids of the other
/// followers that changed.
pub(crate) fn edited<K: Follow>(s: &mut Session, follows: Option<&str>, settings: &K::Settings) -> Result<Vec<u64>> {
    let Some(preset) = follows.and_then(|n| K::library(s).find(n)) else { return Ok(Vec::new()) };
    if preset.settings == *settings {
        return Ok(Vec::new());
    }
    let name = preset.name.clone();
    let preset = K::library_mut(s).update(&name, settings.clone())?;
    K::relink(s, &name, &preset)
}

/// Every kind's presets: one field per kind. Stored as one object, a key per kind, each read on
/// its own, so a kind that does not read never loses another.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct PresetLibraries {
    pub label: LabelPresets,
}

impl PresetLibraries {
    pub fn validate(&self) -> Result<()> {
        self.label.validate()
    }

    /// The libraries read leniently, and what could not be read (`presets.label.3`), as
    /// [`Presets::from_stored`].
    pub fn from_stored(v: &Value) -> (Self, Vec<String>) {
        let mut problems = Vec::new();
        if !v.is_null() && !v.is_object() {
            problems.push("presets".into());
        }
        let label = kind(v, &mut problems);
        (PresetLibraries { label }, problems)
    }
}

/// Kind `K`'s presets stored under `v`, its problems added to `problems`.
fn kind<K: PresetKind>(v: &Value, problems: &mut Vec<String>) -> Presets<K> {
    let (presets, p) = Presets::<K>::from_stored(v.get(K::KIND).unwrap_or(&Value::Null), &format!("presets.{}", K::KIND));
    problems.extend(p);
    presets
}

/// Never fails: what doesn't read is left out, with a warning in the log.
impl<'de> Deserialize<'de> for PresetLibraries {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        let v = Value::deserialize(deserializer).unwrap_or(Value::Null);
        let (libraries, problems) = PresetLibraries::from_stored(&v);
        for p in problems {
            log::warn!("presets: `{p}` could not be read; left out");
        }
        Ok(libraries)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// A kind of its own, so the library is tested apart from labels.
    struct Widths;

    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
    struct Width {
        width: f64,
    }

    impl PresetKind for Widths {
        const KIND: &'static str = "width";
        type Settings = Width;
        fn check(s: &Width) -> std::result::Result<(), String> {
            if (0.1..=10.0).contains(&s.width) { Ok(()) } else { Err("width out of range".into()) }
        }
    }

    fn preset(name: &str, width: f64) -> Preset<Width> {
        Preset { name: name.into(), settings: Width { width } }
    }

    fn names(l: &Presets<Widths>) -> Vec<&str> {
        l.presets().iter().map(|p| p.name.as_str()).collect()
    }

    #[test]
    fn names_are_unique_ignoring_case_and_kept_sorted() {
        let mut l = Presets::<Widths>::default();
        l.add(preset("beta", 2.0)).unwrap();
        l.add(preset("Alpha", 3.0)).unwrap();
        assert!(l.add(preset("ALPHA", 4.0)).is_err());
        assert!(l.add(preset(" x", 4.0)).is_err());
        assert!(l.add(preset("x", 0.0)).is_err());
        assert_eq!(names(&l), ["Alpha", "beta"]);
        assert_eq!(l.update("alpha", Width { width: 5.0 }).unwrap(), preset("Alpha", 5.0), "the name stays as it was written");
        assert!(l.update("alpha", Width { width: 50.0 }).is_err());
        assert!(l.update("gamma", Width { width: 5.0 }).is_err());
        assert!(l.remove("BETA").is_ok());
        assert!(l.remove("beta").is_err());
        assert!(l.validate().is_ok());
        for i in 1..MAX_PRESETS {
            l.add(preset(&format!("p{i:02}"), 2.0)).unwrap();
        }
        assert!(l.add(preset("one too many", 2.0)).is_err());
    }

    #[test]
    fn a_rename_keeps_the_settings_and_the_order() {
        let mut l = Presets::<Widths>::default();
        for (n, w) in [("a", 1.0), ("b", 2.0), ("c", 3.0)] {
            l.add(preset(n, w)).unwrap();
        }
        assert_eq!(l.rename("A", "d").unwrap(), preset("d", 1.0));
        assert_eq!(names(&l), ["b", "c", "d"]);
        assert_eq!(l.rename("d", "D").unwrap(), preset("D", 1.0), "a change of case");
        assert!(l.rename("D", "B").is_err(), "taken");
        assert!(l.rename("D", " e").is_err());
        assert!(l.rename("nope", "e").is_err());
        assert_eq!(names(&l), ["b", "c", "D"]);
        assert!(l.validate().is_ok());
    }

    #[test]
    fn stored_presets_keep_every_row_that_reads() {
        let stored = json!([
            { "name": "b", "width": 3, "future": true },
            { "name": "a", "width": "x" },
            { "name": "B", "width": 3 },
            { "name": "c", "width": 1e308 },
            "x",
            { "name": "a", "width": 2 },
        ]);
        let (l, problems) = Presets::<Widths>::from_stored(&stored, "presets.width");
        assert_eq!(names(&l), ["a", "b"]);
        assert_eq!(problems, ["presets.width.1", "presets.width.2", "presets.width.3", "presets.width.4"]);
        assert_eq!(serde_json::to_value(&l).unwrap(), json!([{ "name": "a", "width": 2.0 }, { "name": "b", "width": 3.0 }]));
        assert_eq!(Presets::<Widths>::from_stored(&json!({}), "p").1, ["p"]);
        assert_eq!(Presets::<Widths>::from_stored(&Value::Null, "p"), (Presets::default(), vec![]));
    }

    #[test]
    fn every_kind_reads_on_its_own() {
        let label = json!({ "name": "Cal", "side": "left", "size": 3, "depth": 1, "margin": 2 });
        let (l, problems) = PresetLibraries::from_stored(&json!({ "label": [label, 1], "future_kind": [] }));
        assert_eq!((l.label.presets().len(), problems), (1, vec!["presets.label.1".to_string()]));
        let back: PresetLibraries = serde_json::from_value(serde_json::to_value(&l).unwrap()).unwrap();
        assert_eq!(back, l);
        assert_eq!(PresetLibraries::from_stored(&json!([])), (PresetLibraries::default(), vec!["presets".to_string()]));
        assert_eq!(serde_json::from_value::<PresetLibraries>(json!("x")).unwrap(), PresetLibraries::default());
    }
}
