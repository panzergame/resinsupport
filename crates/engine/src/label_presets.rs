//! Label presets ([`crate::presets`]' first kind): a label's size, depth and margin, not its text
//! or side, which are each model's own. A label names the preset it follows ([`Label::preset`]) and keeps the values itself;
//! `model.label` changing a followed preset's values through one model changes the preset and
//! every model of the scene following it. What follows a label preset is in
//! `cmd::label_preset`.

use resin_geom::Label;
use serde::{Deserialize, Serialize};

use crate::presets::{Preset, PresetKind, Presets};

/// What a label preset sets: how its letters look, not what they say or where they go. Presets
/// stored with a `side` read, leaving it out.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct LabelSettings {
    pub size: f64,
    pub depth: f64,
    pub margin: f64,
}

impl LabelSettings {
    pub fn of(label: &Label) -> LabelSettings {
        LabelSettings { size: label.size, depth: label.depth, margin: label.margin }
    }

    /// `label` with these settings, following the preset called `name`.
    pub fn applied(&self, name: &str, label: &Label) -> Label {
        Label { size: self.size, depth: self.depth, margin: self.margin, preset: Some(name.into()), ..label.clone() }
    }
}

/// The label kind of preset.
pub struct LabelPresetKind;

impl PresetKind for LabelPresetKind {
    const KIND: &'static str = "label";
    type Settings = LabelSettings;

    fn check(s: &LabelSettings) -> std::result::Result<(), String> {
        s.applied("x", &Label::default()).validate()
    }
}

pub type LabelPreset = Preset<LabelSettings>;
pub type LabelPresets = Presets<LabelPresetKind>;

impl LabelPreset {
    /// The label following this preset: its settings, its name.
    pub fn applied(&self, label: &Label) -> Label {
        self.settings.applied(&self.name, label)
    }
}

#[cfg(test)]
mod tests {
    use resin_geom::LabelSide;

    use super::*;

    #[test]
    fn a_label_follows_its_preset_and_keeps_its_text_and_side() {
        let label = Label { text: "A1".into(), side: LabelSide::Back, preset: Some("CAL".into()), ..Label::default() };
        let p = LabelPreset { name: "cal".into(), settings: LabelSettings { size: 7.0, depth: 0.5, margin: 1.0 } };
        assert_ne!(LabelSettings::of(&label), p.settings);
        let applied = p.applied(&label);
        assert_eq!(applied, Label { text: "A1".into(), side: LabelSide::Back, size: 7.0, preset: Some("cal".into()), ..Label::default() });
        assert_eq!(LabelSettings::of(&applied), p.settings);
        assert!(LabelPresetKind::check(&p.settings).is_ok());
        assert!(LabelPresetKind::check(&LabelSettings { size: 0.0, ..p.settings }).is_err());
    }
}
