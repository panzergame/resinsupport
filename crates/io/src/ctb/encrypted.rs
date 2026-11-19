// The encrypted CTB layout (header, settings, signature, layer pointers and definitions, resin
// parameters), the settings and signature cipher with its key and IV, and the disclaimer are
// ported from mslicer (GPL-3.0), `format/ctb_format/src/v5/mod.rs`, `v5/file.rs`
// (`File::serialize`, `deserialize`), `v5/layer.rs`, `v5/resin.rs` and `shared/mod.rs`
// (`DISCLAIMER`) at 355aca2 (https://github.com/connorslade/mslicer), which credits UVtools for
// them; see ATTRIBUTION.md. The two-phase fields are filled from UVtools' model of them
// (`UVtools.Core/FileFormats/FileFormat.cs` at 5fdcc53), from its field definitions with no
// UVtools code.

//! Chitubox's encrypted CTB, version 5: what an Elegoo Saturn 2 reads.
//!
//! A 48-byte header points at the settings, 288 bytes encrypted with AES-256-CBC under a key
//! every such file shares, and at a signature, the SHA-256 of the settings' checksum, encrypted
//! the same way. Then come the machine name, the disclaimer, the resin parameters, both previews,
//! the layer pointers and the signature between two markers, then each layer: its definition,
//! which carries its own lift and retract, and its data. The layer data is v2's (the RLE and the
//! XOR key stream, the key in the settings); a layer may also have a span of it AES-encrypted,
//! which this writer never uses and the reader undoes.

use aes::Aes256;
use aes::cipher::block_padding::NoPadding;
use aes::cipher::{BlockDecryptMut, BlockEncryptMut, KeyIvInit};
use resin_slice::{Cancel, CtbFormat, Progress, SlicedJob};
use sha2::{Digest, Sha256};

use super::{
    CtbFile, CtbLayer, CtbOptions, Header, MAX_LAYERS, MAX_NAME_BYTES, MAX_RESOLUTION, PAGE, Phases, PrintParams, Reader, SlicerInfo, aa_flag,
    aa_level, cost, crypt, decode_layer, encode_layer, encode_preview, put_f32, put_u32, read_preview, section, write_preview,
};
use crate::IoError;

pub const MAGIC: u32 = 0x12FD_0107;
pub const VERSION: u32 = 5;
pub const HEADER_SIZE: usize = 48;
pub const SETTINGS_SIZE: usize = 288;
pub const SIGNATURE_SIZE: usize = 32;
pub const LAYER_POINTER_SIZE: usize = 16;
pub const LAYER_DEF_SIZE: usize = 0x58;
pub const RESIN_SIZE: usize = 40;

const KEY: [u8; 32] = [
    0xD0, 0x5B, 0x8E, 0x33, 0x71, 0xDE, 0x3D, 0x1A, 0xE5, 0x4F, 0x22, 0xDD, 0xDF, 0x5B, 0xFD, 0x94, 0xAB, 0x5D, 0x64, 0x3A, 0x9D, 0x7E, 0xBF, 0xAF,
    0x42, 0x03, 0xF3, 0x10, 0xD8, 0x52, 0x2A, 0xEA,
];
const IV: [u8; 16] = [0x0F, 0x01, 0x0A, 0x05, 0x05, 0x0B, 0x06, 0x07, 0x08, 0x06, 0x0A, 0x0C, 0x0C, 0x0D, 0x09, 0x0F];
/// mslicer's: the signature only has to match it.
const CHECKSUM: u64 = 0;
const BEFORE_SIGNATURE: u32 = 0x4220_52FA;
const AFTER_SIGNATURE: u32 = 0x6D42_32B3;
/// The layers carry their own settings, which the printer follows (mslicer's value).
const PER_LAYER_SETTINGS: u8 = 0x40;
/// Readers find it where the settings point; mslicer writes it as Chitubox does.
const DISCLAIMER: &str = "Layout and record format for the ctb and cbddlp file types are the copyrighted programs or codes of CBD Technology (China) Inc..The Customer or User shall not in any manner reproduce, distribute, modify, decompile, disassemble, decrypt, extract, reverse engineer, lease, assign, or sublicense the said programs or codes.";
const RESIN_TYPE: &str = "Normal";
const RESIN_NAME: &str = "Standard";
const MAX_SETTINGS_SIZE: u32 = 4096;
/// Version 4 shares version 5's layout up to the resin parameters, which this reader ignores.
const READABLE_VERSIONS: [u32; 2] = [4, 5];

