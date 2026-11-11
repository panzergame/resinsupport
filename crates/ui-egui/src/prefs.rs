//! Preferences, and the mouse mappings the navigation reads.
// The preset table and `lenient` are ported from SolidCraft (MIT OR Apache-2.0),
// crates/ui-egui/src/prefs.rs at 4bdffef0; see ATTRIBUTION.md. `lenient` is reduced: no
// migrations.

use egui::Modifiers;
use serde::{Deserialize, Serialize};

use crate::theme::Tokens;
use crate::{i18n, ui_kit};

/// The key the app keeps the preferences under in eframe's storage.
pub const STORAGE_KEY: &str = "resinsupport.prefs";

pub const OPENSCAD_NAV: &str = "openscad";
pub const TOUCHPAD_NAV: &str = "touchpad";

/// The Preferences pages: title and icon. Every setting below names the page whose Restore
/// defaults resets it.
pub const PAGES: &[(&str, &str)] = &[("prefs.page.general", "settings"), ("prefs.page.navigation", "home")];
pub const GENERAL: usize = 0;
pub const NAVIGATION: usize = 1;

// Reduced from SolidCraft's `preference_settings!` (crates/ui-egui/src/prefs.rs at 4bdffef0): one
// row per setting gives its field, default, page and control, so the struct, its defaults, the
// pages' resets and their forms cannot drift apart.
macro_rules! preference_settings {
    ($( $(#[$doc:meta])* $field:ident: $ty:ty = $default:expr $(, $page:expr => $control:ident($($args:tt)*))? ;)*) => {
        #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
        #[serde(default)]
        pub struct Prefs { $($(#[$doc])* pub $field: $ty,)* }

        impl Default for Prefs {
            fn default() -> Self { Self { $($field: $default,)* } }
        }

        /// Every stored field and the page whose Restore defaults resets it (`None`: no page).
        pub const FIELD_PAGES: &[(&str, Option<usize>)] = &[$((stringify!($field), preference_settings!(@page $($page)?)),)*];

        /// Put page `page`'s settings back to their defaults.
        pub fn reset_page(p: &mut Prefs, page: usize) {
            let d = Prefs::default();
            $($(if page == $page { p.$field = d.$field.clone(); })?)*
        }

        /// Page `page`'s controls, in table order.
        pub fn page_rows(p: &mut Prefs, page: usize, ui: &mut egui::Ui, t: &Tokens) {
            $($(if page == $page { preference_settings!(@control p, ui, t, $field, $control($($args)*)); })?)*
        }
    };
    (@page) => { None };
    (@page $page:expr) => { Some($page) };
    (@control $p:ident, $ui:ident, $t:ident, $field:ident, check($label:literal, $help:literal)) => {
        ui_kit::check_row($ui, $t, i18n::tr($label), i18n::tr($help), &mut $p.$field)
    };
    (@control $p:ident, $ui:ident, $t:ident, $field:ident, slider($label:literal, $range:expr)) => {
        ui_kit::wide_row($ui, $t, i18n::tr($label), |ui| ui.add(egui::Slider::new(&mut $p.$field, $range)))
    };
    (@control $p:ident, $ui:ident, $t:ident, $field:ident, choice($label:literal, $options:expr)) => {
        ui_kit::choice_row($ui, $t, i18n::tr($label), stringify!($field), &mut $p.$field, $options)
    };
    (@control $p:ident, $ui:ident, $t:ident, $field:ident, custom($f:path)) => {
        $f($p, $ui, $t)
    };
}

preference_settings! {
    /// `system`, or a language id from [`i18n::CATALOGS`].
    language: String = i18n::SYSTEM.into(), GENERAL => custom(language_row);
    /// Mouse mapping: a [`NavPreset`] id.
    nav: String = "fusion".into(), NAVIGATION => custom(nav_row);
    orbit_around: OrbitAround = OrbitAround::default(), NAVIGATION => custom(orbit_row);
    /// The wheel zooms toward the point under the cursor (off: toward the view's centre).
    zoom_to_cursor: bool = true, NAVIGATION => check("prefs.zoom_to_cursor", "prefs.zoom_to_cursor.help");
    /// The wheel zooms out when turned forward.
    zoom_reverse: bool = false, NAVIGATION => check("prefs.zoom_reverse", "prefs.zoom_reverse.help");
    wheel_zoom_speed: f32 = 1.0, NAVIGATION => slider("prefs.wheel_zoom_speed", 0.25..=4.0);
    /// Two-finger trackpad scroll pans (off: it zooms like a wheel).
    trackpad_pan: bool = true, NAVIGATION => check("prefs.trackpad_pan", "prefs.trackpad_pan.help");
    /// What a middle double-click does: `home`, `fit` or `off`.
    middle_double_click: String = "home".into(), NAVIGATION => choice("prefs.middle_double_click", &DOUBLE_CLICK);
    /// Off: a right drag that the mapping would orbit leaves the camera alone.
    right_drag_orbits: bool = true, NAVIGATION => check("prefs.right_drag_orbits", "prefs.right_drag_orbits.help");
    /// The support settings card's presets: tool data, on no page, so no page's Restore defaults
    /// wipes it. Read field by field ([`resin_engine::SupportSettings::from_stored`]).
    support_settings: resin_engine::SupportSettings = resin_engine::SupportSettings::default();
    /// The print settings card's printer profile and print settings: tool data, on no page, like
    /// `support_settings`. Read field by field ([`resin_engine::PrintConfig::from_stored`]).
    print_config: resin_engine::PrintConfig = resin_engine::PrintConfig::default();
    /// Imported models are lifted (off: they stay where the file puts them, as before lifting).
    lift_on_import: bool = true, GENERAL => check("prefs.lift_on_import", "prefs.lift_on_import.help");
    /// Where lifted models' lowest points sit, mm: tool data of the object settings card, on no
    /// page, like `support_settings`.
    lift_height: f64 = resin_engine::DEFAULT_LIFT_HEIGHT;
}

const DOUBLE_CLICK: [(&str, &str); 3] = [("home", "prefs.double_click.home"), ("fit", "prefs.double_click.fit"), ("off", "prefs.double_click.off")];

fn language_row(p: &mut Prefs, ui: &mut egui::Ui, t: &Tokens) {
    let system = i18n::format("prefs.language.system", &[("language", &language_name(i18n::system_language()))]);
    let shown = if p.language == i18n::SYSTEM { system.clone() } else { language_name(&p.language).to_string() };
    ui_kit::wide_row(ui, t, i18n::tr("prefs.language"), |ui| {
        egui::ComboBox::from_id_salt("language").selected_text(shown).width(ui.available_width()).show_ui(ui, |ui| {
            ui.selectable_value(&mut p.language, i18n::SYSTEM.to_string(), system);
            for c in i18n::CATALOGS {
                ui.selectable_value(&mut p.language, c.id.to_string(), c.name);
            }
        });
    });
}

fn language_name(id: &str) -> &'static str {
    i18n::CATALOGS.iter().find(|c| c.id == id).map_or("English", |c| c.name)
}

fn nav_row(p: &mut Prefs, ui: &mut egui::Ui, t: &Tokens) {
    let current = nav_preset(&p.nav);
    ui_kit::wide_row(ui, t, i18n::tr("prefs.nav"), |ui| {
        let mut chosen = None;
        egui::ComboBox::from_id_salt("nav").selected_text(i18n::tr(current.label)).width(ui.available_width()).show_ui(ui, |ui| {
            for preset in NAV_PRESETS {
                if ui.selectable_label(preset.id == current.id, i18n::tr(preset.label)).on_hover_text(i18n::tr(preset.help)).clicked() {
                    chosen = Some(preset);
                }
            }
        });
        if let Some(preset) = chosen {
            choose_nav_preset(p, preset);
        }
    });
    ui_kit::hint(ui, t, i18n::tr(current.help));
}

fn orbit_row(p: &mut Prefs, ui: &mut egui::Ui, t: &Tokens) {
    use OrbitAround::*;
    let options = [(ViewCentre, "prefs.orbit.view_centre"), (Cursor, "prefs.orbit.cursor"), (Origin, "prefs.orbit.origin")];
    let shown = options.iter().find(|(o, _)| *o == p.orbit_around).map_or("", |(_, id)| i18n::tr(id));
    ui_kit::wide_row(ui, t, i18n::tr("prefs.orbit_around"), |ui| {
        egui::ComboBox::from_id_salt("orbit_around").selected_text(shown).width(ui.available_width()).show_ui(ui, |ui| {
            for (o, id) in options {
                ui.selectable_value(&mut p.orbit_around, o, i18n::tr(id));
            }
        });
    });
}

/// Saved preferences, keeping every one that still reads, and the names of those that did not
/// (`zoom_reverse`, `support_settings.light.tip.length`), which are back to their defaults. A value
/// this version doesn't understand (a setting a newer or older release saved differently, a hand
/// edit) falls back to its default on its own instead of discarding all of them. Unknown keys are
/// ignored.
pub fn lenient(saved: &serde_json::Value) -> (Prefs, Vec<String>) {
    let mut problems: Vec<String> = saved
        .get("support_settings")
        .map(|v| resin_engine::SupportSettings::from_stored(v).1.into_iter().map(|p| format!("support_settings.{p}")).collect())
        .unwrap_or_default();
    if let Some(v) = saved.get("print_config") {
        problems.extend(resin_engine::PrintConfig::from_stored(v).1.into_iter().map(|p| format!("print_config.{p}")));
    }
    if let Ok(p) = serde_json::from_value::<Prefs>(saved.clone()) {
        return (p, problems);
    }
    let Ok(mut cur) = serde_json::to_value(Prefs::default()) else { return (Prefs::default(), problems) };
    if let (Some(o), Some(src)) = (cur.as_object_mut(), saved.as_object()) {
        for (k, v) in src.iter().take(1000) {
            // Keys this version doesn't have are ignored anyway.
            let Some(old) = o.get(k).cloned() else { continue };
            o.insert(k.clone(), v.clone());
            if serde_json::from_value::<Prefs>(serde_json::Value::Object(o.clone())).is_err() {
                log::warn!("preferences: `{k}` is not understood by this version; using its default");
                problems.push(k.clone());
                o.insert(k.clone(), old);
            }
        }
    } else {
        problems.push(STORAGE_KEY.into());
    }
    (serde_json::from_value(cur).unwrap_or_default(), problems)
}

/// The stored preferences, read leniently, and what fell back to defaults. Stored as JSON; RON from
/// before then (eframe's `set_value`) is still read.
pub fn load(storage: &dyn eframe::Storage) -> (Prefs, Vec<String>) {
    let Some(text) = storage.get_string(STORAGE_KEY) else { return (Prefs::default(), Vec::new()) };
    match serde_json::from_str::<serde_json::Value>(&text) {
        Ok(v) => lenient(&v),
        Err(_) => match eframe::get_value::<Prefs>(storage, STORAGE_KEY) {
            Some(p) => (p, Vec::new()),
            None => {
                log::warn!("preferences: the stored settings could not be read; using the defaults");
                (Prefs::default(), vec![STORAGE_KEY.into()])
            }
        },
    }
}

/// The preferences as stored.
pub fn to_json(p: &Prefs) -> Option<String> {
    serde_json::to_string(p).map_err(|e| log::warn!("preferences: {e}")).ok()
}

impl Prefs {
    /// Touchpad scroll zooms without overwriting the user's preference for other presets.
    pub fn trackpad_pans(&self) -> bool {
        self.nav != TOUCHPAD_NAV && self.trackpad_pan
    }
    pub fn zoom_sign(&self) -> f64 {
        if self.zoom_reverse { -1.0 } else { 1.0 }
    }
    /// What dragging `button` with `mods` does under the chosen mapping.
    pub fn nav_mode(&self, button: NavButton, mods: Modifiers) -> Option<NavMode> {
        let mode = nav_preset(&self.nav).mode(button, mods);
        if button == NavButton::Right && mode == Some(NavMode::Orbit) && !self.right_drag_orbits { None } else { mode }
    }
}

/// What an orbit turns around.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrbitAround {
    /// The world origin: the view turns about it and keeps it still on screen.
    Origin,
    /// The view's centre: what Pan moves and Fit frames.
    #[default]
    ViewCentre,
    /// The model point under the cursor when the drag began.
    Cursor,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NavMode {
    Orbit,
    Pan,
    Zoom,
}

/// A mouse button that can drag the camera.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NavButton {
    Left,
    Middle,
    Right,
}

/// Modifier keys a binding needs held (others may be held too).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct NavKeys {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
}

impl NavKeys {
    const NONE: NavKeys = NavKeys { shift: false, ctrl: false, alt: false };
    const SHIFT: NavKeys = NavKeys { shift: true, ctrl: false, alt: false };
    const CTRL: NavKeys = NavKeys { shift: false, ctrl: true, alt: false };
    const ALT: NavKeys = NavKeys { shift: false, ctrl: false, alt: true };
    const SHIFT_ALT: NavKeys = NavKeys { shift: true, ctrl: false, alt: true };

    /// All of these keys are held (Ctrl is Cmd on macOS).
    fn held(self, m: Modifiers) -> bool {
        (!self.shift || m.shift) && (!self.ctrl || m.ctrl || m.command) && (!self.alt || m.alt)
    }
}

/// One camera binding: dragging `button` with `keys` held does `mode`.
#[derive(Clone, Copy, Debug)]
pub struct NavBinding {
    pub button: NavButton,
    pub keys: NavKeys,
    pub mode: NavMode,
}

const fn bind(button: NavButton, keys: NavKeys, mode: NavMode) -> NavBinding {
    NavBinding { button, keys, mode }
}

/// A mouse mapping: its bindings, most specific first (the first one whose button is dragged
/// with all its keys held wins).
pub struct NavPreset {
    pub id: &'static str,
    /// Catalog ids: the name, and what the mouse does.
    pub label: &'static str,
    pub help: &'static str,
    /// Choosing the preset turns the wheel the other way.
    pub reverse_zoom: bool,
    /// Dragging down zooms in instead of out (independent of the wheel preference).
    pub reverse_drag_zoom: bool,
    /// Choosing the preset sets what a middle double-click does (`None` leaves it as it is).
    pub middle_double_click: Option<&'static str>,
    /// Choosing the preset sets "Right drag orbits" (`None` leaves it as it is).
    pub right_drag_orbits: Option<bool>,
    pub bindings: &'static [NavBinding],
}

impl NavPreset {
    /// What dragging `button` with `mods` held does, if anything.
    pub fn mode(&self, button: NavButton, mods: Modifiers) -> Option<NavMode> {
        self.bindings.iter().find(|b| b.button == button && b.keys.held(mods)).map(|b| b.mode)
    }

    /// Button-free motion has exact modifiers, unlike the additive drag bindings.
    pub fn motion_mode(&self, mods: Modifiers) -> Option<NavMode> {
        if self.id != TOUCHPAD_NAV || mods.ctrl || mods.command || mods.mac_cmd {
            return None;
        }
        match (mods.shift, mods.alt) {
            (true, false) => Some(Pan),
            (false, true) => Some(Orbit),
            _ => None,
        }
    }
}

use NavButton::{Left, Middle, Right};
use NavMode::{Orbit, Pan, Zoom};

/// The mouse mappings. The first is the default.
pub const NAV_PRESETS: &[NavPreset] = &[
    NavPreset {
        id: "fusion",
        label: "nav.fusion",
        help: "nav.fusion.help",
        reverse_zoom: false,
        reverse_drag_zoom: false,
        middle_double_click: None,
        right_drag_orbits: None,
        bindings: &[bind(Middle, NavKeys::SHIFT, Orbit), bind(Middle, NavKeys::NONE, Pan), bind(Right, NavKeys::NONE, Orbit)],
    },
    NavPreset {
        id: "solidworks",
        label: "nav.solidworks",
        help: "nav.solidworks.help",
        reverse_zoom: true,
        reverse_drag_zoom: false,
        middle_double_click: Some("fit"),
        right_drag_orbits: Some(false),
        bindings: &[
            bind(Middle, NavKeys::CTRL, Pan),
            bind(Middle, NavKeys::SHIFT, Zoom),
            bind(Middle, NavKeys::NONE, Orbit),
            bind(Right, NavKeys::NONE, Orbit),
        ],
    },
    NavPreset {
        id: "creo",
        label: "nav.creo",
        help: "nav.creo.help",
        reverse_zoom: false,
        reverse_drag_zoom: true,
        middle_double_click: None,
        right_drag_orbits: None,
        bindings: &[bind(Middle, NavKeys::CTRL, Zoom), bind(Middle, NavKeys::SHIFT, Pan), bind(Middle, NavKeys::NONE, Orbit)],
    },
    NavPreset {
        id: "nx",
        label: "nav.nx",
        help: "nav.nx.help",
        reverse_zoom: false,
        reverse_drag_zoom: false,
        middle_double_click: Some("fit"),
        right_drag_orbits: Some(false),
        bindings: &[bind(Middle, NavKeys::SHIFT, Pan), bind(Middle, NavKeys::CTRL, Zoom), bind(Middle, NavKeys::NONE, Orbit)],
    },
    NavPreset {
        id: "inventor",
        label: "nav.inventor",
        help: "nav.inventor.help",
        reverse_zoom: false,
        reverse_drag_zoom: false,
        middle_double_click: None,
        right_drag_orbits: None,
        bindings: &[
            bind(Middle, NavKeys::SHIFT, Orbit),
            bind(Middle, NavKeys::CTRL, Zoom),
            bind(Middle, NavKeys::NONE, Pan),
            bind(Right, NavKeys::NONE, Orbit),
        ],
    },
    NavPreset {
        id: "alias",
        label: "nav.alias",
        help: "nav.alias.help",
        reverse_zoom: false,
        reverse_drag_zoom: false,
        middle_double_click: None,
        right_drag_orbits: None,
        bindings: &[
            bind(Left, NavKeys::SHIFT_ALT, Orbit),
            bind(Middle, NavKeys::SHIFT_ALT, Pan),
            bind(Right, NavKeys::SHIFT_ALT, Zoom),
            bind(Middle, NavKeys::CTRL, Zoom),
            bind(Middle, NavKeys::NONE, Pan),
            bind(Right, NavKeys::NONE, Orbit),
        ],
    },
    NavPreset {
        id: "maya",
        label: "nav.maya",
        help: "nav.maya.help",
        reverse_zoom: false,
        reverse_drag_zoom: false,
        middle_double_click: None,
        right_drag_orbits: None,
        bindings: &[
            bind(Left, NavKeys::ALT, Orbit),
            bind(Middle, NavKeys::ALT, Pan),
            bind(Right, NavKeys::ALT, Zoom),
            bind(Middle, NavKeys::NONE, Pan),
        ],
    },
    NavPreset {
        id: "blender",
        label: "nav.blender",
        help: "nav.blender.help",
        reverse_zoom: false,
        reverse_drag_zoom: false,
        middle_double_click: None,
        right_drag_orbits: None,
        bindings: &[bind(Middle, NavKeys::SHIFT, Pan), bind(Middle, NavKeys::CTRL, Zoom), bind(Middle, NavKeys::NONE, Orbit)],
    },
    NavPreset {
        id: "tinkercad",
        label: "nav.tinkercad",
        help: "nav.tinkercad.help",
        reverse_zoom: false,
        reverse_drag_zoom: false,
        middle_double_click: None,
        right_drag_orbits: None,
        bindings: &[bind(Right, NavKeys::SHIFT, Pan), bind(Right, NavKeys::NONE, Orbit), bind(Middle, NavKeys::NONE, Pan)],
    },
    NavPreset {
        id: OPENSCAD_NAV,
        label: "nav.openscad",
        help: "nav.openscad.help",
        reverse_zoom: false,
        reverse_drag_zoom: false,
        middle_double_click: None,
        right_drag_orbits: None,
        bindings: &[
            bind(Left, NavKeys::NONE, Orbit),
            bind(Right, NavKeys::SHIFT, Zoom),
            bind(Middle, NavKeys::SHIFT, Zoom),
            bind(Right, NavKeys::NONE, Pan),
            bind(Middle, NavKeys::NONE, Pan),
        ],
    },
    NavPreset {
        id: "hfss",
        label: "nav.hfss",
        help: "nav.hfss.help",
        reverse_zoom: false,
        reverse_drag_zoom: false,
        middle_double_click: None,
        right_drag_orbits: None,
        bindings: &[bind(Left, NavKeys::SHIFT_ALT, Zoom), bind(Left, NavKeys::ALT, Pan)],
    },
    NavPreset {
        id: TOUCHPAD_NAV,
        label: "nav.touchpad",
        help: "nav.touchpad.help",
        reverse_zoom: false,
        reverse_drag_zoom: false,
        middle_double_click: None,
        right_drag_orbits: None,
        bindings: &[bind(Middle, NavKeys::NONE, Pan), bind(Right, NavKeys::NONE, Orbit)],
    },
];

/// The preset with this id (the default for an unknown one).
pub fn nav_preset(id: &str) -> &'static NavPreset {
    NAV_PRESETS.iter().find(|p| p.id == id).unwrap_or(&NAV_PRESETS[0])
}

/// Picks `preset`: its bindings, wheel direction and, when the preset names them, middle
/// double-click and "Right drag orbits".
pub fn choose_nav_preset(prefs: &mut Prefs, preset: &NavPreset) {
    prefs.nav = preset.id.into();
    prefs.zoom_reverse = preset.reverse_zoom;
    if let Some(double_click) = preset.middle_double_click {
        prefs.middle_double_click = double_click.into();
    }
    if let Some(orbits) = preset.right_drag_orbits {
        prefs.right_drag_orbits = orbits;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_fusion() {
        let p = Prefs::default();
        assert_eq!(nav_preset(&p.nav).id, "fusion");
        assert_eq!(p.nav_mode(NavButton::Middle, Modifiers::NONE), Some(NavMode::Pan));
        assert_eq!(p.nav_mode(NavButton::Middle, Modifiers::SHIFT), Some(NavMode::Orbit));
        assert_eq!(p.nav_mode(NavButton::Right, Modifiers::NONE), Some(NavMode::Orbit));
        assert_eq!(nav_preset("unknown").id, "fusion");
    }

    #[test]
    fn choosing_a_preset_applies_its_extras() {
        let mut p = Prefs::default();
        choose_nav_preset(&mut p, nav_preset("solidworks"));
        assert!(p.zoom_reverse && !p.right_drag_orbits);
        assert_eq!(p.middle_double_click, "fit");
        assert_eq!(p.nav_mode(NavButton::Right, Modifiers::NONE), None);
        assert_eq!(p.nav_mode(NavButton::Middle, Modifiers::NONE), Some(NavMode::Orbit));
    }

    #[test]
    fn every_setting_has_a_page_and_restore_defaults_restores_it() {
        // Tool data has no page, so no page's Restore defaults wipes it.
        const TOOL_DATA: &[&str] = &["support_settings", "print_config", "lift_height"];
        for (field, page) in FIELD_PAGES {
            assert!(page.is_some_and(|p| p < PAGES.len()) || (page.is_none() && TOOL_DATA.contains(field)), "{field}");
        }
        let settings = edited_settings();
        let mut p = Prefs {
            language: "en".into(),
            nav: "blender".into(),
            zoom_reverse: true,
            wheel_zoom_speed: 3.0,
            support_settings: settings,
            ..Prefs::default()
        };
        reset_page(&mut p, GENERAL);
        assert_eq!(p.language, i18n::SYSTEM);
        assert_eq!(p.nav, "blender");
        reset_page(&mut p, NAVIGATION);
        assert_eq!(p, Prefs { support_settings: settings, ..Prefs::default() });
    }

    fn edited_settings() -> resin_engine::SupportSettings {
        let mut s = resin_engine::Session::new();
        s.execute(
            "support.configure",
            &serde_json::json!({ "active": "heavy", "preset": "light", "tip": { "length": 2.75 }, "raft": { "enabled": false } }),
        )
        .unwrap();
        *s.support_settings()
    }

    #[test]
    fn saved_preferences_read_back_equal() {
        let p = Prefs { nav: "maya".into(), support_settings: edited_settings(), ..Prefs::default() };
        let saved: serde_json::Value = serde_json::from_str(&to_json(&p).unwrap()).unwrap();
        assert_eq!(lenient(&saved), (p, vec![]));
    }

    #[test]
    fn print_settings_round_trip_and_a_damaged_field_falls_back_alone() {
        let mut s = resin_engine::Session::new();
        s.execute(
            "print.configure",
            &serde_json::json!({ "printer": { "name": "Mine", "mirror_y": true }, "layers": { "height": 0.025 }, "exposure": { "normal": 2.4 } }),
        )
        .unwrap();
        let p = Prefs { print_config: s.print_config().clone(), ..Prefs::default() };
        let mut saved: serde_json::Value = serde_json::from_str(&to_json(&p).unwrap()).unwrap();
        assert_eq!(lenient(&saved), (p.clone(), vec![]));

        saved["print_config"]["exposure"]["normal"] = serde_json::json!("long");
        saved["print_config"]["layers"]["bottom_count"] = serde_json::json!(-3);
        let (back, mut problems) = lenient(&saved);
        problems.sort();
        assert_eq!(problems, ["print_config.exposure.normal", "print_config.layers.bottom_count"]);
        let (c, defaults) = (&back.print_config, resin_engine::slice::PrintSettings::default());
        assert_eq!((c.settings.exposure.normal, c.settings.layers.bottom_count), (defaults.exposure.normal, defaults.layers.bottom_count));
        assert_eq!((c.printer.clone(), c.settings.layers.height), (p.print_config.printer.clone(), 0.025));
        assert_eq!(Prefs { print_config: p.print_config.clone(), ..back }, p, "nothing else moved");
    }

    #[test]
    fn unreadable_settings_fall_back_alone_and_are_named() {
        let saved = serde_json::json!({
            "nav": "maya",
            "zoom_reverse": "sometimes",
            "from_a_newer_version": [1, 2],
            "support_settings": {
                "active": "light",
                "light": { "tip": { "length": 2.5, "contact_diameter": 99 }, "raft": { "enabled": "maybe" } },
                "heavy": { "body": { "diameter": 0.3 } },
                "medium": { "base": { "height": 0.9 } },
            },
        });
        let (p, mut problems) = lenient(&saved);
        problems.sort();
        assert_eq!(
            problems,
            ["support_settings.heavy", "support_settings.light.raft.enabled", "support_settings.light.tip.contact_diameter", "zoom_reverse"]
        );
        assert_eq!((p.nav.as_str(), p.zoom_reverse), ("maya", false));
        let s = p.support_settings;
        let light = resin_engine::PresetId::Light.defaults();
        assert_eq!(
            (s.active, s.light.tip.length, s.light.tip.contact_diameter, s.light.raft.enabled),
            (resin_engine::PresetId::Light, 2.5, light.tip.contact_diameter, true)
        );
        assert_eq!(s.heavy, resin_engine::PresetId::Heavy.defaults());
        assert_eq!(s.medium.base.height, 0.9);
        assert!(s.validate().is_ok());

        // Not settings at all: every default, and said so.
        assert_eq!(lenient(&serde_json::json!([1])), (Prefs::default(), vec![STORAGE_KEY.to_string()]));
        assert_eq!(lenient(&serde_json::json!({ "support_settings": 5 })).1, ["support_settings.support_settings"]);
    }

    #[test]
    fn settings_stored_as_ron_before_still_load() {
        #[derive(Default)]
        struct Mem(std::collections::HashMap<String, String>);
        impl eframe::Storage for Mem {
            fn get_string(&self, key: &str) -> Option<String> {
                self.0.get(key).cloned()
            }
            fn set_string(&mut self, key: &str, value: String) {
                self.0.insert(key.into(), value);
            }
            fn remove_string(&mut self, key: &str) {
                self.0.remove(key);
            }
            fn flush(&mut self) {}
        }
        let mut storage = Mem::default();
        assert_eq!(load(&storage), (Prefs::default(), vec![]));
        // What an earlier version wrote: RON, through eframe's `set_value`.
        #[derive(Serialize)]
        struct Earlier {
            nav: String,
            zoom_reverse: bool,
        }
        eframe::set_value(&mut storage, STORAGE_KEY, &Earlier { nav: "creo".into(), zoom_reverse: true });
        let (p, problems) = load(&storage);
        assert_eq!((p.nav.as_str(), p.zoom_reverse, problems.len()), ("creo", true, 0));
        storage.0.insert(STORAGE_KEY.into(), "not settings".into());
        assert_eq!(load(&storage), (Prefs::default(), vec![STORAGE_KEY.to_string()]));
    }

    #[test]
    fn old_settings_files_keep_their_values_and_gain_the_new_ones() {
        let p: Prefs = serde_json::from_str(r#"{"nav":"maya","zoom_reverse":true}"#).unwrap();
        assert_eq!((p.nav.as_str(), p.zoom_reverse, p.language.as_str()), ("maya", true, i18n::SYSTEM));
    }

    #[test]
    fn preset_names_and_help_are_in_the_catalog() {
        for preset in NAV_PRESETS {
            assert!(i18n::has(preset.label) && i18n::has(preset.help), "{}", preset.id);
        }
    }

    #[test]
    fn preset_ids_are_unique() {
        let mut ids: Vec<&str> = NAV_PRESETS.iter().map(|p| p.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), NAV_PRESETS.len());
    }
}
