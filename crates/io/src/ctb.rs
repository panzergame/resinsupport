// The layer RLE (`LayerEncoder`, `LayerDecoder`), the layer cipher (`xor_cypher`) and the preview
// encoding (`PreviewImage::to_bytes`, `from_bytes`) are ported from mslicer (GPL-3.0),
// `format/ctb_format/src/shared/encoding.rs` and `shared/preview.rs` at 355aca2
// (https://github.com/connorslade/mslicer); see ATTRIBUTION.md. The v2 layout below was written
// from the field definitions in UVtools `UVtools.Core/FileFormats/ChituboxFile.cs` at c2d984c
// (`Header`, `PrintParameters`, `SlicerInfo`, `Preview`, `LayerDef`, `OnBeforeEncode`,
// `EncodeInternally`): facts about the format, no UVtools code, so this file is GPL-3.0.

//! Chitubox `.ctb` files, format version 2: what an Elegoo Saturn 2 reads.
//!
//! All numbers are little-endian, `f32` is IEEE 754 and offsets are `u32` from the file's start.
//! The sections follow one another in this order: header, large preview, small preview, print
//! parameters, slicer info and the machine name, layer table, layer data. A layer is its runs of
//! grey, 7 bits kept, run-length encoded and then XORed with a key stream unless the key is 0.
//!
//! [`read_ctb`] treats its bytes as hostile: every offset is checked against the file, counts are
//! capped before anything is allocated, and nothing panics.

use resin_slice::{Cancel, Progress, Run, SlicedJob};

use crate::IoError;

pub const MAGIC: u32 = 0x12FD_0086;
pub const VERSION: u32 = 2;
pub const HEADER_SIZE: usize = 112;
pub const PREVIEW_HEADER_SIZE: usize = 32;
pub const PRINT_PARAMS_SIZE: usize = 60;
/// Without the machine name that follows it.
pub const SLICER_INFO_SIZE: usize = 76;
pub const LAYER_DEF_SIZE: usize = 36;
/// The preview sizes Chitubox writes (UVtools `ThumbnailsOriginalSize`).
pub const LARGE_PREVIEW: (u32, u32) = (400, 300);
pub const SMALL_PREVIEW: (u32, u32) = (200, 125);
/// mslicer's key: any nonzero one works, a fixed one keeps the output reproducible.
pub const DEFAULT_KEY: u32 = 0x67;
/// The file name extension.
pub const EXTENSION: &str = "ctb";

const PAGE: u64 = 1 << 32;
const MAX_RUN: u64 = 0x0FFF_FFFF;
const MAX_PREVIEW_RUN: u32 = 0xFFF;
const PREVIEW_REPEAT: u16 = 0x20;
/// UVtools' default, Chitubox 1.9.0.
const SOFTWARE_VERSION: u32 = 0x0109_0000;
/// Written in the print parameters' last word by v2 files (UVtools `OnBeforeEncode`).
const V2_MARKER: u32 = 0x1234;
const AA_FLAG_OFF: u8 = 0x07;
const AA_FLAG_ON: u8 = 0x0F;
const MAX_LAYERS: u32 = 1_000_000;
const MAX_RESOLUTION: u32 = 32_768;
const MAX_PREVIEW_PIXELS: u64 = 1024 * 1024;
const MAX_NAME_BYTES: u32 = 1024;
const PLAIN_BACKGROUND: [u8; 3] = [0x30, 0x30, 0x30];
const PLAIN_SHAPE: [u8; 3] = [0xFF, 0xFF, 0xFF];

/// What [`write_ctb`] needs besides the job.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CtbOptions {
    /// 0 writes the layers unciphered.
    pub encryption_key: u32,
    /// Minutes since the Unix epoch: a parameter, so the same job writes the same bytes.
    pub modified_minutes: u32,
}

impl Default for CtbOptions {
    fn default() -> Self {
        CtbOptions { encryption_key: DEFAULT_KEY, modified_minutes: 0 }
    }
}

/// An RGB image, 8 bits a channel, row-major.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Preview {
    pub width: u32,
    pub height: u32,
    pub rgb: Vec<u8>,
}

/// The two images the printer lists a file with: [`LARGE_PREVIEW`] and [`SMALL_PREVIEW`] in size.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Previews {
    pub large: Preview,
    pub small: Preview,
}

impl Previews {
    /// The job seen from above, every layer at once, in white on dark grey: for when there is no
    /// renderer to draw the scene.
    pub fn plain(job: &SlicedJob) -> Previews {
        Previews { large: plain_preview(job, LARGE_PREVIEW), small: plain_preview(job, SMALL_PREVIEW) }
    }
}

/// The screen fitted into `size`, centred, with every pixel any layer lights in white.
fn plain_preview(job: &SlicedJob, (pw, ph): (u32, u32)) -> Preview {
    let mut rgb: Vec<u8> = PLAIN_BACKGROUND.iter().copied().cycle().take(pw as usize * ph as usize * 3).collect();
    let (w, h) = (u64::from(job.profile.resolution_x), u64::from(job.profile.resolution_y));
    if w == 0 || h == 0 || pw == 0 || ph == 0 {
        return Preview { width: pw, height: ph, rgb };
    }
    let s = (f64::from(pw) / w as f64).min(f64::from(ph) / h as f64);
    let iw = ((w as f64 * s).round() as u32).clamp(1, pw);
    let ih = ((h as f64 * s).round() as u32).clamp(1, ph);
    let (x0, y0) = ((pw - iw) / 2, (ph - ih) / 2);
    let mut lit = vec![false; iw as usize * ih as usize];
    for layer in &job.layers {
        let mut at = 0u64;
        for r in &layer.runs {
            let (mut from, to) = (at, at.saturating_add(r.len).min(w * h));
            at = to;
            if r.grey == 0 {
                continue;
            }
            while from < to {
                let row = from / w;
                let end = to.min((row + 1) * w);
                let cy = ((row as f64 * s) as u32).min(ih - 1);
                let cx0 = (((from - row * w) as f64 * s) as u32).min(iw - 1);
                let cx1 = (((end - row * w) as f64 * s).ceil() as u32).clamp(cx0 + 1, iw);
                let base = cy as usize * iw as usize;
                if let Some(cells) = lit.get_mut(base + cx0 as usize..base + cx1 as usize) {
                    cells.fill(true);
                }
                from = end;
            }
        }
    }
    for (i, _) in lit.iter().enumerate().filter(|(_, l)| **l) {
        let (cx, cy) = (i as u32 % iw, i as u32 / iw);
        let at = ((y0 + cy) as usize * pw as usize + (x0 + cx) as usize) * 3;
        if let Some(px) = rgb.get_mut(at..at + 3) {
            px.copy_from_slice(&PLAIN_SHAPE);
        }
    }
    Preview { width: pw, height: ph, rgb }
}

/// The file header, at offset 0.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Header {
    pub magic: u32,
    pub version: u32,
    /// The screen's width and depth and the build height, mm.
    pub bed: [f32; 3],
    /// The last layer's top, mm.
    pub total_height: f32,
    pub layer_height: f32,
    /// Seconds.
    pub exposure: f32,
    pub bottom_exposure: f32,
    pub light_off_delay: f32,
    pub bottom_layers: u32,
    pub resolution_x: u32,
    pub resolution_y: u32,
    pub large_preview_offset: u32,
    pub layer_table_offset: u32,
    pub layer_count: u32,
    pub small_preview_offset: u32,
    /// Seconds.
    pub print_time: u32,
    /// 0 normal, 1 the screen mirrored along X.
    pub projector_type: u32,
    pub print_params_offset: u32,
    pub print_params_size: u32,
    /// Always 1 in a CTB: the real level is in the slicer info.
    pub aa_level: u32,
    pub light_pwm: u16,
    pub bottom_light_pwm: u16,
    pub encryption_key: u32,
    pub slicer_info_offset: u32,
    pub slicer_info_size: u32,
}

