// The print settings' defaults are mslicer's (GPL-3.0), `common/src/slice/config.rs` at 355aca2
// (`SliceConfig::default`, `ExposureConfig::default`); see ATTRIBUTION.md. The two-phase lift and
// retract follow UVtools' model (`UVtools.Core/FileFormats/FileFormat.cs`, `RetractHeight2`, at
// 5fdcc53), written from its field definitions with no UVtools code.

//! The printer profile and the print settings, their defaults and their ranges. The ranges and
//! the cross-field rules are in one place, [`FIELDS`] and [`validate`]: the `print.*` commands
//! and the stored preferences go through them alike.

use serde::{Deserialize, Serialize};

/// What the machine is. Defaults: the Elegoo Saturn 2 (8K), from Elegoo's published
/// specifications.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PrinterProfile {
    /// Written to the CTB machine name.
    pub name: String,
    /// Pixels.
    pub resolution_x: u32,
    pub resolution_y: u32,
    /// The screen's size, mm; the pixel pitch is derived from it, never stored.
    pub display_x: f64,
    pub display_y: f64,
    /// mm.
    pub build_height: f64,
    /// Applied when rasterizing.
    pub mirror_x: bool,
    pub mirror_y: bool,
    /// The kind of CTB file the printer reads. Not a slicing input: changing it never makes a
    /// job stale.
    #[serde(default)]
    pub ctb_format: CtbFormat,
}

/// Which CTB file [`PrinterProfile`]'s printer reads.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CtbFormat {
    /// Chitubox's encrypted CTB, version 5: what Elegoo's Saturn 2 and later printers read.
    #[default]
    Encrypted,
    /// Plain CTB, version 2, for older printers.
    V2,
}

impl CtbFormat {
    pub const ALL: [CtbFormat; 2] = [CtbFormat::Encrypted, CtbFormat::V2];

    /// Its name in `print.configure` and the stored settings.
    pub fn id(self) -> &'static str {
        match self {
            CtbFormat::Encrypted => "encrypted",
            CtbFormat::V2 => "v2",
        }
    }

    pub fn from_id(id: &str) -> Option<CtbFormat> {
        CtbFormat::ALL.into_iter().find(|f| f.id() == id)
    }
}

impl Default for PrinterProfile {
    fn default() -> Self {
        PrinterProfile {
            name: "ELEGOO SATURN 2".into(),
            resolution_x: 7680,
            resolution_y: 4320,
            display_x: 218.88,
            display_y: 123.12,
            build_height: 250.0,
            mirror_x: false,
            mirror_y: false,
            ctb_format: CtbFormat::Encrypted,
        }
    }
}

impl PrinterProfile {
    /// One pixel's width and depth, mm.
    pub fn pixel_pitch(&self) -> (f64, f64) {
        (self.display_x / f64::from(self.resolution_x), self.display_y / f64::from(self.resolution_y))
    }

    /// From plate millimetres to pixel coordinates with `aa` sub-samples per pixel side, as
    /// `(scale, offset)` per axis, `px = mm · scale + offset`: the plate's centre is the image's,
    /// row 0 its +Y edge, then the mirrors.
    pub fn image_map(&self, aa: u32) -> ([f64; 2], [f64; 2]) {
        let size = [f64::from(self.resolution_x) * f64::from(aa), f64::from(self.resolution_y) * f64::from(aa)];
        let (kx, ky) = (size[0] / self.display_x, size[1] / self.display_y);
        let (mut sx, mut sy) = (kx, -ky);
        let (mut ox, mut oy) = (self.display_x / 2.0 * kx, self.display_y / 2.0 * ky);
        if self.mirror_x {
            (sx, ox) = (-sx, size[0] - ox);
        }
        if self.mirror_y {
            (sy, oy) = (-sy, size[1] - oy);
        }
        ([sx, sy], [ox, oy])
    }

    fn number(&self, key: &str) -> Option<f64> {
        Some(match key {
            "resolution_x" => self.resolution_x.into(),
            "resolution_y" => self.resolution_y.into(),
            "display_x" => self.display_x,
            "display_y" => self.display_y,
            "build_height" => self.build_height,
            _ => return None,
        })
    }

    /// Set number `key` to `x`, which [`Field::accepts`] already took. Whether there is one.
    fn set_number(&mut self, key: &str, x: f64) -> bool {
        match key {
            "resolution_x" => self.resolution_x = x as u32,
            "resolution_y" => self.resolution_y = x as u32,
            "display_x" => self.display_x = x,
            "display_y" => self.display_y = x,
            "build_height" => self.build_height = x,
            _ => return false,
        }
        true
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Layers {
    /// mm.
    pub height: f64,
    pub bottom_count: u32,
    /// Layers after the bottom ones whose exposure goes linearly from bottom to normal.
    pub transition_count: u32,
}

/// Seconds, and the light's PWM power (1 to 255).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Exposure {
    pub normal: f64,
    pub bottom: f64,
    pub light_off: f64,
    pub bottom_light_off: f64,
    pub power: u8,
    pub bottom_power: u8,
}

/// How the plate moves after a layer, for the regular layers and the bottom ones: [`Travel`]'s
/// values, flat so each is one setting.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Motion {
    pub lift_distance_fast: f64,
    pub lift_distance_slow: f64,
    pub lift_speed_fast: f64,
    pub lift_speed_slow: f64,
    pub retract_distance_slow: f64,
    pub retract_speed_fast: f64,
    pub retract_speed_slow: f64,
    pub bottom_lift_distance_fast: f64,
    pub bottom_lift_distance_slow: f64,
    pub bottom_lift_speed_fast: f64,
    pub bottom_lift_speed_slow: f64,
    pub bottom_retract_distance_slow: f64,
    pub bottom_retract_speed_fast: f64,
    pub bottom_retract_speed_slow: f64,
}