/// The settings, decrypted. Seconds, mm, mm/min; offsets from the file's start.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Settings {
    checksum: u64,
    layer_pointers_offset: u32,
    bed: [f32; 3],
    total_height: f32,
    layer_height: f32,
    exposure: f32,
    bottom_exposure: f32,
    light_off_delay: f32,
    bottom_layers: u32,
    resolution_x: u32,
    resolution_y: u32,
    layer_count: u32,
    large_preview_offset: u32,
    small_preview_offset: u32,
    print_time: u32,
    projector_type: u32,
    bottom: Phases,
    regular: Phases,
    volume_ml: f32,
    weight_g: f32,
    cost: f32,
    bottom_light_off_delay: f32,
    light_pwm: u16,
    bottom_light_pwm: u16,
    encryption_key: u32,
    machine_name_offset: u32,
    machine_name_size: u32,
    aa_flag: u8,
    per_layer_settings: u8,
    modified_minutes: u32,
    aa_level: u32,
    /// The wait before each exposure, after the retract.
    rest_after_retract: f32,
    transition_layers: u32,
    bottom_rest_after_retract: f32,
    last_layer_index: u32,
    disclaimer_offset: u32,
    disclaimer_size: u32,
    resin_offset: u32,
}

impl Settings {
    fn write(&self, out: &mut Vec<u8>) {
        let (b, r) = (&self.bottom, &self.regular);
        out.extend_from_slice(&self.checksum.to_le_bytes());
        put_u32(out, self.layer_pointers_offset);
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
            self.layer_count,
            self.large_preview_offset,
            self.small_preview_offset,
            self.print_time,
            self.projector_type,
        ] {
            put_u32(out, v);
        }
        for v in [b.lift_height, b.lift_speed, r.lift_height, r.lift_speed, r.retract_speed, self.volume_ml, self.weight_g, self.cost] {
            put_f32(out, v);
        }
        put_f32(out, self.bottom_light_off_delay);
        put_u32(out, 1);
        out.extend_from_slice(&self.light_pwm.to_le_bytes());
        out.extend_from_slice(&self.bottom_light_pwm.to_le_bytes());
        put_u32(out, self.encryption_key);
        for v in [b.lift_height_2, b.lift_speed_2, r.lift_height_2, r.lift_speed_2, r.retract_height_2, r.retract_speed_2] {
            put_f32(out, v);
        }
        // The wait after the lift.
        put_f32(out, 0.0);
        put_u32(out, self.machine_name_offset);
        put_u32(out, self.machine_name_size);
        out.push(self.aa_flag);
        out.extend_from_slice(&[0, 0]);
        out.push(self.per_layer_settings);
        put_u32(out, self.modified_minutes);
        put_u32(out, self.aa_level);
        put_f32(out, self.rest_after_retract);
        // The second wait after the lift.
        put_f32(out, 0.0);
        put_u32(out, self.transition_layers);
        put_f32(out, b.retract_speed);
        put_f32(out, b.retract_speed_2);
        put_u32(out, 0);
        put_f32(out, 4.0);
        put_u32(out, 0);
        put_f32(out, 4.0);
        put_f32(out, self.bottom_rest_after_retract);
        // The third wait after the lift, the wait before it.
        put_f32(out, 0.0);
        put_f32(out, 0.0);
        put_f32(out, b.retract_height_2);
        put_u32(out, 0);
        put_u32(out, 0);
        put_u32(out, 4);
        put_u32(out, self.last_layer_index);
        (0..4).for_each(|_| put_u32(out, 0));
        put_u32(out, self.disclaimer_offset);
        put_u32(out, self.disclaimer_size);
        put_u32(out, 0);
        put_u32(out, self.resin_offset);
        put_u32(out, 0);
        put_u32(out, 0);
    }

    fn read(r: &mut Reader) -> Result<Self, IoError> {
        let checksum = u64::from_le_bytes(r.take()?);
        let layer_pointers_offset = r.u32()?;
        let bed = [r.f32()?, r.f32()?, r.f32()?];
        r.skip(8)?;
        let (total_height, layer_height, exposure, bottom_exposure, light_off_delay) = (r.f32()?, r.f32()?, r.f32()?, r.f32()?, r.f32()?);
        let (bottom_layers, resolution_x, resolution_y, layer_count) = (r.u32()?, r.u32()?, r.u32()?, r.u32()?);
        let (large_preview_offset, small_preview_offset, print_time, projector_type) = (r.u32()?, r.u32()?, r.u32()?, r.u32()?);
        let mut bottom = Phases { lift_height: r.f32()?, lift_speed: r.f32()?, ..Phases::default() };
        let mut regular = Phases { lift_height: r.f32()?, lift_speed: r.f32()?, retract_speed: r.f32()?, ..Phases::default() };
        let (volume_ml, weight_g, cost, bottom_light_off_delay) = (r.f32()?, r.f32()?, r.f32()?, r.f32()?);
        r.skip(4)?;
        let (light_pwm, bottom_light_pwm, encryption_key) = (r.u16()?, r.u16()?, r.u32()?);
        (bottom.lift_height_2, bottom.lift_speed_2) = (r.f32()?, r.f32()?);
        (regular.lift_height_2, regular.lift_speed_2, regular.retract_height_2, regular.retract_speed_2) = (r.f32()?, r.f32()?, r.f32()?, r.f32()?);
        r.skip(4)?;
        let (machine_name_offset, machine_name_size, aa_flag) = (r.u32()?, r.u32()?, r.u8()?);
        r.skip(2)?;
        let (per_layer_settings, modified_minutes, aa_level, rest_after_retract) = (r.u8()?, r.u32()?, r.u32()?, r.f32()?);
        r.skip(4)?;
        let transition_layers = r.u32()?;
        (bottom.retract_speed, bottom.retract_speed_2) = (r.f32()?, r.f32()?);
        r.skip(16)?;
        let bottom_rest_after_retract = r.f32()?;
        r.skip(8)?;
        bottom.retract_height_2 = r.f32()?;
        r.skip(12)?;
        let last_layer_index = r.u32()?;
        r.skip(16)?;
        let (disclaimer_offset, disclaimer_size) = (r.u32()?, r.u32()?);
        r.skip(4)?;
        let resin_offset = r.u32()?;
        Ok(Settings {
            checksum,
            layer_pointers_offset,
            bed,
            total_height,
            layer_height,
            exposure,
            bottom_exposure,
            light_off_delay,
            bottom_layers,
            resolution_x,
            resolution_y,
            layer_count,
            large_preview_offset,
            small_preview_offset,
            print_time,
            projector_type,
            bottom,
            regular,
            volume_ml,
            weight_g,
            cost,
            bottom_light_off_delay,
            light_pwm,
            bottom_light_pwm,
            encryption_key,
            machine_name_offset,
            machine_name_size,
            aa_flag,
            per_layer_settings,
            modified_minutes,
            aa_level,
            rest_after_retract,
            transition_layers,
            bottom_rest_after_retract,
            last_layer_index,
            disclaimer_offset,
            disclaimer_size,
            resin_offset,
        })
    }
}