/// Heights in mm, speeds in mm/min, times in seconds.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PrintParams {
    pub bottom_lift_height: f32,
    pub bottom_lift_speed: f32,
    pub lift_height: f32,
    pub lift_speed: f32,
    pub retract_speed: f32,
    pub volume_ml: f32,
    pub weight_g: f32,
    pub cost: f32,
    pub bottom_light_off_delay: f32,
    pub light_off_delay: f32,
    pub bottom_layer_count: u32,
    /// [`V2_MARKER`] in a v2 file.
    pub marker: u32,
}

/// The slicer info, v2's part of it: the second-stage motions are v4's and stay 0.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SlicerInfo {
    pub machine_name_offset: u32,
    pub machine_name_size: u32,
    pub aa_flag: u8,
    pub per_layer_settings: u8,
    pub modified_minutes: u32,
    pub aa_level: u32,
    pub software_version: u32,
    pub transition_layers: u32,
}

/// One entry of the layer table.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LayerDef {
    /// The layer's top, mm.
    pub z: f32,
    pub exposure: f32,
    pub light_off: f32,
    /// Within page `page` of 4 GiB.
    pub data_offset: u32,
    pub data_size: u32,
    pub page: u32,
    pub table_size: u32,
}

/// A layer as read back: where it is, how it is exposed, and its runs, neighbours of one grey
/// merged.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CtbLayer {
    pub z: f32,
    pub exposure: f32,
    pub light_off: f32,
    pub runs: Vec<Run>,
}

/// A whole file as read back.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CtbFile {
    pub header: Header,
    pub params: PrintParams,
    pub slicer: SlicerInfo,
    pub machine_name: String,
    pub large_preview: Option<Preview>,
    pub small_preview: Option<Preview>,
    pub layers: Vec<CtbLayer>,
}

/// Now, in minutes since the Unix epoch, for [`CtbOptions::modified_minutes`]. Not in the
/// browser, whose `SystemTime` panics.
#[cfg(not(target_arch = "wasm32"))]
pub fn minutes_now() -> u32 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| u32::try_from(d.as_secs() / 60).unwrap_or(u32::MAX))
}

/// Whether `path` names a CTB file.
pub fn is_ctb_path(path: &std::path::Path) -> bool {
    path.extension().is_some_and(|e| e.eq_ignore_ascii_case(EXTENSION))
}

/// `job` as a CTB v2 file.
pub fn write_ctb(job: &SlicedJob, previews: &Previews, opts: &CtbOptions) -> Result<Vec<u8>, IoError> {
    write_ctb_with(job, previews, opts, &Progress::default(), &Cancel::default())
}

/// [`write_ctb`], counting the layers written in `progress` and stopping with
/// [`IoError::Cancelled`] once `cancel` is set.
pub fn write_ctb_with(job: &SlicedJob, previews: &Previews, opts: &CtbOptions, progress: &Progress, cancel: &Cancel) -> Result<Vec<u8>, IoError> {
    let (p, s, e, m) = (&job.profile, &job.settings, &job.settings.exposure, &job.settings.motion);
    let count = u32::try_from(job.layers.len()).map_err(|_| IoError::TooLarge("too many layers".into()))?;
    if count == 0 {
        return Err(IoError::Format("the job has no layers".into()));
    }
    let pixels = u64::from(p.resolution_x) * u64::from(p.resolution_y);
    let large = encode_preview(&previews.large)?;
    let small = encode_preview(&previews.small)?;
    let name = p.name.as_bytes();

    let large_at = HEADER_SIZE as u64;
    let small_at = large_at + (PREVIEW_HEADER_SIZE + large.len()) as u64;
    let params_at = small_at + (PREVIEW_HEADER_SIZE + small.len()) as u64;
    let slicer_at = params_at + PRINT_PARAMS_SIZE as u64;
    let name_at = slicer_at + SLICER_INFO_SIZE as u64;
    let table_at = name_at + name.len() as u64;
    let data_at = table_at + LAYER_DEF_SIZE as u64 * u64::from(count);
    // Only the layer data may pass 4 GiB, through its page number.
    let below_4g = |v: u64| u32::try_from(v).map_err(|_| IoError::TooLarge("the previews, settings and layer table pass 4 GiB".into()));
    below_4g(data_at)?;

    let volume = job.stats.volume_ml;
    let cost = if s.estimates.bottle_volume > 0.0 { volume / s.estimates.bottle_volume * s.estimates.bottle_cost } else { 0.0 };
    let header = Header {
        magic: MAGIC,
        version: VERSION,
        bed: [p.display_x as f32, p.display_y as f32, p.build_height as f32],
        total_height: job.layers.last().map_or(0.0, |l| l.z),
        layer_height: s.layers.height as f32,
        exposure: e.normal as f32,
        bottom_exposure: e.bottom as f32,
        light_off_delay: e.light_off as f32,
        bottom_layers: s.layers.bottom_count,
        resolution_x: p.resolution_x,
        resolution_y: p.resolution_y,
        large_preview_offset: below_4g(large_at)?,
        layer_table_offset: below_4g(table_at)?,
        layer_count: count,
        small_preview_offset: below_4g(small_at)?,
        print_time: job.stats.print_time_s.ceil() as u32,
        // The rasterizer has already mirrored the image; this tells the printer it was.
        projector_type: u32::from(p.mirror_x),
        print_params_offset: below_4g(params_at)?,
        print_params_size: PRINT_PARAMS_SIZE as u32,
        aa_level: 1,
        light_pwm: e.power.into(),
        bottom_light_pwm: e.bottom_power.into(),
        encryption_key: opts.encryption_key,
        slicer_info_offset: below_4g(slicer_at)?,
        slicer_info_size: SLICER_INFO_SIZE as u32,
    };
    let params = PrintParams {
        bottom_lift_height: m.bottom_lift_height as f32,
        bottom_lift_speed: m.bottom_lift_speed as f32,
        lift_height: m.lift_height as f32,
        lift_speed: m.lift_speed as f32,
        retract_speed: m.retract_speed as f32,
        volume_ml: volume as f32,
        weight_g: (volume * s.estimates.resin_density) as f32,
        cost: cost as f32,
        bottom_light_off_delay: e.bottom_light_off as f32,
        light_off_delay: e.light_off as f32,
        bottom_layer_count: s.layers.bottom_count,
        marker: V2_MARKER,
    };
    let aa_level = u32::from(s.slicing.anti_aliasing.max(1));
    let slicer = SlicerInfo {
        machine_name_offset: below_4g(name_at)?,
        machine_name_size: below_4g(name.len() as u64)?,
        aa_flag: if aa_level == 1 { AA_FLAG_OFF } else { AA_FLAG_ON },
        // Per-layer settings disallowed.
        per_layer_settings: 0,
        modified_minutes: opts.modified_minutes,
        aa_level,
        software_version: SOFTWARE_VERSION,
        transition_layers: s.layers.transition_count,
    };

    let mut out = Vec::with_capacity(data_at as usize);
    header.write(&mut out);
    write_preview(&mut out, &previews.large, &large);
    write_preview(&mut out, &previews.small, &small);
    params.write(&mut out);
    slicer.write(&mut out);
    out.extend_from_slice(name);
    let table = out.len();
    out.resize(table + LAYER_DEF_SIZE * job.layers.len(), 0);

    progress.begin(job.layers.len());
    let mut entry = Vec::with_capacity(LAYER_DEF_SIZE);
    for (i, layer) in (0u32..).zip(&job.layers) {
        if cancel.is_cancelled() {
            return Err(IoError::Cancelled);
        }
        let mut data = encode_layer(&layer.runs, pixels).map_err(|why| IoError::Format(format!("layer {i}: {why}")))?;
        crypt(&mut data, opts.encryption_key, i);
        let at = out.len() as u64;
        let def = LayerDef {
            z: layer.z,
            exposure: layer.exposure,
            light_off: layer.light_off,
            data_offset: (at % PAGE) as u32,
            data_size: u32::try_from(data.len()).map_err(|_| IoError::TooLarge(format!("layer {i} is over 4 GiB")))?,
            page: u32::try_from(at / PAGE).map_err(|_| IoError::TooLarge("the file is too large".into()))?,
            table_size: LAYER_DEF_SIZE as u32,
        };
        entry.clear();
        def.write(&mut entry);
        let slot = table + i as usize * LAYER_DEF_SIZE;
        if let Some(dst) = out.get_mut(slot..slot + LAYER_DEF_SIZE) {
            dst.copy_from_slice(&entry);
        }
        out.extend_from_slice(&data);
        progress.advance();
    }
    Ok(out)
}