impl Motion {
    /// The regular layers' travel.
    pub fn regular(&self) -> Travel {
        Travel {
            lift_distance_fast: self.lift_distance_fast,
            lift_distance_slow: self.lift_distance_slow,
            lift_speed_fast: self.lift_speed_fast,
            lift_speed_slow: self.lift_speed_slow,
            retract_distance_slow: self.retract_distance_slow,
            retract_speed_fast: self.retract_speed_fast,
            retract_speed_slow: self.retract_speed_slow,
        }
    }

    /// The bottom layers' travel.
    pub fn bottom(&self) -> Travel {
        Travel {
            lift_distance_fast: self.bottom_lift_distance_fast,
            lift_distance_slow: self.bottom_lift_distance_slow,
            lift_speed_fast: self.bottom_lift_speed_fast,
            lift_speed_slow: self.bottom_lift_speed_slow,
            retract_distance_slow: self.bottom_retract_distance_slow,
            retract_speed_fast: self.bottom_retract_speed_fast,
            retract_speed_slow: self.bottom_retract_speed_slow,
        }
    }

    /// Shorten the regular (`bottom` false) or bottom slow retract to the lift it comes back
    /// from, as a shorter lift leaves it no more room.
    pub fn fit_retract(&mut self, bottom: bool) {
        if bottom {
            self.bottom_retract_distance_slow = self.bottom_retract_distance_slow.min(self.bottom().lift_distance());
        } else {
            self.retract_distance_slow = self.retract_distance_slow.min(self.regular().lift_distance());
        }
    }
}

/// One layer's plate travel, in two phases each way: distances in mm, speeds in mm/min. The lift
/// goes slow (peeling the layer off the film) then fast; the retract comes back the whole lift,
/// fast then slow over its last `retract_distance_slow`, so its fast distance is what the slow one
/// leaves ([`Travel::retract_distance_fast`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Travel {
    pub lift_distance_fast: f64,
    pub lift_distance_slow: f64,
    pub lift_speed_fast: f64,
    pub lift_speed_slow: f64,
    pub retract_distance_slow: f64,
    pub retract_speed_fast: f64,
    pub retract_speed_slow: f64,
}

impl Travel {
    /// How far the plate rises, both phases.
    pub fn lift_distance(&self) -> f64 {
        self.lift_distance_fast + self.lift_distance_slow
    }

    /// What the slow retract leaves of the lift.
    pub fn retract_distance_fast(&self) -> f64 {
        (self.lift_distance() - self.retract_distance_slow).max(0.0)
    }