/// A layer's definition, just before its data.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct LayerDef {
    table_size: u32,
    z: f32,
    exposure: f32,
    light_off_delay: f32,
    /// Within page `page` of 4 GiB.
    data_offset: u32,
    page: u32,
    data_size: u32,
    /// The span of the data that is AES-encrypted, from its start; none when the size is 0.
    encrypted_offset: u32,
    encrypted_size: u32,
    phases: Phases,
    rest_after_retract: f32,
    light_pwm: f32,
}

impl LayerDef {
    fn write(&self, out: &mut Vec<u8>) {
        let p = &self.phases;
        put_u32(out, self.table_size);
        for v in [self.z, self.exposure, self.light_off_delay] {
            put_f32(out, v);
        }
        for v in [self.data_offset, self.page, self.data_size, 0, self.encrypted_offset, self.encrypted_size] {
            put_u32(out, v);
        }
        for v in [p.lift_height, p.lift_speed, p.lift_height_2, p.lift_speed_2, p.retract_speed, p.retract_height_2, p.retract_speed_2] {
            put_f32(out, v);
        }
        // The waits before and after the lift.
        put_f32(out, 0.0);
        put_f32(out, 0.0);
        put_f32(out, self.rest_after_retract);
        put_f32(out, self.light_pwm);
        put_u32(out, 0);
    }