/// Read a CTB v2 file.
pub fn read_ctb(bytes: &[u8]) -> Result<CtbFile, IoError> {
    let header = Header::read(&mut Reader::at(bytes, 0, HEADER_SIZE)?)?;
    if header.magic != MAGIC {
        return Err(IoError::Format("not a CTB file".into()));
    }
    if header.version != VERSION {
        return Err(IoError::Format(format!("CTB version {} is not supported", header.version)));
    }
    let (rx, ry) = (header.resolution_x, header.resolution_y);
    if !(1..=MAX_RESOLUTION).contains(&rx) || !(1..=MAX_RESOLUTION).contains(&ry) {
        return Err(IoError::Format(format!("bad screen resolution {rx} × {ry}")));
    }
    if header.layer_count > MAX_LAYERS {
        return Err(IoError::TooLarge(format!("{} layers", header.layer_count)));
    }
    let count = header.layer_count as usize;
    if (header.print_params_size as usize) < PRINT_PARAMS_SIZE {
        return Err(IoError::Format("the print parameters are too short".into()));
    }
    let params = PrintParams::read(&mut Reader::at(bytes, header.print_params_offset.into(), PRINT_PARAMS_SIZE)?)?;
    let (slicer, machine_name) = if header.slicer_info_offset == 0 {
        (SlicerInfo::default(), String::new())
    } else {
        if (header.slicer_info_size as usize) < SLICER_INFO_SIZE {
            return Err(IoError::Format("the slicer info is too short".into()));
        }
        let slicer = SlicerInfo::read(&mut Reader::at(bytes, header.slicer_info_offset.into(), SLICER_INFO_SIZE)?)?;
        if slicer.machine_name_size > MAX_NAME_BYTES {
            return Err(IoError::TooLarge("the machine name".into()));
        }
        let name = section(bytes, slicer.machine_name_offset.into(), slicer.machine_name_size.into())?;
        (slicer, String::from_utf8_lossy(name).into_owned())
    };
    let large_preview = read_preview(bytes, header.large_preview_offset)?;
    let small_preview = read_preview(bytes, header.small_preview_offset)?;

    let mut table = Reader::at(bytes, header.layer_table_offset.into(), LAYER_DEF_SIZE * count)?;
    let pixels = u64::from(rx) * u64::from(ry);
    let mut layers = Vec::with_capacity(count);
    // v2 files never share a layer's data, and entries that did could make a small file decode
    // the same block over and over: the data read may not add up to more than the file.
    let mut read = 0u64;
    for i in 0..header.layer_count {
        let def = LayerDef::read(&mut table)?;
        let at = u64::from(def.page) * PAGE + u64::from(def.data_offset);
        read += u64::from(def.data_size);
        if read > bytes.len() as u64 {
            return Err(IoError::Format("the layers' data adds up to more than the file".into()));
        }
        let mut data = section(bytes, at, def.data_size.into())?.to_vec();
        crypt(&mut data, header.encryption_key, i);
        let runs = decode_layer(&data, pixels).map_err(|why| IoError::Format(format!("layer {i}: {why}")))?;
        layers.push(CtbLayer { z: def.z, exposure: def.exposure, light_off: def.light_off, runs });
    }
    Ok(CtbFile { header, params, slicer, machine_name, large_preview, small_preview, layers })
}

/// `runs` as the layer RLE: one stream over the whole image, runs crossing row ends. Neighbours
/// whose 7-bit grey is the same are merged, runs too long for one record split. An error if they
/// do not cover exactly `pixels`.
fn encode_layer(runs: &[Run], pixels: u64) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut total = 0u64;
    let mut pending: Option<(u8, u64)> = None;
    for r in runs {
        total = total.checked_add(r.len).ok_or("the runs overflow")?;
        if r.len == 0 {
            continue;
        }
        let v = r.grey >> 1;
        match &mut pending {
            Some((pv, n)) if *pv == v => *n += r.len,
            _ => {
                if let Some((pv, n)) = pending {
                    push_run(&mut out, pv, n);
                }
                pending = Some((v, r.len));
            }
        }
    }
    if let Some((pv, n)) = pending {
        push_run(&mut out, pv, n);
    }
    if total != pixels {
        return Err(format!("the runs cover {total} pixels, not {pixels}"));
    }
    Ok(out)
}

/// One run of 7-bit grey `v`, in as many records as it needs.
fn push_run(out: &mut Vec<u8>, v: u8, mut len: u64) {
    while len > 0 {
        let n = len.min(MAX_RUN);
        out.push(v | if n > 1 { 0x80 } else { 0 });
        match n {
            1 => {}
            2..=0x7F => out.push(n as u8),
            0x80..=0x3FFF => out.extend_from_slice(&[(n >> 8) as u8 | 0x80, n as u8]),
            0x4000..=0x1F_FFFF => out.extend_from_slice(&[(n >> 16) as u8 | 0xC0, (n >> 8) as u8, n as u8]),
            _ => out.extend_from_slice(&[(n >> 24) as u8 | 0xE0, (n >> 16) as u8, (n >> 8) as u8, n as u8]),
        }
        len -= n;
    }
}