    /// Seconds up and back down, the speeds being per minute.
    pub fn seconds(&self) -> f64 {
        let leg = |distance: f64, speed: f64| if speed > 0.0 { distance / speed * 60.0 } else { 0.0 };
        leg(self.lift_distance_slow, self.lift_speed_slow)
            + leg(self.lift_distance_fast, self.lift_speed_fast)
            + leg(self.retract_distance_fast(), self.retract_speed_fast)
            + leg(self.retract_distance_slow, self.retract_speed_slow)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Slicing {
    /// Sub-samples per pixel side, one of [`ANTI_ALIASING`]; 1 is off. Unused when `exact`.
    pub anti_aliasing: u8,
    /// Planes cut through each layer's height, evenly spaced, their greys averaged: one of
    /// [`ANTI_ALIASING`]; 1 cuts the layer's middle only.
    #[serde(default = "one")]
    pub anti_aliasing_z: u8,
    /// Each pixel's grey is the share of it the layer covers, worked out from the edges, in place
    /// of `anti_aliasing`'s sub-samples.
    #[serde(default)]
    pub exact: bool,
    /// Greys become `255 · (grey / 255)^gamma`; 0 lights the pixels at least half covered and
    /// darkens the rest. 1 leaves them as rasterized.
    #[serde(default = "no_gamma")]
    pub gamma: f64,
    /// A box blur after the anti-aliasing and before the gamma: pixels taken in on each side.
    #[serde(default = "one_px")]
    pub blur_radius: u32,
    /// Times the blur runs, 0 for none; three come close to a Gaussian.
    #[serde(default)]
    pub blur_passes: u8,
    /// After the blur and before the gamma, greys printing under `dither_floor` are lit at it on
    /// their share of the layers and left dark on the others.
    #[serde(default)]
    pub dither_z: bool,
    /// Each pixel dithers from its own starting point, so neighbours along an edge switch on at
    /// different layers.
    #[serde(default)]
    pub dither_xy: bool,
    /// The lowest grey sent to the printer that the resin cures reliably, in percent of full
    /// light: with `dither_z` or `dither_xy`, the greys printing under it are dithered between it
    /// and dark. 100 dithers between lit and dark only.
    #[serde(default = "half")]
    pub dither_floor: u8,
    /// With `dither_z` or `dither_xy`, how far from vertical a surface may lean, in degrees, and
    /// still be dithered: flatter ones keep their greys. Read from how much a pixel's grey changes
    /// across the layer's Z anti-aliasing cuts, so ignored with one cut. 90 dithers everywhere.
    #[serde(default = "twenty_degrees")]
    pub dither_max_angle: f64,
}

impl Slicing {
    /// `dither_floor` as a grey, 1 to 255.
    pub fn dither_floor_grey(&self) -> u8 {
        let grey = (u32::from(self.dither_floor.min(100)) * 255 + 50) / 100;
        u8::try_from(grey).unwrap_or(u8::MAX).max(1)
    }

    /// What each grey becomes through `gamma`, as the slicer applies it.
    pub fn gamma_table(&self) -> [u8; 256] {
        crate::raster::gamma_table(self.gamma)
    }
}

fn one_px() -> u32 {
    1
}

fn no_gamma() -> f64 {
    1.0
}

fn one() -> u8 {
    1
}

fn half() -> u8 {
    50
}

fn twenty_degrees() -> f64 {
    20.0
}

/// For the resin and cost estimates.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Estimates {
    /// g/ml.
    pub resin_density: f64,
    /// ml.
    pub bottle_volume: f64,
    /// In the user's currency.
    pub bottle_cost: f64,
}

/// How a resin is exposed and how the plate moves. The defaults are a starting point, to be tuned
/// for each resin.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PrintSettings {
    pub layers: Layers,
    pub exposure: Exposure,
    pub motion: Motion,
    pub slicing: Slicing,
    pub estimates: Estimates,
}

impl Default for PrintSettings {
    fn default() -> Self {
        PrintSettings {
            // mslicer's 0.5 mm of bottom layers and 0.2 mm of transition, at 0.05 mm.
            layers: Layers { height: 0.05, bottom_count: 10, transition_count: 4 },
            exposure: Exposure { normal: 3.0, bottom: 30.0, light_off: 1.0, bottom_light_off: 1.0, power: 255, bottom_power: 255 },
            // mslicer's single-phase 5 mm at 330 mm/min both ways: the second phases are off.
            motion: Motion {
                lift_distance_fast: 0.0,
                lift_distance_slow: 5.0,
                lift_speed_fast: 330.0,
                lift_speed_slow: 330.0,
                retract_distance_slow: 0.0,
                retract_speed_fast: 330.0,
                retract_speed_slow: 330.0,
                bottom_lift_distance_fast: 0.0,
                bottom_lift_distance_slow: 5.0,
                bottom_lift_speed_fast: 330.0,
                bottom_lift_speed_slow: 330.0,
                bottom_retract_distance_slow: 0.0,
                bottom_retract_speed_fast: 330.0,
                bottom_retract_speed_slow: 330.0,
            },
            slicing: Slicing {
                anti_aliasing: 1,
                anti_aliasing_z: 1,
                exact: false,
                gamma: 1.0,
                blur_radius: 1,
                blur_passes: 0,
                dither_z: false,
                dither_xy: false,
                dither_floor: 50,
                dither_max_angle: 20.0,
            },
            estimates: Estimates { resin_density: 1.1, bottle_volume: 1000.0, bottle_cost: 0.0 },
        }
    }
}

impl PrintSettings {
    /// Layer `layer`'s exposure, s: bottom for the bottom layers, normal from the end of the
    /// transition on, and in the transition a linear step between them that reaches neither
    /// (transition layer `k` of `n` takes `k + 1` steps of `(bottom − normal) / (n + 1)`), so the
    /// first transition layer is already less than a bottom one.
    pub fn exposure_for(&self, layer: u32) -> f32 {
        self.layer_motion(layer).exposure as f32
    }

    /// Whether layer `layer` is exposed as a bottom layer, steps between, or is a normal one.
    pub fn exposure_kind(&self, layer: u32) -> ExposureKind {
        let (bottom, transition) = (self.layers.bottom_count, self.layers.transition_count);
        if layer < bottom {
            ExposureKind::Bottom
        } else if layer < bottom.saturating_add(transition) {
            ExposureKind::Transition
        } else {
            ExposureKind::Normal
        }
    }

    /// How far layer `layer` is from a bottom layer (0) to a normal one (1): the transition
    /// layers step between them as [`PrintSettings::exposure_for`] says.
    fn normal_weight(&self, layer: u32) -> f64 {
        let (bottom, transition) = (self.layers.bottom_count, self.layers.transition_count);
        if layer < bottom {
            0.0
        } else if layer < bottom.saturating_add(transition) {
            f64::from(layer - bottom + 1) / (f64::from(transition) + 1.0)
        } else {
            1.0
        }
    }

