//! What follows a label preset: a model's label (`label.preset.*`, run by [`super::preset`]).

use resin_geom::{Label, same_preset_name};
use serde_json::Value;

use super::model::one_target;
use crate::presets::{Follow, Presets};
use crate::{EngineError, LabelPreset, LabelPresetKind, LabelSettings, Result, Scene, Session};

/// Every label in `scene` following the preset called `old` following it as `new`; their ids.
fn renamed(scene: &mut Scene, old: &str, new: &str) -> Result<Vec<u64>> {
    let behind: Vec<(u64, Label)> = scene
        .models()
        .iter()
        .filter_map(|m| m.label.as_ref().map(|l| (m.id, l)))
        .filter(|(_, l)| l.preset.as_deref().is_some_and(|n| same_preset_name(n, old) && n != new))
        .map(|(id, l)| (id, Label { preset: Some(new.into()), ..l.clone() }))
        .collect();
    for (id, l) in &behind {
        scene.set_label(*id, Some(l.clone()))?;
    }
    Ok(behind.into_iter().map(|(id, _)| id).collect())
}

impl Follow for LabelPresetKind {
    const TARGET_PARAMS: &'static [&'static str] = &["id"];
    type Target = u64;

    fn library(s: &Session) -> &Presets<Self> {
        &s.presets.label
    }

    fn library_mut(s: &mut Session) -> &mut Presets<Self> {
        &mut s.presets.label
    }

    /// `id?`: the one selected model by default.
    fn target(s: &Session, p: &Value) -> Result<u64> {
        one_target(s, p)
    }

    fn settings(s: &Session, id: &u64) -> Result<LabelSettings> {
        s.scene
            .model(*id)
            .and_then(|m| m.label.as_ref())
            .map(LabelSettings::of)
            .ok_or_else(|| EngineError::BadParams(format!("model {id} has no label")))
    }

    fn link(s: &mut Session, id: &u64, preset: &LabelPreset) -> Result<()> {
        s.every_scene(|scene| {
            let Some(label) = scene.model(*id).and_then(|m| m.label.clone()) else { return Ok(()) };
            let linked = preset.applied(&label);
            // A scene where the label had other settings keeps its link: it is not this preset.
            if LabelSettings::of(&label) != preset.settings || linked == label {
                return Ok(());
            }
            scene.set_label(*id, Some(linked))
        })
    }

    fn rename(s: &mut Session, old: &str, new: &str) -> Result<Vec<u64>> {
        let mut changed = Vec::new();
        let mut current = true;
        s.every_scene(|scene| {
            let ids = renamed(scene, old, new)?;
            if std::mem::take(&mut current) {
                changed = ids;
            }
            Ok(())
        })?;
        Ok(changed)
    }

    fn relink(s: &mut Session, old: &str, preset: &LabelPreset) -> Result<Vec<u64>> {
        let behind: Vec<(u64, Label)> = s
            .scene
            .models()
            .iter()
            .filter_map(|m| m.label.as_ref().map(|l| (m.id, l)))
            .filter(|(_, l)| l.preset.as_deref().is_some_and(|n| same_preset_name(n, old)))
            .map(|(id, l)| (id, preset.applied(l)))
            .filter(|(id, l)| s.scene.model(*id).and_then(|m| m.label.as_ref()) != Some(l))
            .collect();
        for (id, l) in &behind {
            s.scene.set_label(*id, Some(l.clone()))?;
        }
        Ok(behind.into_iter().map(|(id, _)| id).collect())
    }
}