/// The layer RLE back to runs, neighbours of one grey merged. An error unless they cover exactly
/// `pixels`; at most one run per byte, so what is allocated is bounded by the data.
fn decode_layer(data: &[u8], pixels: u64) -> Result<Vec<Run>, String> {
    let truncated = || "the layer data is cut short".to_string();
    let mut runs: Vec<Run> = Vec::new();
    let (mut total, mut i) = (0u64, 0usize);
    while let Some(&b) = data.get(i) {
        i += 1;
        let v = b & 0x7F;
        let len = if b & 0x80 == 0 {
            1
        } else {
            let first = u64::from(*data.get(i).ok_or_else(truncated)?);
            let (extra, top) = match first {
                f if f & 0x80 == 0 => (0, f),
                f if f & 0xC0 == 0x80 => (1, f & 0x3F),
                f if f & 0xE0 == 0xC0 => (2, f & 0x1F),
                f if f & 0xF0 == 0xE0 => (3, f & 0x0F),
                _ => return Err("a run's length is malformed".into()),
            };
            let rest = data.get(i + 1..i + 1 + extra).ok_or_else(truncated)?;
            i += 1 + extra;
            rest.iter().fold(top, |acc, &b| acc << 8 | u64::from(b))
        };
        if len == 0 {
            continue;
        }
        total = total.saturating_add(len);
        if total > pixels {
            return Err(format!("the runs cover more than {pixels} pixels"));
        }
        let grey = if v == 0 { 0 } else { (v << 1) | 1 };
        match runs.last_mut() {
            Some(r) if r.grey == grey => r.len += len,
            _ => runs.push(Run { len, grey }),
        }
    }
    if total != pixels {
        return Err(format!("the runs cover {total} pixels, not {pixels}"));
    }
    Ok(runs)
}

/// XOR layer `layer`'s data with its key stream; its own inverse. Key 0 leaves the data alone.
fn crypt(data: &mut [u8], key: u32, layer: u32) {
    if key == 0 {
        return;
    }
    let init = key.wrapping_mul(0x2D83_CDAC).wrapping_add(0xD8A8_3423);
    let mut k = layer.wrapping_mul(0x1E15_30CD).wrapping_add(0xEC3D_47CD).wrapping_mul(init);
    for chunk in data.chunks_mut(4) {
        for (b, kb) in chunk.iter_mut().zip(k.to_le_bytes()) {
            *b ^= kb;
        }
        k = k.wrapping_add(init);
    }
}

/// RGB565 with the repeat flag, green's lowest bit, cleared.
fn rgb565(px: &[u8]) -> u16 {
    let (r, g, b) = (px.first().copied().unwrap_or(0), px.get(1).copied().unwrap_or(0), px.get(2).copied().unwrap_or(0));
    (u16::from(b >> 3) | u16::from(g >> 2) << 5 | u16::from(r >> 3) << 11) & !PREVIEW_REPEAT
}

/// A preview's pixels, run-length encoded.
fn encode_preview(p: &Preview) -> Result<Vec<u8>, IoError> {
    if p.rgb.len() as u64 != u64::from(p.width) * u64::from(p.height) * 3 {
        return Err(IoError::Format(format!("a {} × {} preview with {} bytes of pixels", p.width, p.height, p.rgb.len())));
    }
    let mut out = Vec::new();
    let mut push = |c: u16, n: u32| {
        if n <= 2 {
            (0..n).for_each(|_| out.extend_from_slice(&c.to_le_bytes()));
        } else {
            out.extend_from_slice(&(c | PREVIEW_REPEAT).to_le_bytes());
            out.extend_from_slice(&((n - 1) as u16 | 0x3000).to_le_bytes());
        }
    };
    let mut run: Option<(u16, u32)> = None;
    for px in p.rgb.chunks_exact(3) {
        let c = rgb565(px);
        match &mut run {
            Some((rc, n)) if *rc == c && *n < MAX_PREVIEW_RUN => *n += 1,
            _ => {
                if let Some((rc, n)) = run {
                    push(rc, n);
                }
                run = Some((c, 1));
            }
        }
    }
    if let Some((rc, n)) = run {
        push(rc, n);
    }
    Ok(out)
}

/// A preview's pixels back to RGB, 3 bits short in each channel. Fewer pixels than its size
/// leave the rest black.
fn decode_preview(data: &[u8], width: u32, height: u32) -> Result<Preview, IoError> {
    let total = width as usize * height as usize;
    let mut rgb = Vec::with_capacity(total * 3);
    let mut words = data.chunks_exact(2).map(|w| u16::from_le_bytes([w[0], w[1]]));
    if !data.len().is_multiple_of(2) {
        return Err(IoError::Format("a preview's data has an odd length".into()));
    }
    while let Some(c) = words.next() {
        let n = if c & PREVIEW_REPEAT == 0 {
            1
        } else {
            1 + usize::from(words.next().ok_or_else(|| IoError::Format("a preview's run is cut short".into()))? & 0x0FFF)
        };
        if rgb.len() / 3 + n > total {
            return Err(IoError::Format("a preview has more pixels than its size".into()));
        }
        let px = [(((c >> 11) & 0x1F) << 3) as u8, (((c >> 6) & 0x1F) << 3) as u8, ((c & 0x1F) << 3) as u8];
        (0..n).for_each(|_| rgb.extend_from_slice(&px));
    }
    rgb.resize(total * 3, 0);
    Ok(Preview { width, height, rgb })
}

fn write_preview(out: &mut Vec<u8>, p: &Preview, data: &[u8]) {
    let data_at = (out.len() + PREVIEW_HEADER_SIZE) as u32;
    for v in [p.width, p.height, data_at, data.len() as u32, 0, 0, 0, 0] {
        put_u32(out, v);
    }
    out.extend_from_slice(data);
}

/// The preview whose header is at `offset`, none at offset 0.
fn read_preview(bytes: &[u8], offset: u32) -> Result<Option<Preview>, IoError> {
    if offset == 0 {
        return Ok(None);
    }
    let mut r = Reader::at(bytes, offset.into(), PREVIEW_HEADER_SIZE)?;
    let (width, height, data_at, len) = (r.u32()?, r.u32()?, r.u32()?, r.u32()?);
    if u64::from(width) * u64::from(height) > MAX_PREVIEW_PIXELS {
        return Err(IoError::TooLarge(format!("a {width} × {height} preview")));
    }
    decode_preview(section(bytes, data_at.into(), len.into())?, width, height).map(Some)
}

fn put_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn put_f32(out: &mut Vec<u8>, v: f32) {
    out.extend_from_slice(&v.to_le_bytes());
}

/// `len` bytes at `offset`, or an error if they are not all inside `bytes`.
fn section(bytes: &[u8], offset: u64, len: u64) -> Result<&[u8], IoError> {
    let end = offset.checked_add(len).ok_or_else(|| IoError::Format("an offset overflows".into()))?;
    let (from, to) = (usize::try_from(offset), usize::try_from(end));
    match (from, to) {
        (Ok(from), Ok(to)) => bytes.get(from..to).ok_or_else(|| IoError::Format(format!("{len} bytes at {offset} are past the file's end"))),
        _ => Err(IoError::Format("an offset is past the file's end".into())),
    }
}

/// Little-endian fields read in turn from a section already known to be in the file.
struct Reader<'a> {
    data: &'a [u8],
}

impl<'a> Reader<'a> {
    fn at(bytes: &'a [u8], offset: u64, len: usize) -> Result<Self, IoError> {
        Ok(Reader { data: section(bytes, offset, len as u64)? })
    }

    fn take<const N: usize>(&mut self) -> Result<[u8; N], IoError> {
        let (head, rest) = self.data.split_first_chunk::<N>().ok_or_else(|| IoError::Format("a section is cut short".into()))?;
        self.data = rest;
        Ok(*head)
    }

    fn u8(&mut self) -> Result<u8, IoError> {
        Ok(self.take::<1>()?[0])
    }

    fn u16(&mut self) -> Result<u16, IoError> {
        self.take().map(u16::from_le_bytes)
    }

    fn u32(&mut self) -> Result<u32, IoError> {
        self.take().map(u32::from_le_bytes)
    }