    /// Everything layer `layer` does, the transition layers stepping each bottom value towards
    /// its normal one as the exposure does.
    pub fn layer_motion(&self, layer: u32) -> LayerMotion {
        let t = self.normal_weight(layer);
        let step = |bottom: f64, normal: f64| {
            if t == 0.0 {
                bottom
            } else if t == 1.0 {
                normal
            } else {
                bottom + (normal - bottom) * t
            }
        };
        let e = &self.exposure;
        let (b, r) = (self.motion.bottom(), self.motion.regular());
        LayerMotion {
            exposure: step(e.bottom, e.normal),
            light_off: step(e.bottom_light_off, e.light_off),
            // Stepping both ends alike keeps the slow retract within the lift.
            travel: Travel {
                lift_distance_fast: step(b.lift_distance_fast, r.lift_distance_fast),
                lift_distance_slow: step(b.lift_distance_slow, r.lift_distance_slow),
                lift_speed_fast: step(b.lift_speed_fast, r.lift_speed_fast),
                lift_speed_slow: step(b.lift_speed_slow, r.lift_speed_slow),
                retract_distance_slow: step(b.retract_distance_slow, r.retract_distance_slow),
                retract_speed_fast: step(b.retract_speed_fast, r.retract_speed_fast),
                retract_speed_slow: step(b.retract_speed_slow, r.retract_speed_slow),
            },
        }
    }

    fn number(&self, group: &str, key: &str) -> Option<f64> {
        let (l, e, m, s, x) = (&self.layers, &self.exposure, &self.motion, &self.slicing, &self.estimates);
        Some(match (group, key) {
            ("layers", "height") => l.height,
            ("layers", "bottom_count") => l.bottom_count.into(),
            ("layers", "transition_count") => l.transition_count.into(),
            ("exposure", "normal") => e.normal,
            ("exposure", "bottom") => e.bottom,
            ("exposure", "light_off") => e.light_off,
            ("exposure", "bottom_light_off") => e.bottom_light_off,
            ("exposure", "power") => e.power.into(),
            ("exposure", "bottom_power") => e.bottom_power.into(),
            ("motion", "lift_distance_fast") => m.lift_distance_fast,
            ("motion", "lift_distance_slow") => m.lift_distance_slow,
            ("motion", "lift_speed_fast") => m.lift_speed_fast,
            ("motion", "lift_speed_slow") => m.lift_speed_slow,
            ("motion", "retract_distance_slow") => m.retract_distance_slow,
            ("motion", "retract_speed_fast") => m.retract_speed_fast,
            ("motion", "retract_speed_slow") => m.retract_speed_slow,
            ("motion", "bottom_lift_distance_fast") => m.bottom_lift_distance_fast,
            ("motion", "bottom_lift_distance_slow") => m.bottom_lift_distance_slow,
            ("motion", "bottom_lift_speed_fast") => m.bottom_lift_speed_fast,
            ("motion", "bottom_lift_speed_slow") => m.bottom_lift_speed_slow,
            ("motion", "bottom_retract_distance_slow") => m.bottom_retract_distance_slow,
            ("motion", "bottom_retract_speed_fast") => m.bottom_retract_speed_fast,
            ("motion", "bottom_retract_speed_slow") => m.bottom_retract_speed_slow,
            ("slicing", "anti_aliasing") => s.anti_aliasing.into(),
            ("slicing", "anti_aliasing_z") => s.anti_aliasing_z.into(),
            ("slicing", "blur_radius") => s.blur_radius.into(),
            ("slicing", "blur_passes") => s.blur_passes.into(),
            ("slicing", "dither_floor") => s.dither_floor.into(),
            ("slicing", "dither_max_angle") => s.dither_max_angle,
            ("slicing", "gamma") => s.gamma,
            ("estimates", "resin_density") => x.resin_density,
            ("estimates", "bottle_volume") => x.bottle_volume,
            ("estimates", "bottle_cost") => x.bottle_cost,
            _ => return None,
        })
    }