    fn read(r: &mut Reader) -> Result<Self, IoError> {
        let (table_size, z, exposure, light_off_delay) = (r.u32()?, r.f32()?, r.f32()?, r.f32()?);
        let (data_offset, page, data_size) = (r.u32()?, r.u32()?, r.u32()?);
        r.skip(4)?;
        let (encrypted_offset, encrypted_size) = (r.u32()?, r.u32()?);
        let phases = Phases {
            lift_height: r.f32()?,
            lift_speed: r.f32()?,
            lift_height_2: r.f32()?,
            lift_speed_2: r.f32()?,
            retract_speed: r.f32()?,
            retract_height_2: r.f32()?,
            retract_speed_2: r.f32()?,
        };
        r.skip(8)?;
        let (rest_after_retract, light_pwm) = (r.f32()?, r.f32()?);
        Ok(LayerDef {
            table_size,
            z,
            exposure,
            light_off_delay,
            data_offset,
            page,
            data_size,
            encrypted_offset,
            encrypted_size,
            phases,
            rest_after_retract,
            light_pwm,
        })
    }
}

fn cipher_error() -> IoError {
    IoError::Format("a block to encrypt is not a whole number of AES blocks".into())
}

/// Encrypt `data` in place; its length is a multiple of 16.
pub(super) fn encrypt(data: &mut [u8]) -> Result<(), IoError> {
    let len = data.len();
    cbc::Encryptor::<Aes256>::new(&KEY.into(), &IV.into()).encrypt_padded_mut::<NoPadding>(data, len).map_err(|_| cipher_error())?;
    Ok(())
}

/// Decrypt `data` in place; its length is a multiple of 16.
pub(super) fn decrypt(data: &mut [u8]) -> Result<(), IoError> {
    cbc::Decryptor::<Aes256>::new(&KEY.into(), &IV.into()).decrypt_padded_mut::<NoPadding>(data).map_err(|_| cipher_error())?;
    Ok(())
}

/// What the signature must be for a file whose settings carry `checksum`.
fn signature(checksum: u64) -> Result<Vec<u8>, IoError> {
    let mut sig = Sha256::digest(checksum.to_le_bytes()).to_vec();
    encrypt(&mut sig)?;
    Ok(sig)
}

fn offset(v: usize) -> Result<u32, IoError> {
    u32::try_from(v).map_err(|_| IoError::TooLarge("the previews, settings and layer pointers pass 4 GiB".into()))
}

fn patch_u32(out: &mut [u8], at: usize, v: u32) {
    if let Some(dst) = out.get_mut(at..at + 4) {
        dst.copy_from_slice(&v.to_le_bytes());
    }
}