    fn f32(&mut self) -> Result<f32, IoError> {
        self.take().map(f32::from_le_bytes)
    }

    fn skip(&mut self, n: usize) -> Result<(), IoError> {
        self.data = self.data.get(n..).ok_or_else(|| IoError::Format("a section is cut short".into()))?;
        Ok(())
    }
}

impl Header {
    fn write(&self, out: &mut Vec<u8>) {
        put_u32(out, self.magic);
        put_u32(out, self.version);
        self.bed.iter().for_each(|&v| put_f32(out, v));
        put_u32(out, 0);
        put_u32(out, 0);
        for v in [self.total_height, self.layer_height, self.exposure, self.bottom_exposure, self.light_off_delay] {
            put_f32(out, v);
        }
        for v in [
            self.bottom_layers,
            self.resolution_x,
            self.resolution_y,
            self.large_preview_offset,
            self.layer_table_offset,
            self.layer_count,
            self.small_preview_offset,
            self.print_time,
            self.projector_type,
            self.print_params_offset,
            self.print_params_size,
            self.aa_level,
        ] {
            put_u32(out, v);
        }
        out.extend_from_slice(&self.light_pwm.to_le_bytes());
        out.extend_from_slice(&self.bottom_light_pwm.to_le_bytes());
        for v in [self.encryption_key, self.slicer_info_offset, self.slicer_info_size] {
            put_u32(out, v);
        }
    }

    fn read(r: &mut Reader) -> Result<Self, IoError> {
        let (magic, version) = (r.u32()?, r.u32()?);
        let bed = [r.f32()?, r.f32()?, r.f32()?];
        r.skip(8)?;
        Ok(Header {
            magic,
            version,
            bed,
            total_height: r.f32()?,
            layer_height: r.f32()?,
            exposure: r.f32()?,
            bottom_exposure: r.f32()?,
            light_off_delay: r.f32()?,
            bottom_layers: r.u32()?,
            resolution_x: r.u32()?,
            resolution_y: r.u32()?,
            large_preview_offset: r.u32()?,
            layer_table_offset: r.u32()?,
            layer_count: r.u32()?,
            small_preview_offset: r.u32()?,
            print_time: r.u32()?,
            projector_type: r.u32()?,
            print_params_offset: r.u32()?,
            print_params_size: r.u32()?,
            aa_level: r.u32()?,
            light_pwm: r.u16()?,
            bottom_light_pwm: r.u16()?,
            encryption_key: r.u32()?,
            slicer_info_offset: r.u32()?,
            slicer_info_size: r.u32()?,
        })
    }
}

impl PrintParams {
    fn write(&self, out: &mut Vec<u8>) {
        for v in [
            self.bottom_lift_height,
            self.bottom_lift_speed,
            self.lift_height,
            self.lift_speed,
            self.retract_speed,
            self.volume_ml,
            self.weight_g,
            self.cost,
            self.bottom_light_off_delay,
            self.light_off_delay,
        ] {
            put_f32(out, v);
        }
        for v in [self.bottom_layer_count, 0, 0, 0, self.marker] {
            put_u32(out, v);
        }
    }

    fn read(r: &mut Reader) -> Result<Self, IoError> {
        let p = PrintParams {
            bottom_lift_height: r.f32()?,
            bottom_lift_speed: r.f32()?,
            lift_height: r.f32()?,
            lift_speed: r.f32()?,
            retract_speed: r.f32()?,
            volume_ml: r.f32()?,
            weight_g: r.f32()?,
            cost: r.f32()?,
            bottom_light_off_delay: r.f32()?,
            light_off_delay: r.f32()?,
            bottom_layer_count: r.u32()?,
            marker: 0,
        };
        r.skip(12)?;
        Ok(PrintParams { marker: r.u32()?, ..p })
    }
}

impl SlicerInfo {
    fn write(&self, out: &mut Vec<u8>) {
        (0..7).for_each(|_| put_f32(out, 0.0));
        put_u32(out, self.machine_name_offset);
        put_u32(out, self.machine_name_size);
        out.push(self.aa_flag);
        out.extend_from_slice(&[0, 0]);
        out.push(self.per_layer_settings);
        for v in [self.modified_minutes, self.aa_level, self.software_version] {
            put_u32(out, v);
        }
        put_f32(out, 0.0);
        put_f32(out, 0.0);
        for v in [self.transition_layers, 0, 0, 0] {
            put_u32(out, v);
        }
    }

    fn read(r: &mut Reader) -> Result<Self, IoError> {
        r.skip(28)?;
        let (machine_name_offset, machine_name_size, aa_flag) = (r.u32()?, r.u32()?, r.u8()?);
        r.skip(2)?;
        let per_layer_settings = r.u8()?;
        let (modified_minutes, aa_level, software_version) = (r.u32()?, r.u32()?, r.u32()?);
        r.skip(8)?;
        Ok(SlicerInfo {
            machine_name_offset,
            machine_name_size,
            aa_flag,
            per_layer_settings,
            modified_minutes,
            aa_level,
            software_version,
            transition_layers: r.u32()?,
        })
    }
}

impl LayerDef {
    fn write(&self, out: &mut Vec<u8>) {
        for v in [self.z, self.exposure, self.light_off] {
            put_f32(out, v);
        }
        for v in [self.data_offset, self.data_size, self.page, self.table_size, 0, 0] {
            put_u32(out, v);
        }
    }