    /// Set number `group.key` to `x`, which [`Field::accepts`] already took. Whether there is one.
    fn set_number(&mut self, group: &str, key: &str, x: f64) -> bool {
        let (l, e, m) = (&mut self.layers, &mut self.exposure, &mut self.motion);
        match (group, key) {
            ("layers", "height") => l.height = x,
            ("layers", "bottom_count") => l.bottom_count = x as u32,
            ("layers", "transition_count") => l.transition_count = x as u32,
            ("exposure", "normal") => e.normal = x,
            ("exposure", "bottom") => e.bottom = x,
            ("exposure", "light_off") => e.light_off = x,
            ("exposure", "bottom_light_off") => e.bottom_light_off = x,
            ("exposure", "power") => e.power = x as u8,
            ("exposure", "bottom_power") => e.bottom_power = x as u8,
            ("motion", "lift_distance_fast") => m.lift_distance_fast = x,
            ("motion", "lift_distance_slow") => m.lift_distance_slow = x,
            ("motion", "lift_speed_fast") => m.lift_speed_fast = x,
            ("motion", "lift_speed_slow") => m.lift_speed_slow = x,
            ("motion", "retract_distance_slow") => m.retract_distance_slow = x,
            ("motion", "retract_speed_fast") => m.retract_speed_fast = x,
            ("motion", "retract_speed_slow") => m.retract_speed_slow = x,
            ("motion", "bottom_lift_distance_fast") => m.bottom_lift_distance_fast = x,
            ("motion", "bottom_lift_distance_slow") => m.bottom_lift_distance_slow = x,
            ("motion", "bottom_lift_speed_fast") => m.bottom_lift_speed_fast = x,
            ("motion", "bottom_lift_speed_slow") => m.bottom_lift_speed_slow = x,
            ("motion", "bottom_retract_distance_slow") => m.bottom_retract_distance_slow = x,
            ("motion", "bottom_retract_speed_fast") => m.bottom_retract_speed_fast = x,
            ("motion", "bottom_retract_speed_slow") => m.bottom_retract_speed_slow = x,
            ("slicing", "anti_aliasing") => self.slicing.anti_aliasing = x as u8,
            ("slicing", "anti_aliasing_z") => self.slicing.anti_aliasing_z = x as u8,
            ("slicing", "blur_radius") => self.slicing.blur_radius = x as u32,
            ("slicing", "blur_passes") => self.slicing.blur_passes = x as u8,
            ("slicing", "dither_floor") => self.slicing.dither_floor = x as u8,
            ("slicing", "dither_max_angle") => self.slicing.dither_max_angle = x,
            ("slicing", "gamma") => self.slicing.gamma = x,
            ("estimates", "resin_density") => self.estimates.resin_density = x,
            ("estimates", "bottle_volume") => self.estimates.bottle_volume = x,
            ("estimates", "bottle_cost") => self.estimates.bottle_cost = x,
            _ => return false,
        }
        true
    }
}

/// How a layer is exposed: see [`PrintSettings::exposure_kind`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExposureKind {
    Bottom,
    Transition,
    Normal,
}

/// One layer's exposure (seconds) and plate travel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayerMotion {
    pub exposure: f64,
    pub light_off: f64,
    pub travel: Travel,
}

impl LayerMotion {
    /// Seconds the layer takes: exposed, dark, lifted and lowered back (mslicer's
    /// `ExposureConfig::print_time`, each phase of the travel at its own speed).
    pub fn seconds(&self) -> f64 {
        self.exposure + self.light_off + self.travel.seconds()
    }
}

/// The groups, as named in parameters and stored settings: the printer profile's, then the
/// print settings'.
pub const GROUPS: [&str; 6] = ["printer", "layers", "exposure", "motion", "slicing", "estimates"];

/// The anti-aliasing levels: sub-samples per pixel side.
pub const ANTI_ALIASING: [u8; 4] = [1, 2, 4, 8];

/// The printer name's longest length, in characters.
pub const MAX_NAME_CHARS: usize = 64;

/// The smallest pixel, mm: below it the profile is a typo.
pub const MIN_PIXEL_PITCH: f64 = 0.001;

/// One number setting and the values it takes: beyond them a value is a typo, not an intent.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Field {
    pub group: &'static str,
    pub key: &'static str,
    pub min: f64,
    pub max: f64,
    pub integer: bool,
}

const fn real(group: &'static str, key: &'static str, min: f64, max: f64) -> Field {
    Field { group, key, min, max, integer: false }
}

const fn int(group: &'static str, key: &'static str, min: f64, max: f64) -> Field {
    Field { group, key, min, max, integer: true }
}

/// "finite, ≥ 0".
const ANY: f64 = f64::MAX;

/// Every number setting. Text, switches and choices (`printer.name`, `printer.mirror_x`,
/// `printer.mirror_y`, `printer.ctb_format`, `slicing.exact`, `slicing.dither_z`, `slicing.dither_xy`) are not here.
pub const FIELDS: &[Field] = &[
    int("printer", "resolution_x", 1.0, 32768.0),
    int("printer", "resolution_y", 1.0, 32768.0),
    real("printer", "display_x", 1.0, 1000.0),
    real("printer", "display_y", 1.0, 1000.0),
    real("printer", "build_height", 1.0, 1000.0),
    real("layers", "height", 0.01, 0.3),
    int("layers", "bottom_count", 0.0, 200.0),
    int("layers", "transition_count", 0.0, 200.0),
    real("exposure", "normal", 0.1, 300.0),
    real("exposure", "bottom", 0.1, 600.0),
    real("exposure", "light_off", 0.0, 120.0),
    real("exposure", "bottom_light_off", 0.0, 120.0),
    int("exposure", "power", 1.0, 255.0),
    int("exposure", "bottom_power", 1.0, 255.0),
    real("motion", "lift_distance_fast", 0.0, 50.0),
    real("motion", "lift_distance_slow", 0.0, 50.0),
    real("motion", "lift_speed_fast", 1.0, 1000.0),
    real("motion", "lift_speed_slow", 1.0, 1000.0),
    real("motion", "retract_distance_slow", 0.0, 50.0),
    real("motion", "retract_speed_fast", 1.0, 1000.0),
    real("motion", "retract_speed_slow", 1.0, 1000.0),
    real("motion", "bottom_lift_distance_fast", 0.0, 50.0),
    real("motion", "bottom_lift_distance_slow", 0.0, 50.0),
    real("motion", "bottom_lift_speed_fast", 1.0, 1000.0),
    real("motion", "bottom_lift_speed_slow", 1.0, 1000.0),
    real("motion", "bottom_retract_distance_slow", 0.0, 50.0),
    real("motion", "bottom_retract_speed_fast", 1.0, 1000.0),
    real("motion", "bottom_retract_speed_slow", 1.0, 1000.0),
    int("slicing", "anti_aliasing", 1.0, 8.0),
    int("slicing", "anti_aliasing_z", 1.0, 8.0),
    int("slicing", "blur_radius", 1.0, 16.0),
    int("slicing", "blur_passes", 0.0, 4.0),
    int("slicing", "dither_floor", 1.0, 100.0),
    real("slicing", "dither_max_angle", 0.0, 90.0),
    real("slicing", "gamma", 0.0, 1.0),
    real("estimates", "resin_density", 0.0, ANY),
    real("estimates", "bottle_volume", 0.0, ANY),
    real("estimates", "bottle_cost", 0.0, ANY),
];