/// `job` as an encrypted CTB file.
pub(super) fn write(
    job: &SlicedJob,
    previews: &super::Previews,
    opts: &CtbOptions,
    progress: &Progress,
    cancel: &Cancel,
) -> Result<Vec<u8>, IoError> {
    let (p, s, e, m) = (&job.profile, &job.settings, &job.settings.exposure, &job.settings.motion);
    let count = u32::try_from(job.layers.len()).map_err(|_| IoError::TooLarge("too many layers".into()))?;
    if count == 0 {
        return Err(IoError::Format("the job has no layers".into()));
    }
    let pixels = u64::from(p.resolution_x) * u64::from(p.resolution_y);
    let large = encode_preview(&previews.large)?;
    let small = encode_preview(&previews.small)?;

    // The header and the settings are written last, once every offset is known.
    let mut out = vec![0; HEADER_SIZE + SETTINGS_SIZE];
    let name_at = out.len();
    out.extend_from_slice(p.name.as_bytes());
    out.push(0);
    let disclaimer_at = out.len();
    out.extend_from_slice(DISCLAIMER.as_bytes());
    let resin_at = out.len();
    write_resin(&mut out, &p.name, s.estimates.resin_density as f32)?;
    let large_at = out.len();
    write_preview(&mut out, &previews.large, &large);
    let small_at = out.len();
    write_preview(&mut out, &previews.small, &small);
    let pointers_at = out.len();
    out.resize(pointers_at + LAYER_POINTER_SIZE * job.layers.len(), 0);
    put_u32(&mut out, BEFORE_SIGNATURE);
    put_u32(&mut out, 0);
    let signature_at = out.len();
    out.extend_from_slice(&signature(CHECKSUM)?);
    put_u32(&mut out, AFTER_SIGNATURE);
    offset(out.len())?;

    let volume = job.stats.volume_ml;
    let aa = aa_level(s);
    let settings = Settings {
        checksum: CHECKSUM,
        layer_pointers_offset: offset(pointers_at)?,
        bed: [p.display_x as f32, p.display_y as f32, p.build_height as f32],
        total_height: job.layers.last().map_or(0.0, |l| l.z),
        layer_height: s.layers.height as f32,
        exposure: e.normal as f32,
        bottom_exposure: e.bottom as f32,
        // The waits are the rests after the retract, as mslicer writes them.
        light_off_delay: 0.0,
        bottom_layers: s.layers.bottom_count,
        resolution_x: p.resolution_x,
        resolution_y: p.resolution_y,
        layer_count: count,
        large_preview_offset: offset(large_at)?,
        small_preview_offset: offset(small_at)?,
        print_time: job.stats.print_time_s.ceil() as u32,
        // The rasterizer has already mirrored the image; this tells the printer it was.
        projector_type: u32::from(p.mirror_x),
        bottom: Phases::from_travel(&m.bottom()),
        regular: Phases::from_travel(&m.regular()),
        volume_ml: volume as f32,
        weight_g: (volume * s.estimates.resin_density) as f32,
        cost: cost(job) as f32,
        bottom_light_off_delay: 0.0,
        light_pwm: e.power.into(),
        bottom_light_pwm: e.bottom_power.into(),
        encryption_key: opts.encryption_key,
        machine_name_offset: offset(name_at)?,
        machine_name_size: offset(p.name.len() + 1)?,
        aa_flag: aa_flag(aa),
        per_layer_settings: PER_LAYER_SETTINGS,
        modified_minutes: opts.modified_minutes,
        aa_level: aa,
        rest_after_retract: e.light_off as f32,
        transition_layers: s.layers.transition_count,
        bottom_rest_after_retract: e.bottom_light_off as f32,
        last_layer_index: count - 1,
        disclaimer_offset: offset(disclaimer_at)?,
        disclaimer_size: offset(DISCLAIMER.len())?,
        resin_offset: offset(resin_at)?,
    };

    progress.begin(job.layers.len());
    let mut def_bytes = Vec::with_capacity(LAYER_DEF_SIZE);
    for (i, layer) in (0u32..).zip(&job.layers) {
        if cancel.is_cancelled() {
            return Err(IoError::Cancelled);
        }
        let mut data = encode_layer(&layer.runs, pixels).map_err(|why| IoError::Format(format!("layer {i}: {why}")))?;
        crypt(&mut data, opts.encryption_key, i);
        let def_at = out.len() as u64;
        let data_at = def_at + LAYER_DEF_SIZE as u64;
        let page = |at: u64| u32::try_from(at / PAGE).map_err(|_| IoError::TooLarge("the file is too large".into()));
        let motion = s.layer_motion(i);
        let def = LayerDef {
            table_size: LAYER_DEF_SIZE as u32,
            z: layer.z,
            exposure: layer.exposure,
            light_off_delay: 0.0,
            data_offset: (data_at % PAGE) as u32,
            page: page(data_at)?,
            data_size: u32::try_from(data.len()).map_err(|_| IoError::TooLarge(format!("layer {i} is over 4 GiB")))?,
            encrypted_offset: 0,
            encrypted_size: 0,
            phases: Phases::from_travel(&motion.travel),
            rest_after_retract: layer.light_off,
            light_pwm: f32::from(if i < s.layers.bottom_count { e.bottom_power } else { e.power }),
        };
        let slot = pointers_at + i as usize * LAYER_POINTER_SIZE;
        patch_u32(&mut out, slot, (def_at % PAGE) as u32);
        patch_u32(&mut out, slot + 4, page(def_at)?);
        patch_u32(&mut out, slot + 8, LAYER_DEF_SIZE as u32);
        def_bytes.clear();
        def.write(&mut def_bytes);
        out.extend_from_slice(&def_bytes);
        out.extend_from_slice(&data);
        progress.advance();
    }

    let mut plain = Vec::with_capacity(SETTINGS_SIZE);
    settings.write(&mut plain);
    encrypt(&mut plain)?;
    if let Some(dst) = out.get_mut(HEADER_SIZE..HEADER_SIZE + SETTINGS_SIZE) {
        dst.copy_from_slice(&plain);
    }
    let mut header = Vec::with_capacity(HEADER_SIZE);
    for v in [MAGIC, SETTINGS_SIZE as u32, HEADER_SIZE as u32, 0, VERSION, SIGNATURE_SIZE as u32, offset(signature_at)?, 0, 0x0001_0001, 0, 0x2A, 0] {
        put_u32(&mut header, v);
    }
    if let Some(dst) = out.get_mut(..HEADER_SIZE) {
        dst.copy_from_slice(&header);
    }
    Ok(out)
}