    fn read(r: &mut Reader) -> Result<Self, IoError> {
        let d = LayerDef {
            z: r.f32()?,
            exposure: r.f32()?,
            light_off: r.f32()?,
            data_offset: r.u32()?,
            data_size: r.u32()?,
            page: r.u32()?,
            table_size: r.u32()?,
        };
        r.skip(8)?;
        Ok(d)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use resin_slice::{JobStats, Layer, PrintSettings, PrinterProfile, Stamp};

    /// A small deterministic generator, so the random tests repeat.
    struct XorShift(u64);

    impl XorShift {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
    }

    /// The runs a reader gets back: greys cut to 7 bits, neighbours of one grey merged.
    fn normalised(runs: &[Run]) -> Vec<Run> {
        let mut out: Vec<Run> = Vec::new();
        for r in runs.iter().filter(|r| r.len > 0) {
            let v = r.grey >> 1;
            let grey = if v == 0 { 0 } else { (v << 1) | 1 };
            match out.last_mut() {
                Some(o) if o.grey == grey => o.len += r.len,
                _ => out.push(Run { len: r.len, grey }),
            }
        }
        out
    }

    fn push(runs: &mut Vec<Run>, len: u64, grey: u8) {
        match runs.last_mut() {
            Some(r) if r.grey == grey => r.len += len,
            _ if len > 0 => runs.push(Run { len, grey }),
            _ => {}
        }
    }

    /// The runs lighting every pixel whose centre is inside one of `rects`, `[x0, y0, x1, y1]` on
    /// the plate in mm, placed as the rasterizer places them.
    fn rect_runs(p: &PrinterProfile, rects: &[[f64; 4]]) -> Vec<Run> {
        let ([sx, sy], [ox, oy]) = p.image_map(1);
        let (w, h) = (p.resolution_x, p.resolution_y);
        let range = |a: f64, b: f64, max: u32| {
            let (lo, hi) = (a.min(b), a.max(b));
            ((lo - 0.5).ceil().clamp(0.0, f64::from(max)) as u32, (hi - 0.5).ceil().clamp(0.0, f64::from(max)) as u32)
        };
        let boxes: Vec<_> = rects.iter().map(|r| (range(r[0] * sx + ox, r[2] * sx + ox, w), range(r[1] * sy + oy, r[3] * sy + oy, h))).collect();
        let mut runs = Vec::new();
        for row in 0..h {
            let mut spans: Vec<(u32, u32)> = boxes.iter().filter(|(_, (r0, r1))| (*r0..*r1).contains(&row)).map(|(c, _)| *c).collect();
            spans.sort();
            let mut at = 0;
            for (a, b) in spans {
                let a = a.max(at);
                if b > a {
                    push(&mut runs, u64::from(a - at), 0);
                    push(&mut runs, u64::from(b - a), 255);
                    at = b;
                }
            }
            push(&mut runs, u64::from(w - at), 0);
        }
        runs
    }

    /// `count` identical layers of `runs`, exposed and timed as the slicer would.
    fn job_of(profile: PrinterProfile, runs: Vec<Run>, count: u32) -> SlicedJob {
        let settings = PrintSettings::default();
        let lit: u64 = runs.iter().filter(|r| r.grey > 0).map(|r| r.len).sum();
        let h = settings.layers.height;
        let (px, py) = profile.pixel_pitch();
        let layers: Vec<Layer> = (0..count)
            .map(|i| Layer {
                runs: runs.clone(),
                z: ((f64::from(i) + 1.0) * h) as f32,
                exposure: settings.exposure_for(i),
                light_off: settings.layer_motion(i).light_off as f32,
                lit,
            })
            .collect();
        let stats = JobStats {
            volume_ml: f64::from(count) * lit as f64 * px * py * h / 1000.0,
            print_time_s: (0..count).map(|i| settings.layer_motion(i).seconds()).sum(),
            ..JobStats::default()
        };
        SlicedJob { layers, profile, settings, stats, stamp: Stamp::default() }
    }

    fn cube() -> SlicedJob {
        let p = PrinterProfile::default();
        let runs = rect_runs(&p, &[[-5.0, -5.0, 5.0, 5.0]]);
        job_of(p, runs, 200)
    }

    /// A 20 mm F with 2 mm strokes, 5 mm in from the plate's +X +Y corner.
    fn letter_f() -> SlicedJob {
        let p = PrinterProfile::default();
        let (x0, y0) = (p.display_x / 2.0 - 5.0 - 12.0, p.display_y / 2.0 - 5.0 - 20.0);
        let runs = rect_runs(&p, &[[x0, y0, x0 + 2.0, y0 + 20.0], [x0, y0 + 18.0, x0 + 12.0, y0 + 20.0], [x0, y0 + 9.0, x0 + 9.0, y0 + 11.0]]);
        job_of(p, runs, 40)
    }

    /// A tiny job with greys of every kind, for the tests that read a whole file many times.
    fn tiny() -> SlicedJob {
        let p = PrinterProfile { resolution_x: 16, resolution_y: 8, display_x: 16.0, display_y: 8.0, ..PrinterProfile::default() };
        let mut job = job_of(p, vec![Run { len: 40, grey: 0 }, Run { len: 3, grey: 255 }, Run { len: 1, grey: 128 }, Run { len: 84, grey: 0 }], 3);
        job.layers[1].runs = vec![Run { len: 1, grey: 7 }, Run { len: 126, grey: 200 }, Run { len: 1, grey: 1 }];
        job
    }

    fn tiny_previews() -> Previews {
        let image = |w: u32, h: u32| Preview { width: w, height: h, rgb: (0..w * h * 3).map(|i| (i * 37) as u8).collect() };
        Previews { large: image(4, 3), small: image(2, 2) }
    }

    fn u32_at(bytes: &[u8], at: usize) -> u32 {
        u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
    }

    #[test]
    fn sections_have_their_sizes() {
        let mut v = Vec::new();
        Header::default().write(&mut v);
        assert_eq!(v.len(), HEADER_SIZE);
        v.clear();
        write_preview(&mut v, &Preview::default(), &[]);
        assert_eq!(v.len(), PREVIEW_HEADER_SIZE);
        v.clear();
        PrintParams::default().write(&mut v);
        assert_eq!(v.len(), PRINT_PARAMS_SIZE);
        v.clear();
        SlicerInfo::default().write(&mut v);
        assert_eq!(v.len(), SLICER_INFO_SIZE);
        v.clear();
        LayerDef::default().write(&mut v);
        assert_eq!(v.len(), LAYER_DEF_SIZE);
    }

    #[test]
    fn a_job_reads_back() {
        let job = cube();
        let opts = CtbOptions { encryption_key: DEFAULT_KEY, modified_minutes: 29_000_000 };
        let bytes = write_ctb(&job, &Previews::plain(&job), &opts).unwrap();
        let f = read_ctb(&bytes).unwrap();
        let (p, s) = (&job.profile, &job.settings);
        let h = &f.header;
        assert_eq!((h.magic, h.version, h.resolution_x, h.resolution_y), (MAGIC, 2, 7680, 4320));
        assert_eq!(h.bed, [p.display_x as f32, p.display_y as f32, p.build_height as f32]);
        assert_eq!(h.total_height, job.layers[199].z);
        assert_eq!(h.layer_height, s.layers.height as f32);
        assert_eq!((h.exposure, h.bottom_exposure, h.light_off_delay), (s.exposure.normal as f32, s.exposure.bottom as f32, 1.0));
        assert_eq!((h.bottom_layers, h.layer_count, h.aa_level, h.projector_type), (s.layers.bottom_count, 200, 1, 0));
        assert_eq!((h.light_pwm, h.bottom_light_pwm, h.encryption_key), (255, 255, DEFAULT_KEY));
        assert_eq!(h.print_time, job.stats.print_time_s.ceil() as u32);
        assert_eq!((h.print_params_size, h.slicer_info_size), (60, 76));
        assert_eq!(f.params.lift_height, s.motion.lift_height as f32);
        assert_eq!(f.params.retract_speed, s.motion.retract_speed as f32);
        assert_eq!(f.params.volume_ml, job.stats.volume_ml as f32);
        assert_eq!((f.params.bottom_layer_count, f.params.marker), (s.layers.bottom_count, 0x1234));
        assert_eq!(f.slicer.machine_name_offset, h.slicer_info_offset + 76);
        assert_eq!(f.machine_name, p.name);
        assert_eq!((f.slicer.aa_flag, f.slicer.aa_level, f.slicer.per_layer_settings), (0x07, 1, 0));
        assert_eq!((f.slicer.modified_minutes, f.slicer.software_version), (29_000_000, 0x0109_0000));
        assert_eq!(f.slicer.transition_layers, s.layers.transition_count);
        assert_eq!(f.large_preview.as_ref().map(|p| (p.width, p.height)), Some(LARGE_PREVIEW));
        assert_eq!(f.small_preview.as_ref().map(|p| (p.width, p.height)), Some(SMALL_PREVIEW));
        assert_eq!(f.layers.len(), job.layers.len());
        for (got, want) in f.layers.iter().zip(&job.layers) {
            assert_eq!((got.z, got.exposure, got.light_off), (want.z, want.exposure, want.light_off));
            assert_eq!(got.runs, normalised(&want.runs));
        }
        assert!(f.layers[0].exposure > f.layers[199].exposure, "bottom layers are exposed longer");
    }

    #[test]
    fn sections_follow_one_another() {
        let job = tiny();
        let bytes = write_ctb(&job, &tiny_previews(), &CtbOptions::default()).unwrap();
        let h = read_ctb(&bytes).unwrap().header;
        assert_eq!(h.large_preview_offset, 112);
        assert_eq!(h.small_preview_offset, 112 + 32 + u32_at(&bytes, 112 + 12));
        let small_len = u32_at(&bytes, h.small_preview_offset as usize + 12);
        assert_eq!(h.print_params_offset, h.small_preview_offset + 32 + small_len);
        assert_eq!(h.slicer_info_offset, h.print_params_offset + 60);
        assert_eq!(h.layer_table_offset, h.slicer_info_offset + 76 + job.profile.name.len() as u32);
        assert_eq!(u32_at(&bytes, h.layer_table_offset as usize + 12), h.layer_table_offset + 3 * 36, "the first layer's data follows the table");
    }

    #[test]
    fn mirroring_x_is_the_projector_type() {
        let mut job = tiny();
        job.profile.mirror_x = true;
        let bytes = write_ctb(&job, &tiny_previews(), &CtbOptions::default()).unwrap();
        assert_eq!(read_ctb(&bytes).unwrap().header.projector_type, 1);
    }

    #[test]
    fn anti_aliasing_goes_in_the_slicer_info() {
        let mut job = tiny();
        job.settings.slicing.anti_aliasing = 4;
        let f = read_ctb(&write_ctb(&job, &tiny_previews(), &CtbOptions::default()).unwrap()).unwrap();
        assert_eq!((f.header.aa_level, f.slicer.aa_level, f.slicer.aa_flag), (1, 4, 0x0F));
    }

    #[test]
    fn rle_round_trips_every_length_class() {
        let lengths = [1u64, 2, 0x7F, 0x80, 0x3FFF, 0x4000, 0x1F_FFFF, 0x20_0000, 0xFFF_FFFF, 0xFFF_FFFF + 5, 3 * 0xFFF_FFFF + 1];
        let runs: Vec<Run> = lengths.iter().enumerate().map(|(i, &len)| Run { len, grey: if i % 2 == 0 { 255 } else { 0 } }).collect();
        let pixels = lengths.iter().sum();
        let data = encode_layer(&runs, pixels).unwrap();
        assert_eq!(decode_layer(&data, pixels).unwrap(), normalised(&runs));
        // Lengths at a class's top take that class's bytes.
        for (len, bytes) in [(1u64, 1usize), (2, 2), (0x7F, 2), (0x80, 3), (0x3FFF, 3), (0x4000, 4), (0x1F_FFFF, 4), (0x20_0000, 5), (0xFFF_FFFF, 5)]
        {
            assert_eq!(encode_layer(&[Run { len, grey: 255 }], len).unwrap().len(), bytes, "length {len:#x}");
        }
        assert_eq!(
            encode_layer(&[Run { len: 0xFFF_FFFF + 1, grey: 255 }], 0xFFF_FFFF + 1).unwrap().len(),
            6,
            "split into a full record and one pixel"
        );
    }

    #[test]
    fn rle_round_trips_random_runs() {
        let mut rng = XorShift(0x9E37_79B9_7F4A_7C15);
        for _ in 0..200 {
            let runs: Vec<Run> = (0..rng.next() % 300)
                .map(|_| {
                    let len = match rng.next() % 4 {
                        0 => 1 + rng.next() % 3,
                        1 => 1 + rng.next() % 0x200,
                        2 => 1 + rng.next() % 0x5_0000,
                        _ => rng.next() % 0x30_0000,
                    };
                    Run { len, grey: rng.next() as u8 }
                })
                .collect();
            let pixels = runs.iter().map(|r| r.len).sum();
            let data = encode_layer(&runs, pixels).unwrap();
            assert_eq!(decode_layer(&data, pixels).unwrap(), normalised(&runs));
        }
    }

    #[test]
    fn rle_needs_the_runs_to_cover_the_screen() {
        let runs = [Run { len: 10, grey: 255 }];
        assert!(encode_layer(&runs, 11).is_err());
        assert!(encode_layer(&runs, 9).is_err());
        assert!(encode_layer(&[Run { len: u64::MAX, grey: 0 }, Run { len: 2, grey: 0 }], 1).is_err());
        let data = encode_layer(&runs, 10).unwrap();
        assert!(decode_layer(&data, 9).is_err() && decode_layer(&data, 11).is_err());
        let job = SlicedJob { layers: vec![Layer { runs: runs.to_vec(), ..Layer::default() }], ..tiny() };
        assert!(matches!(write_ctb(&job, &tiny_previews(), &CtbOptions::default()), Err(IoError::Format(_))));
    }

    #[test]
    fn cipher_is_its_own_inverse() {
        let plain: Vec<u8> = (0..1001u32).map(|i| (i * 7) as u8).collect();
        for key in [0, 1, DEFAULT_KEY, u32::MAX] {
            for layer in [0, 1, 199, u32::MAX] {
                let mut data = plain.clone();
                crypt(&mut data, key, layer);
                assert_eq!(data == plain, key == 0, "key {key:#x} layer {layer}");
                crypt(&mut data, key, layer);
                assert_eq!(data, plain);
            }
        }
    }

    #[test]
    fn mslicer_cipher_agrees() {
        // mslicer's `xor_cypher`, byte by byte, as the reference for the chunked version.
        let reference = |data: &mut [u8], seed: u32, layer: u32| {
            let init = seed.wrapping_mul(0x2D83CDAC).wrapping_add(0xD8A83423);
            let mut key = layer.wrapping_mul(0x1E1530CD).wrapping_add(0xEC3D47CD).wrapping_mul(init);
            let mut index = 0;
            for byte in data.iter_mut() {
                let k = (key >> (8 * index)) as u8;
                index += 1;
                if index & 3 == 0 {
                    key = key.wrapping_add(init);
                    index = 0;
                }
                *byte ^= k;
            }
        };
        let plain: Vec<u8> = (0..103u32).map(|i| (i * 13) as u8).collect();
        let (mut a, mut b) = (plain.clone(), plain);
        crypt(&mut a, DEFAULT_KEY, 5);
        reference(&mut b, DEFAULT_KEY, 5);
        assert_eq!(a, b);
    }

    #[test]
    fn key_0_writes_the_rle_as_it_is() {
        let job = tiny();
        let pixels = 16 * 8;
        for key in [0, DEFAULT_KEY] {
            let bytes = write_ctb(&job, &tiny_previews(), &CtbOptions { encryption_key: key, modified_minutes: 0 }).unwrap();
            let table = read_ctb(&bytes).unwrap().header.layer_table_offset as usize;
            let entry = table + LAYER_DEF_SIZE;
            let (at, len) = (u32_at(&bytes, entry + 12) as usize, u32_at(&bytes, entry + 16) as usize);
            let raw = encode_layer(&job.layers[1].runs, pixels).unwrap();
            assert_eq!(bytes[at..at + len] == raw[..], key == 0, "key {key:#x}");
        }
    }

    #[test]
    fn previews_round_trip() {
        let quantised = |rgb: &[u8]| rgb.iter().map(|&c| c & 0xF8).collect::<Vec<u8>>();
        let (w, h) = LARGE_PREVIEW;
        let gradient = Preview { width: w, height: h, rgb: (0..w * h).flat_map(|i| [(i % w) as u8, (i / w) as u8, (i % 7 * 40) as u8]).collect() };
        let data = encode_preview(&gradient).unwrap();
        assert_eq!(decode_preview(&data, w, h).unwrap().rgb, quantised(&gradient.rgb));

        let flat = Preview { width: w, height: h, rgb: [10, 200, 30].repeat((w * h) as usize) };
        let data = encode_preview(&flat).unwrap();
        let runs = (w * h).div_ceil(0xFFF) as usize;
        assert_eq!(data.len(), runs * 4, "runs split at 0xFFF pixels, each a colour and a length");
        assert_eq!(decode_preview(&data, w, h).unwrap().rgb, quantised(&flat.rgb));

        let short = Preview { width: 3, height: 1, rgb: vec![0; 8] };
        assert!(encode_preview(&short).is_err());
    }

    #[test]
    fn the_plain_preview_shows_the_shape() {
        let job = cube();
        let p = Previews::plain(&job);
        let at = |img: &Preview, x: u32, y: u32| img.rgb[((y * img.width + x) * 3) as usize];
        for img in [&p.large, &p.small] {
            assert_eq!(img.rgb.len() as u32, img.width * img.height * 3);
            assert_eq!(at(img, img.width / 2, img.height / 2), 0xFF, "the centred cube is white");
            assert_eq!(at(img, 0, 0), 0x30, "the corners are dark grey");
        }
    }

    #[test]
    fn the_f_sits_at_the_plus_x_plus_y_corner_upright() {
        let job = letter_f();
        let w = u64::from(job.profile.resolution_x);
        let (mut rows, mut cols) = ((u64::MAX, 0), (u64::MAX, 0));
        let mut at = 0;
        let mut top_row_len = 0;
        for r in &job.layers[0].runs {
            if r.grey > 0 {
                let (row, col) = (at / w, at % w);
                if row < rows.0 {
                    top_row_len = r.len;
                }
                rows = (rows.0.min(row), rows.1.max(row));
                cols = (cols.0.min(col), cols.1.max(col + r.len - 1));
            }
            at += r.len;
        }
        let (h, w) = (u64::from(job.profile.resolution_y), w);
        assert!(cols.0 > w / 2 && rows.1 < h / 2, "+X is right and +Y is row 0: the F is in the image's top right");
        assert!(top_row_len > 5 * (cols.1 - cols.0) / 6, "the top bar is the first lit row: the F is upright");
    }

    #[test]
    fn progress_counts_layers_and_cancel_stops() {
        let job = tiny();
        let (progress, cancel) = (Progress::default(), Cancel::default());
        write_ctb_with(&job, &tiny_previews(), &CtbOptions::default(), &progress, &cancel).unwrap();
        assert_eq!(progress.get(), (3, 3));
        cancel.cancel();
        assert!(matches!(write_ctb_with(&job, &tiny_previews(), &CtbOptions::default(), &progress, &cancel), Err(IoError::Cancelled)));
    }

    #[test]
    fn an_empty_job_is_refused() {
        let job = SlicedJob { layers: Vec::new(), ..tiny() };
        assert!(write_ctb(&job, &tiny_previews(), &CtbOptions::default()).is_err());
    }

    #[test]
    fn hostile_files_are_errors_not_panics() {
        let good = write_ctb(&tiny(), &tiny_previews(), &CtbOptions::default()).unwrap();
        assert!(read_ctb(&good).is_ok());
        for len in 0..good.len() {
            assert!(read_ctb(&good[..len]).is_err(), "cut to {len} bytes");
        }

        let mut rng = XorShift(0x2545_F491_4F6C_DD1D);
        for _ in 0..2000 {
            let noise: Vec<u8> = (0..rng.next() % 600).map(|_| rng.next() as u8).collect();
            let _ = read_ctb(&noise);
            let mut with_header = good[..8].to_vec();
            with_header.extend_from_slice(&noise);
            let _ = read_ctb(&with_header);
            let mut flipped = good.clone();
            for _ in 0..1 + rng.next() % 4 {
                let at = (rng.next() % flipped.len() as u64) as usize;
                flipped[at] ^= 1 << (rng.next() % 8);
            }
            let _ = read_ctb(&flipped);
        }

        let table = read_ctb(&good).unwrap().header.layer_table_offset as usize;
        // Offsets and counts past the file's end are errors; sizes of fixed sections are only
        // checked to be large enough.
        let offsets = [60usize, 64, 68, 72, 84, 104, table + 12, table + 16, table + 20, table + 36 + 12];
        let sizes = [88usize, 108];
        let len = good.len() as u32;
        for at in offsets.into_iter().chain(sizes) {
            for v in [0, 1, len - 1, len, len + 1, 0x7FFF_FFFF, u32::MAX] {
                let mut bad = good.clone();
                bad[at..at + 4].copy_from_slice(&v.to_le_bytes());
                let read = read_ctb(&bad);
                if v >= len && offsets.contains(&at) {
                    assert!(read.is_err(), "{v:#x} at {at}");
                }
            }
        }
        let mut huge = good.clone();
        huge[52..56].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(read_ctb(&huge).is_err(), "a resolution over the cap");
        // Unciphered, every entry would decode one block again and again.
        let checker = PrinterProfile { resolution_x: 256, resolution_y: 256, display_x: 25.6, display_y: 25.6, ..PrinterProfile::default() };
        let one_pixel_runs = (0..256 * 256).map(|i| Run { len: 1, grey: if i % 2 == 0 { 0 } else { 255 } }).collect();
        let mut repeated =
            write_ctb(&job_of(checker, one_pixel_runs, 400), &tiny_previews(), &CtbOptions { encryption_key: 0, modified_minutes: 0 }).unwrap();
        let table = read_ctb(&repeated).unwrap().header.layer_table_offset as usize;
        let block = repeated[table + 12..table + 20].to_vec();
        let data_len = u32_at(&repeated, table + 16) as usize;
        for i in 1..400 {
            let at = table + i * LAYER_DEF_SIZE + 12;
            repeated[at..at + 8].copy_from_slice(&block);
        }
        let mut grown = repeated[..repeated.len() - 399 * data_len].to_vec();
        grown.resize(grown.len() + 2 * data_len, 0);
        assert!(read_ctb(&grown).is_err(), "400 entries on one block in a file far smaller than their sum");
        let mut v4 = good;
        v4[4..8].copy_from_slice(&4u32.to_le_bytes());
        assert!(read_ctb(&v4).is_err(), "only v2 is read");
    }

    /// The files for the printer test (`docs/prompts/slicing/04-export-ctb.md`): run by hand with
    /// `CTB_OUT_DIR=<dir> cargo test -p resin-io ctb_printer_files -- --ignored`.
    #[test]
    #[ignore]
    fn ctb_printer_files() {
        let dir = std::path::PathBuf::from(std::env::var("CTB_OUT_DIR").expect("set CTB_OUT_DIR to the directory to write to"));
        let cube = cube();
        let f = letter_f();
        for (name, job, key) in [("cube-k67.ctb", &cube, DEFAULT_KEY), ("cube-k0.ctb", &cube, 0), ("f-k67.ctb", &f, DEFAULT_KEY)] {
            let bytes = write_ctb(job, &Previews::plain(job), &CtbOptions { encryption_key: key, modified_minutes: 29_000_000 }).unwrap();
            let back = read_ctb(&bytes).unwrap();
            assert_eq!(back.layers.len(), job.layers.len());
            crate::write_atomic(&dir.join(name), &bytes, false).unwrap();
        }
    }
}