/// Number setting `group.key`.
pub fn field(group: &str, key: &str) -> Option<&'static Field> {
    FIELDS.iter().find(|f| f.group == group && f.key == key)
}

impl Field {
    /// Whether `x` is a value of this setting.
    pub fn accepts(&self, x: f64) -> bool {
        let in_range = x.is_finite() && x >= self.min && x <= self.max && (!self.integer || x.fract() == 0.0);
        in_range && (!self.is_level() || ANTI_ALIASING.iter().any(|a| f64::from(*a) == x))
    }

    /// Whether it takes only the [`ANTI_ALIASING`] levels.
    fn is_level(&self) -> bool {
        self.group == "slicing" && (self.key == "anti_aliasing" || self.key == "anti_aliasing_z")
    }

    /// What it must be, for an error message.
    pub fn describe(&self) -> String {
        let (g, k) = (self.group, self.key);
        if self.is_level() {
            format!("`{g}.{k}` must be 1, 2, 4 or 8")
        } else if self.integer {
            format!("`{g}.{k}` must be an integer from {} to {}", self.min, self.max)
        } else if self.max == ANY {
            format!("`{g}.{k}` must be a finite number from {}", self.min)
        } else {
            format!("`{g}.{k}` must be a number from {} to {}", self.min, self.max)
        }
    }

    /// Its value in `printer` and `settings`.
    pub fn get(&self, printer: &PrinterProfile, settings: &PrintSettings) -> Option<f64> {
        if self.group == "printer" { printer.number(self.key) } else { settings.number(self.group, self.key) }
    }

    /// Set it to `x`, if [`Field::accepts`] it. Whether it was set.
    pub fn set(&self, printer: &mut PrinterProfile, settings: &mut PrintSettings, x: f64) -> bool {
        if !self.accepts(x) {
            return false;
        }
        if self.group == "printer" { printer.set_number(self.key, x) } else { settings.set_number(self.group, self.key, x) }
    }
}