/// The resin parameters: no colour, `density` g/ml, then the resin's type and name and the machine
/// name. mslicer puts the machine name first; this order keeps the addresses right for readers that
/// follow them and suits UVtools 7.1.2, which reads the three in turn.
fn write_resin(out: &mut Vec<u8>, machine: &str, density: f32) -> Result<(), IoError> {
    let at = out.len();
    let type_at = at + RESIN_SIZE;
    let resin_at = type_at + RESIN_TYPE.len();
    let name_at = resin_at + RESIN_NAME.len();
    put_u32(out, 0);
    put_u32(out, 0);
    put_u32(out, offset(name_at)?);
    for v in [RESIN_TYPE.len(), type_at, RESIN_NAME.len(), resin_at, machine.len()] {
        put_u32(out, offset(v)?);
    }
    put_f32(out, density);
    put_u32(out, 0);
    for text in [RESIN_TYPE, RESIN_NAME, machine] {
        out.extend_from_slice(text.as_bytes());
    }
    Ok(())
}

/// Read an encrypted CTB file, version 4 or 5, as hostile input.
pub(super) fn read(bytes: &[u8]) -> Result<CtbFile, IoError> {
    let mut h = Reader::at(bytes, 0, HEADER_SIZE)?;
    let (magic, settings_size, settings_offset) = (h.u32()?, h.u32()?, h.u32()?);
    h.skip(4)?;
    let (version, signature_size, signature_offset) = (h.u32()?, h.u32()?, h.u32()?);
    if magic != MAGIC {
        return Err(IoError::Format("not an encrypted CTB file".into()));
    }
    if !READABLE_VERSIONS.contains(&version) {
        return Err(IoError::Format(format!("encrypted CTB version {version} is not supported")));
    }
    if !(SETTINGS_SIZE as u32..=MAX_SETTINGS_SIZE).contains(&settings_size) || settings_size % 16 != 0 {
        return Err(IoError::Format(format!("settings of {settings_size} bytes")));
    }
    let mut plain = section(bytes, settings_offset.into(), settings_size.into())?.to_vec();
    decrypt(&mut plain)?;
    let st = Settings::read(&mut Reader { data: &plain })?;
    if signature_size as usize != SIGNATURE_SIZE || section(bytes, signature_offset.into(), signature_size.into())? != signature(st.checksum)? {
        return Err(IoError::Format("the signature does not match the settings".into()));
    }
    let (rx, ry) = (st.resolution_x, st.resolution_y);
    if !(1..=MAX_RESOLUTION).contains(&rx) || !(1..=MAX_RESOLUTION).contains(&ry) {
        return Err(IoError::Format(format!("bad screen resolution {rx} × {ry}")));
    }
    if st.layer_count > MAX_LAYERS {
        return Err(IoError::TooLarge(format!("{} layers", st.layer_count)));
    }
    if st.machine_name_size > MAX_NAME_BYTES {
        return Err(IoError::TooLarge("the machine name".into()));
    }
    let name = section(bytes, st.machine_name_offset.into(), st.machine_name_size.into())?;
    let machine_name = String::from_utf8_lossy(name).trim_end_matches('\0').to_owned();
    let large_preview = read_preview(bytes, st.large_preview_offset)?;
    let small_preview = read_preview(bytes, st.small_preview_offset)?;

    let count = st.layer_count as usize;
    let mut pointers = Reader::at(bytes, st.layer_pointers_offset.into(), LAYER_POINTER_SIZE * count)?;
    let pixels = u64::from(rx) * u64::from(ry);
    let mut layers = Vec::with_capacity(count);
    // As in v2: entries sharing one block could make a small file decode it over and over.
    let mut read = 0u64;
    for i in 0..st.layer_count {
        let (at, page) = (pointers.u32()?, pointers.u32()?);
        pointers.skip(8)?;
        let def = LayerDef::read(&mut Reader::at(bytes, u64::from(page) * PAGE + u64::from(at), LAYER_DEF_SIZE)?)?;
        if (def.table_size as usize) < LAYER_DEF_SIZE {
            return Err(IoError::Format(format!("layer {i}'s definition is too short")));
        }
        read += u64::from(def.data_size);
        if read > bytes.len() as u64 {
            return Err(IoError::Format("the layers' data adds up to more than the file".into()));
        }
        let mut data = section(bytes, u64::from(def.page) * PAGE + u64::from(def.data_offset), def.data_size.into())?.to_vec();
        if def.encrypted_size != 0 {
            let from = def.encrypted_offset as usize;
            let span = from.checked_add(def.encrypted_size as usize).and_then(|to| data.get_mut(from..to));
            decrypt(span.ok_or_else(|| IoError::Format(format!("layer {i}'s encrypted span is past its data")))?)?;
        }
        crypt(&mut data, st.encryption_key, i);
        let runs = decode_layer(&data, pixels).map_err(|why| IoError::Format(format!("layer {i}: {why}")))?;
        layers.push(CtbLayer { z: def.z, exposure: def.exposure, light_off: def.rest_after_retract, phases: def.phases, runs });
    }

    let header = Header {
        magic,
        version,
        bed: st.bed,
        total_height: st.total_height,
        layer_height: st.layer_height,
        exposure: st.exposure,
        bottom_exposure: st.bottom_exposure,
        light_off_delay: st.light_off_delay,
        bottom_layers: st.bottom_layers,
        resolution_x: rx,
        resolution_y: ry,
        large_preview_offset: st.large_preview_offset,
        layer_table_offset: st.layer_pointers_offset,
        layer_count: st.layer_count,
        small_preview_offset: st.small_preview_offset,
        print_time: st.print_time,
        projector_type: st.projector_type,
        print_params_offset: 0,
        print_params_size: 0,
        aa_level: 1,
        light_pwm: st.light_pwm,
        bottom_light_pwm: st.bottom_light_pwm,
        encryption_key: st.encryption_key,
        slicer_info_offset: 0,
        slicer_info_size: 0,
    };
    let params = PrintParams {
        bottom_lift_height: st.bottom.lift_height,
        bottom_lift_speed: st.bottom.lift_speed,
        lift_height: st.regular.lift_height,
        lift_speed: st.regular.lift_speed,
        retract_speed: st.regular.retract_speed,
        volume_ml: st.volume_ml,
        weight_g: st.weight_g,
        cost: st.cost,
        bottom_light_off_delay: st.bottom_light_off_delay,
        light_off_delay: st.light_off_delay,
        bottom_layer_count: st.bottom_layers,
        marker: 0,
    };
    let slicer = SlicerInfo {
        machine_name_offset: st.machine_name_offset,
        machine_name_size: st.machine_name_size,
        aa_flag: st.aa_flag,
        per_layer_settings: st.per_layer_settings,
        modified_minutes: st.modified_minutes,
        aa_level: st.aa_level,
        software_version: 0,
        transition_layers: st.transition_layers,
    };
    Ok(CtbFile {
        format: CtbFormat::Encrypted,
        header,
        params,
        slicer,
        bottom_phases: st.bottom,
        phases: st.regular,
        bottom_rest_after_retract: st.bottom_rest_after_retract,
        rest_after_retract: st.rest_after_retract,
        machine_name,
        large_preview,
        small_preview,
        layers,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_sections_have_their_sizes() {
        let mut v = Vec::new();
        Settings::default().write(&mut v);
        assert_eq!(v.len(), SETTINGS_SIZE);
        v.clear();
        LayerDef::default().write(&mut v);
        assert_eq!(v.len(), LAYER_DEF_SIZE);
        v.clear();
        write_resin(&mut v, "", 1.0).unwrap();
        assert_eq!(v.len(), RESIN_SIZE + RESIN_TYPE.len() + RESIN_NAME.len());
        assert_eq!(DISCLAIMER.len(), 320);
    }

    #[test]
    fn the_resin_strings_are_where_their_addresses_say_in_turn() {
        let mut v = vec![0; 5];
        write_resin(&mut v, "PRINTER", 1.1).unwrap();
        let at = |i: usize| u32::from_le_bytes(v[5 + i..5 + i + 4].try_into().unwrap()) as usize;
        let text = |from: usize, len: usize| std::str::from_utf8(&v[from..from + len]).unwrap().to_owned();
        assert_eq!(text(at(16), at(12)), RESIN_TYPE);
        assert_eq!(text(at(24), at(20)), RESIN_NAME);
        assert_eq!(text(at(8), at(28)), "PRINTER");
        assert_eq!(at(16), 5 + RESIN_SIZE, "the type first, right after the fixed part");
        assert_eq!((at(24), at(8)), (at(16) + RESIN_TYPE.len(), at(24) + RESIN_NAME.len()), "then the name, then the machine");
    }

    #[test]
    fn settings_round_trip() {
        let st = Settings {
            checksum: 0x0123_4567_89AB_CDEF,
            layer_pointers_offset: 7,
            bed: [1.0, 2.0, 3.0],
            total_height: 4.0,
            layer_height: 0.05,
            exposure: 2.0,
            bottom_exposure: 30.0,
            light_off_delay: 0.5,
            bottom_layers: 6,
            resolution_x: 10,
            resolution_y: 11,
            layer_count: 12,
            large_preview_offset: 13,
            small_preview_offset: 14,
            print_time: 15,
            projector_type: 1,
            bottom: Phases {
                lift_height: 1.0,
                lift_speed: 2.0,
                lift_height_2: 3.0,
                lift_speed_2: 4.0,
                retract_speed: 5.0,
                retract_height_2: 6.0,
                retract_speed_2: 7.0,
            },
            regular: Phases {
                lift_height: 8.0,
                lift_speed: 9.0,
                lift_height_2: 10.0,
                lift_speed_2: 11.0,
                retract_speed: 12.0,
                retract_height_2: 13.0,
                retract_speed_2: 14.0,
            },
            volume_ml: 1.5,
            weight_g: 1.6,
            cost: 0.1,
            bottom_light_off_delay: 0.25,
            light_pwm: 200,
            bottom_light_pwm: 201,
            encryption_key: 0xDEAD_BEEF,
            machine_name_offset: 16,
            machine_name_size: 17,
            aa_flag: 0x0F,
            per_layer_settings: 0x40,
            modified_minutes: 18,
            aa_level: 4,
            rest_after_retract: 1.25,
            transition_layers: 19,
            bottom_rest_after_retract: 2.5,
            last_layer_index: 11,
            disclaimer_offset: 20,
            disclaimer_size: 21,
            resin_offset: 22,
        };
        let mut v = Vec::new();
        st.write(&mut v);
        assert_eq!(Settings::read(&mut Reader { data: &v }).unwrap(), st);
    }

    #[test]
    fn layer_definitions_round_trip() {
        let def = LayerDef {
            table_size: 0x58,
            z: 0.1,
            exposure: 2.0,
            light_off_delay: 0.5,
            data_offset: 1000,
            page: 1,
            data_size: 77,
            encrypted_offset: 16,
            encrypted_size: 32,
            phases: Phases {
                lift_height: 1.0,
                lift_speed: 2.0,
                lift_height_2: 3.0,
                lift_speed_2: 4.0,
                retract_speed: 5.0,
                retract_height_2: 6.0,
                retract_speed_2: 7.0,
            },
            rest_after_retract: 1.5,
            light_pwm: 255.0,
        };
        let mut v = Vec::new();
        def.write(&mut v);
        assert_eq!(LayerDef::read(&mut Reader { data: &v }).unwrap(), def);
    }

    #[test]
    fn the_cipher_round_trips_and_needs_whole_blocks() {
        let plain: Vec<u8> = (0..64u32).map(|i| (i * 11) as u8).collect();
        let mut data = plain.clone();
        encrypt(&mut data).unwrap();
        assert_ne!(data, plain);
        decrypt(&mut data).unwrap();
        assert_eq!(data, plain);
        assert!(encrypt(&mut [0u8; 15]).is_err() && decrypt(&mut [0u8; 17]).is_err());
    }

    #[test]
    fn the_signature_is_the_one_uvtools_writes() {
        // UVtools 7.1.2 (`UVtoolsCmd convert <file> CTBEncryptedFile`) signs checksum 0xCAFEBABE
        // with these bytes: the key, the IV and the hash agree with an independent writer.
        let uvtools = [
            0x05, 0xA3, 0x72, 0x41, 0x6A, 0xDC, 0x7B, 0x35, 0xCD, 0x50, 0x8A, 0xB1, 0x0A, 0xC3, 0x6B, 0xC4, 0x51, 0xE6, 0x40, 0xED, 0x11, 0x76, 0x02,
            0x20, 0xBE, 0x73, 0xBF, 0x8D, 0x96, 0x2C, 0xAE, 0xD6,
        ];
        assert_eq!(signature(0xCAFE_BABE).unwrap(), uvtools);
    }
}