/// Every field in its range, and the fields that depend on each other agreeing: the bottom and
/// transition layers fit in the build height, a pixel is at least [`MIN_PIXEL_PITCH`] wide, and
/// a slow retract is no longer than its lift. The error names the field (`layers.height`).
pub fn validate(printer: &PrinterProfile, settings: &PrintSettings) -> Result<(), String> {
    if printer.name.chars().count() > MAX_NAME_CHARS || printer.name.chars().any(char::is_control) {
        return Err(format!("`printer.name` must be at most {MAX_NAME_CHARS} characters, without control characters"));
    }
    for f in FIELDS {
        if !f.get(printer, settings).is_some_and(|x| f.accepts(x)) {
            return Err(f.describe());
        }
    }
    let (px, py) = printer.pixel_pitch();
    if !(px >= MIN_PIXEL_PITCH && py >= MIN_PIXEL_PITCH) {
        return Err(format!("a pixel must be at least {} µm wide: make the display larger or the resolution smaller", MIN_PIXEL_PITCH * 1000.0));
    }
    let l = &settings.layers;
    // The small allowance keeps an exact fit (250 mm of 0.05 mm layers) from rounding down.
    let fit = (printer.build_height / l.height + 1e-6).floor();
    if f64::from(l.bottom_count) + f64::from(l.transition_count) > fit {
        return Err(format!(
            "the bottom and transition layers ({}) must fit in the build height ({fit} layers)",
            l.bottom_count + l.transition_count
        ));
    }
    let m = &settings.motion;
    for (key, travel) in [("retract_distance_slow", m.regular()), ("bottom_retract_distance_slow", m.bottom())] {
        // The allowance keeps a slow retract typed as the whole lift (0.8 of 0.7 + 0.1) in.
        if travel.retract_distance_slow > travel.lift_distance() + 1e-9 {
            let lift = (travel.lift_distance() * 1000.0).round() / 1000.0;
            return Err(format!("`motion.{key}` must be at most the lift distance ({lift} mm)"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_pass_and_every_field_is_reachable() {
        let (p, s) = (PrinterProfile::default(), PrintSettings::default());
        assert_eq!(validate(&p, &s), Ok(()));
        for f in FIELDS {
            assert!(f.get(&p, &s).is_some(), "{}.{}", f.group, f.key);
            assert!(GROUPS.contains(&f.group));
            let (mut p2, mut s2) = (p.clone(), s);
            assert!(f.set(&mut p2, &mut s2, f.min), "{}.{} takes its minimum", f.group, f.key);
            assert_eq!(f.get(&p2, &s2), Some(f.min));
        }
        let (px, py) = p.pixel_pitch();
        assert!((px - 0.0285).abs() < 1e-6 && (py - 0.0285).abs() < 1e-6);
    }

    #[test]
    fn validate_rejects_every_out_of_range_field_and_non_finite_numbers() {
        let (p, s) = (PrinterProfile::default(), PrintSettings::default());
        for f in FIELDS {
            let mut bad = vec![f64::NAN, f64::INFINITY, f64::NEG_INFINITY, f.min - 1.0];
            if f.max != ANY {
                bad.push(f.max + 1.0);
            }
            if f.integer {
                bad.push(f.min + 0.5);
            }
            for x in bad {
                // `set` refuses it, and so does `validate` when it is forced in.
                let (mut p2, mut s2) = (p.clone(), s);
                assert!(!f.set(&mut p2, &mut s2, x), "{}.{} = {x}", f.group, f.key);
                let forced = if f.group == "printer" { p2.set_number(f.key, x) } else { s2.set_number(f.group, f.key, x) };
                assert!(forced);
                // An integer field casts NaN to 0 and saturates; only a cast that lands outside
                // its range is left for `validate` to find.
                if f.get(&p2, &s2).is_some_and(|v| f.accepts(v)) {
                    continue;
                }
                let why = validate(&p2, &s2).expect_err(&format!("{}.{} = {x}", f.group, f.key));
                assert!(why.contains(&format!("{}.{}", f.group, f.key)), "{why}");
            }
        }
        // Every real field forced to NaN is caught.
        for f in FIELDS.iter().filter(|f| !f.integer) {
            let (mut p2, mut s2) = (p.clone(), s);
            if f.group == "printer" {
                p2.set_number(f.key, f64::NAN)
            } else {
                s2.set_number(f.group, f.key, f64::NAN)
            };
            assert!(validate(&p2, &s2).is_err(), "{}.{}", f.group, f.key);
        }
        for aa in [0.0, 3.0, 5.0, 16.0] {
            assert!(!field("slicing", "anti_aliasing").is_some_and(|f| f.accepts(aa)), "{aa}");
            assert!(!field("slicing", "anti_aliasing_z").is_some_and(|f| f.accepts(aa)), "{aa}");
        }
    }

    #[test]
    fn validate_checks_the_fields_together() {
        let s = PrintSettings::default();
        let long = PrinterProfile { name: "x".repeat(MAX_NAME_CHARS + 1), ..PrinterProfile::default() };
        assert!(validate(&long, &s).unwrap_err().contains("printer.name"));
        assert!(validate(&PrinterProfile { name: "a\nb".into(), ..PrinterProfile::default() }, &s).is_err());
        assert!(validate(&PrinterProfile { name: "é".repeat(MAX_NAME_CHARS), ..PrinterProfile::default() }, &s).is_ok());

        // 1 mm across 32768 pixels: 0.03 µm.
        let tiny = PrinterProfile { display_x: 1.0, resolution_x: 32768, ..PrinterProfile::default() };
        assert!(validate(&tiny, &s).unwrap_err().contains("µm"));

        // 2 mm of build height holds 40 layers of 0.05 mm: 40 fit, 41 don't.
        let short = PrinterProfile { build_height: 2.0, ..PrinterProfile::default() };
        let mut fits = s;
        fits.layers.bottom_count = 36;
        assert_eq!(validate(&short, &fits), Ok(()));
        fits.layers.bottom_count = 37;
        assert!(validate(&short, &fits).unwrap_err().contains("build height"));
    }

    #[test]
    fn exposure_goes_from_bottom_through_the_transition_to_normal() {
        let s = PrintSettings::default();
        let (bottom, transition) = (s.layers.bottom_count, s.layers.transition_count);
        for i in 0..bottom {
            assert_eq!(s.exposure_for(i), 30.0);
            assert_eq!(s.exposure_kind(i), ExposureKind::Bottom);
        }
        assert_eq!(s.exposure_kind(bottom), ExposureKind::Transition);
        assert_eq!(s.exposure_kind(bottom + transition - 1), ExposureKind::Transition);
        assert_eq!(s.exposure_kind(bottom + transition), ExposureKind::Normal);
        let mut last = 30.0;
        for i in bottom..bottom + transition {
            let e = s.exposure_for(i);
            assert!(e > 3.0 && e < 30.0, "layer {i}: {e}");
            assert!(e < last, "layer {i} decreases");
            last = e;
        }
        // Even steps of (30 − 3) / 5.
        assert!((s.exposure_for(bottom) - 24.6).abs() < 1e-4);
        assert!((s.exposure_for(bottom + transition - 1) - 8.4).abs() < 1e-4);
        for i in [bottom + transition, bottom + transition + 1, 4999, u32::MAX] {
            assert_eq!(s.exposure_for(i), 3.0);
        }
        // No transition: straight from bottom to normal; no bottom layers: the transition first.
        let mut none = s;
        none.layers.transition_count = 0;
        assert_eq!((none.exposure_for(bottom - 1), none.exposure_for(bottom)), (30.0, 3.0));
        none.layers.bottom_count = 0;
        none.layers.transition_count = 2;
        assert!((none.exposure_for(0) - 21.0).abs() < 1e-4);
        assert_eq!(none.exposure_for(2), 3.0);
    }

    #[test]
    fn travel_times_each_phase_and_the_retract_comes_back_the_whole_lift() {
        let s = PrintSettings::default();
        // The defaults are one phase each way: 5 mm up and down at 330 mm/min.
        for t in [s.motion.regular(), s.motion.bottom()] {
            assert_eq!((t.lift_distance(), t.retract_distance_fast()), (5.0, 5.0));
            assert!((t.seconds() - 2.0 * 5.0 / 330.0 * 60.0).abs() < 1e-12);
        }
        let t = Travel {
            lift_distance_fast: 4.0,
            lift_distance_slow: 2.0,
            lift_speed_fast: 240.0,
            lift_speed_slow: 60.0,
            retract_distance_slow: 1.5,
            retract_speed_fast: 300.0,
            retract_speed_slow: 90.0,
        };
        assert_eq!(t.retract_distance_fast(), 4.5);
        // 2 s slow up, 1 s fast up, 0.9 s fast down, 1 s slow down.
        assert!((t.seconds() - 4.9).abs() < 1e-12, "{}", t.seconds());

        let mut m = s.motion;
        m.lift_distance_fast = 3.0;
        m.lift_distance_slow = 1.0;
        m.retract_distance_slow = 3.0;
        m.bottom_retract_distance_slow = 2.0;
        m.fit_retract(false);
        assert_eq!((m.retract_distance_slow, m.regular().retract_distance_fast()), (3.0, 1.0), "within the lift: kept");
        m.lift_distance_fast = 1.5;
        m.fit_retract(false);
        assert_eq!((m.retract_distance_slow, m.regular().retract_distance_fast()), (2.5, 0.0));
        assert_eq!(m.bottom_retract_distance_slow, 2.0, "the bottom's is its own");
    }

    #[test]
    fn validate_keeps_each_slow_retract_within_its_lift() {
        let p = PrinterProfile::default();
        let mut s = PrintSettings::default();
        s.motion.retract_distance_slow = 5.0;
        s.motion.bottom_retract_distance_slow = 5.0;
        assert_eq!(validate(&p, &s), Ok(()), "the whole lift");
        let mut sum = s;
        (sum.motion.lift_distance_slow, sum.motion.lift_distance_fast, sum.motion.retract_distance_slow) = (0.7, 0.1, 0.8);
        assert_eq!(validate(&p, &sum), Ok(()), "0.7 + 0.1 is 0.8, rounding aside");
        sum.motion.retract_distance_slow = 0.81;
        assert!(validate(&p, &sum).unwrap_err().contains("(0.8 mm)"));
        s.motion.lift_distance_slow = 4.0;
        assert!(validate(&p, &s).unwrap_err().contains("motion.retract_distance_slow"));
        s.motion.fit_retract(false);
        assert_eq!(validate(&p, &s), Ok(()));
        s.motion.bottom_lift_distance_fast = 0.0;
        s.motion.bottom_lift_distance_slow = 2.0;
        assert!(validate(&p, &s).unwrap_err().contains("motion.bottom_retract_distance_slow"));
    }

    #[test]
    fn the_transition_steps_the_travel_from_bottom_to_regular() {
        let mut s = PrintSettings::default();
        s.layers.transition_count = 1;
        s.motion.bottom_lift_distance_slow = 8.0;
        s.motion.bottom_retract_distance_slow = 8.0;
        s.motion.lift_distance_slow = 2.0;
        s.motion.lift_distance_fast = 4.0;
        s.motion.retract_speed_slow = 30.0;
        let bottom = s.layers.bottom_count;
        assert_eq!(s.layer_motion(bottom - 1).travel, s.motion.bottom());
        let t = s.layer_motion(bottom).travel;
        assert_eq!((t.lift_distance_slow, t.lift_distance_fast, t.retract_distance_slow), (5.0, 2.0, 4.0));
        assert_eq!((t.retract_speed_slow, t.retract_distance_fast()), (180.0, 3.0));
        assert_eq!(s.layer_motion(bottom + 1).travel, s.motion.regular());
    }

    #[test]
    fn settings_serialize_in_their_groups() {
        let v = serde_json::to_value(PrintSettings::default()).unwrap();
        assert_eq!(v["layers"]["bottom_count"], 10);
        assert_eq!(v["exposure"]["normal"], 3.0);
        assert_eq!(v["slicing"]["anti_aliasing"], 1);
        let p = serde_json::to_value(PrinterProfile::default()).unwrap();
        assert_eq!(p["display_x"], 218.88);
        assert_eq!(p["name"], "ELEGOO SATURN 2");
    }
}
