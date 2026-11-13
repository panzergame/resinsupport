//! Project files (`.rsup`): the models and their supports, as the user left them.
//!
//! Ported from SolidCraft `crates/io/src/design.rs` (the design container: package writer,
//! bounded reader, hash-named blobs) at `4bdffef0`, reduced to what a project holds.
//!
//! A project is a ZIP with nothing in front of it:
//! - `container.json`: `{"format": "resinsupport-container/1"}`, the package layout;
//! - `document.json`: `{"format": "resinsupport/2", "models": […], "supports": […], "braces": […]}`,
//!   Deflated (`resinsupport/1`, read too, has no braces);
//! - `blobs/<lowercase SHA-256>.mesh`: each distinct mesh once, stored, hashed over its bytes.
//!
//! A mesh blob is `RSMESH01`, a little-endian `u64` triangle count, then nine little-endian `f32`
//! per triangle: the model's own triangle soup, exactly. Supports name their model by its index
//! in `models` and keep their tip and normal in the model's own coordinates; placed corners are in
//! the scene. A branch names the support it joins by its index in `supports`, always an earlier one.
//! A brace names its two supports by their index in `supports`, neither a branch. A file of a
//! newer format than this build knows is refused, never guessed at.
//!
//! Files are hostile: entries, sizes and counts are bounded before anything is allocated, names
//! that could reach outside the package are refused, ZIP CRCs and blob hashes are checked, and
//! nothing here panics.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Read, Seek, SeekFrom, Write};
use std::sync::Arc;

use resin_geom::{Mesh, Placement, Vec3};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::IoError;
use crate::stl::MAX_TRIANGLES;

/// The file name extension of a project.
pub const EXTENSION: &str = "rsup";
const CONTAINER_FORMAT: &str = "resinsupport-container/1";
const FORMAT_NAME: &str = "resinsupport/";
const FORMAT_VERSION: u64 = 2;
const MESH_MAGIC: &[u8; 8] = b"RSMESH01";
const MESH_HEADER: usize = 16;
const TRIANGLE_BYTES: usize = 36;
const MAX_ENTRIES: usize = 10_000;
const MAX_DOCUMENT: u64 = 256 << 20;
/// Below 4 GiB with room to spare: a bigger package would need ZIP64, which the reader refuses.
const MAX_EXPANDED: u64 = 3 << 30;
/// Largest file read at all.
pub const MAX_FILE_BYTES: u64 = MAX_EXPANDED;

/// What a project holds.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Project {
    pub models: Vec<ProjectModel>,
    pub supports: Vec<ProjectSupport>,
    pub braces: Vec<ProjectBrace>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProjectModel {
    pub name: String,
    /// As imported, in its own coordinates. Models that share one mesh share it in the file too.
    pub mesh: Arc<Mesh>,
    /// Measured from the mesh as imported.
    pub placement: Placement,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProjectSupport {
    /// Index into [`Project::models`].
    pub model: usize,
    /// In the model's own coordinates.
    pub tip: Vec3,
    pub normal: Vec3,
    /// The corners the user placed, in the scene; `None` while the route is worked out.
    pub corners: Option<Vec<Vec3>>,
    /// For a branch, the index into [`Project::supports`] of the support it joins: always lower
    /// than its own.
    pub parent: Option<usize>,
}

/// How a brace's struts run. Here rather than in the engine because the project file names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BracePattern {
    /// One strut per bay, alternating.
    ZigZag,
    /// Both diagonals in each bay.
    Cross,
}

impl BracePattern {
    pub const ALL: [BracePattern; 2] = [BracePattern::ZigZag, BracePattern::Cross];
    pub fn id(self) -> &'static str {
        match self {
            BracePattern::ZigZag => "zigzag",
            BracePattern::Cross => "cross",
        }
    }
    pub fn parse(s: &str) -> Option<BracePattern> {
        BracePattern::ALL.into_iter().find(|p| p.id() == s)
    }
}

/// Struts between two supports, with the settings they were made with.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectBrace {
    /// Indices into [`Project::supports`], neither a branch.
    pub a: usize,
    pub b: usize,
    pub pattern: BracePattern,
    /// Degrees from horizontal.
    pub angle: f64,
    /// Of the body's diameter.
    pub diameter_ratio: f64,
    /// Millimetres.
    pub head_gap: f64,
    pub foot_gap: f64,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    format: String,
    models: Vec<ModelEntry>,
    supports: Vec<SupportEntry>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    braces: Vec<ProjectBrace>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ModelEntry {
    name: String,
    placement: Placement,
    mesh_blob: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SupportEntry {
    model: usize,
    tip: Vec3,
    normal: Vec3,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    corners: Option<Vec<Vec3>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    parent: Option<usize>,
}

fn invalid(message: impl std::fmt::Display) -> IoError {
    IoError::Format(format!("project file: {message}"))
}

/// Whether `bytes` start like a project (a ZIP), whatever the file is called.
pub fn is_project(bytes: &[u8]) -> bool {
    bytes.starts_with(b"PK\x03\x04")
}

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

fn part_limit(name: &str) -> u64 {
    if name.ends_with(".mesh") { (MAX_TRIANGLES * TRIANGLE_BYTES + MESH_HEADER) as u64 } else { MAX_DOCUMENT }
}

fn encode_mesh(mesh: &Mesh) -> Result<Vec<u8>, IoError> {
    let count = mesh.triangles.len();
    if count > MAX_TRIANGLES {
        return Err(IoError::TooLarge(format!("{count} triangles, the limit is {MAX_TRIANGLES}")));
    }
    if mesh.triangles.iter().flatten().flatten().any(|v| !v.is_finite()) {
        return Err(invalid("a mesh has a non-finite coordinate"));
    }
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(MESH_HEADER + count * TRIANGLE_BYTES).map_err(|_| invalid("could not reserve a mesh blob"))?;
    bytes.extend_from_slice(MESH_MAGIC);
    bytes.extend_from_slice(&(count as u64).to_le_bytes());
    for v in mesh.triangles.iter().flatten().flatten() {
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    Ok(bytes)
}

fn decode_mesh(bytes: &[u8]) -> Result<Mesh, IoError> {
    if bytes.get(..8) != Some(MESH_MAGIC.as_slice()) {
        return Err(invalid("unsupported mesh blob"));
    }
    let count: [u8; 8] = bytes.get(8..16).ok_or_else(|| invalid("truncated mesh blob"))?.try_into().map_err(invalid)?;
    let count = usize::try_from(u64::from_le_bytes(count)).map_err(invalid)?;
    if count > MAX_TRIANGLES {
        return Err(IoError::TooLarge(format!("{count} triangles, the limit is {MAX_TRIANGLES}")));
    }
    let table = bytes.get(MESH_HEADER..).filter(|t| t.len() == count * TRIANGLE_BYTES).ok_or_else(|| invalid("mesh blob length mismatch"))?;
    let mut triangles = Vec::new();
    triangles.try_reserve_exact(count).map_err(|_| invalid("could not reserve a mesh"))?;
    for record in table.chunks_exact(TRIANGLE_BYTES) {
        let mut v = [0f32; 9];
        for (x, b) in v.iter_mut().zip(record.chunks_exact(4)) {
            *x = f32::from_le_bytes([b[0], b[1], b[2], b[3]]);
        }
        if v.iter().any(|x| !x.is_finite()) {
            return Err(invalid("a mesh has a non-finite coordinate"));
        }
        triangles.push([[v[0], v[1], v[2]], [v[3], v[4], v[5]], [v[6], v[7], v[8]]]);
    }
    Ok(Mesh::new(triangles))
}

struct PackageWriter {
    zip: zip::ZipWriter<Cursor<Vec<u8>>>,
    names: BTreeSet<String>,
    expanded: u64,
}

impl PackageWriter {
    fn new() -> PackageWriter {
        PackageWriter { zip: zip::ZipWriter::new(Cursor::new(Vec::new())), names: BTreeSet::new(), expanded: 0 }
    }
    fn part(&mut self, name: &str, bytes: &[u8], compressed: bool) -> Result<(), IoError> {
        if bytes.len() as u64 > part_limit(name) {
            return Err(IoError::TooLarge(format!("project part {name}")));
        }
        if !self.names.insert(name.to_owned()) {
            return Ok(());
        }
        self.expanded = self.expanded.saturating_add(bytes.len() as u64);
        if self.expanded > MAX_EXPANDED || self.names.len() > MAX_ENTRIES {
            return Err(IoError::TooLarge(format!("the project would pass {} MiB", MAX_EXPANDED >> 20)));
        }
        let method = if compressed { zip::CompressionMethod::Deflated } else { zip::CompressionMethod::Stored };
        let options = zip::write::SimpleFileOptions::default().compression_method(method).compression_level(compressed.then_some(1));
        self.zip.start_file(name, options).map_err(invalid)?;
        self.zip.write_all(bytes).map_err(invalid)
    }
    /// Store `bytes` once under their hash; the reference to it.
    fn blob(&mut self, bytes: &[u8], extension: &str) -> Result<String, IoError> {
        let name = format!("blobs/{}.{extension}", digest(bytes));
        // Stored: a binary mesh barely shrinks, and stored parts read fastest.
        self.part(&name, bytes, false)?;
        Ok(name)
    }
    fn finish(self) -> Result<Vec<u8>, IoError> {
        Ok(self.zip.finish().map_err(invalid)?.into_inner())
    }
}

/// The project as file bytes.
pub fn write_project(project: &Project) -> Result<Vec<u8>, IoError> {
    let mut writer = PackageWriter::new();
    // One blob per shared mesh, without encoding it again.
    let mut written: Vec<(*const Mesh, String)> = Vec::new();
    let mut models = Vec::with_capacity(project.models.len());
    for m in &project.models {
        if !m.placement.is_finite() {
            return Err(invalid(format!("model {} has a non-finite placement", m.name)));
        }
        let key = Arc::as_ptr(&m.mesh);
        let blob = match written.iter().find(|(k, _)| *k == key) {
            Some((_, name)) => name.clone(),
            None => {
                let name = writer.blob(&encode_mesh(&m.mesh)?, "mesh")?;
                written.push((key, name.clone()));
                name
            }
        };
        models.push(ModelEntry { name: m.name.clone(), placement: m.placement, mesh_blob: blob });
    }
    let mut supports = Vec::with_capacity(project.supports.len());
    for (i, s) in project.supports.iter().enumerate() {
        check_support(s, i, project.models.len())?;
        supports.push(SupportEntry { model: s.model, tip: s.tip, normal: s.normal, corners: s.corners.clone(), parent: s.parent });
    }
    for b in &project.braces {
        check_brace(b, &project.supports)?;
    }
    let document = Document { format: format!("{FORMAT_NAME}{FORMAT_VERSION}"), models, supports, braces: project.braces.clone() };
    writer.part("document.json", &serde_json::to_vec(&document).map_err(invalid)?, true)?;
    writer.part("container.json", &serde_json::to_vec(&serde_json::json!({ "format": CONTAINER_FORMAT })).map_err(invalid)?, false)?;
    writer.finish()
}

/// Support number `index` of the project.
fn check_support(s: &ProjectSupport, index: usize, models: usize) -> Result<(), IoError> {
    if s.model >= models {
        return Err(invalid(format!("a support names model {}, there are {models}", s.model)));
    }
    // Only an earlier one: no branch can then lead round to itself.
    if s.parent.is_some_and(|p| p >= index) {
        return Err(invalid(format!("support {index} branches from a support that is not before it")));
    }
    let corners_ok = s.corners.as_ref().is_none_or(|c| c.iter().all(|p| p.is_finite()));
    if !s.tip.is_finite() || !s.normal.is_finite() || !corners_ok {
        return Err(invalid("a support has a non-finite point"));
    }
    Ok(())
}

fn check_brace(b: &ProjectBrace, supports: &[ProjectSupport]) -> Result<(), IoError> {
    let root = |i: usize| supports.get(i).is_some_and(|s| s.parent.is_none());
    if b.a == b.b || !root(b.a) || !root(b.b) {
        return Err(invalid(format!("a brace joins supports {} and {}, which are not two supports with bodies", b.a, b.b)));
    }
    if ![b.angle, b.diameter_ratio, b.head_gap, b.foot_gap].iter().all(|v| v.is_finite()) {
        return Err(invalid("a brace has a non-finite setting"));
    }
    Ok(())
}

struct PackageReader<R: Read + Seek> {
    zip: zip::ZipArchive<R>,
}

impl<R: Read + Seek> PackageReader<R> {
    fn new(mut source: R) -> Result<Self, IoError> {
        let length = source.seek(SeekFrom::End(0)).map_err(invalid)?;
        if length > MAX_FILE_BYTES {
            return Err(IoError::TooLarge(format!("{length} bytes, the limit is {MAX_FILE_BYTES}")));
        }
        let tail_length = length.min(65_557);
        source.seek(SeekFrom::Start(length - tail_length)).map_err(invalid)?;
        let mut tail = vec![0; tail_length as usize];
        source.read_exact(&mut tail).map_err(invalid)?;
        // zip's name index can coalesce duplicate central-directory entries, so the declared entry
        // count is checked too: duplicate names can't silently pick a payload. No ZIP64, no disks.
        let eocd = tail.windows(4).rposition(|v| v == b"PK\x05\x06").ok_or_else(|| invalid("missing ZIP directory"))?;
        let end = tail.get(eocd..).filter(|v| v.len() >= 22).ok_or_else(|| invalid("truncated ZIP directory"))?;
        let u16_at = |offset: usize| -> Result<usize, IoError> {
            let b = end.get(offset..offset + 2).ok_or_else(|| invalid("truncated ZIP directory"))?;
            Ok(usize::from(u16::from_le_bytes([b[0], b[1]])))
        };
        let declared = u16_at(10)?;
        if declared > MAX_ENTRIES || u16_at(4)? != 0 || u16_at(6)? != 0 || u16_at(8)? != declared || end.len() != 22 + u16_at(20)? {
            return Err(invalid("too many entries or an unsupported ZIP directory"));
        }
        // A project is a ZIP with nothing in front of it: saying so skips zip's search for a prefix.
        let config = zip::read::Config { archive_offset: zip::read::ArchiveOffset::Known(0) };
        let mut zip = zip::ZipArchive::with_config(config, source).map_err(invalid)?;
        if zip.len() != declared {
            return Err(invalid("duplicate entries"));
        }
        let mut names = BTreeSet::new();
        let mut total = 0u64;
        for i in 0..zip.len() {
            let part = zip.by_index_raw(i).map_err(invalid)?;
            let name = part.name().map_err(invalid)?.into_owned();
            if name.is_empty() || name.contains('\\') || name.split('/').any(|c| c.is_empty() || c == "." || c == "..") || !names.insert(name.clone())
            {
                return Err(invalid("duplicate or invalid entry name"));
            }
            total = total.saturating_add(part.size());
            if part.size() > part_limit(&name) || total > MAX_EXPANDED {
                return Err(IoError::TooLarge("the project expands too far".into()));
            }
        }
        Ok(PackageReader { zip })
    }

    fn part(&mut self, name: &str) -> Result<Vec<u8>, IoError> {
        let part = self.zip.by_name(name).map_err(|e| invalid(format!("part `{name}`: {e}")))?;
        let expected = part.size();
        if expected > part_limit(name) {
            return Err(IoError::TooLarge(format!("project part {name}")));
        }
        let mut bytes = Vec::new();
        bytes.try_reserve_exact(expected as usize).map_err(|_| invalid("could not reserve a part"))?;
        // A compressed part may expand past its declared size (a decompression bomb): stop one byte
        // past it. Reading to the end checks the CRC.
        part.take(expected + 1).read_to_end(&mut bytes).map_err(invalid)?;
        if bytes.len() as u64 != expected {
            return Err(invalid(format!("part `{name}` is not the size it says")));
        }
        Ok(bytes)
    }
}

/// Read a project from memory.
pub fn read_project(bytes: &[u8]) -> Result<Project, IoError> {
    if !is_project(bytes) {
        return Err(invalid("not a ResinSupport project"));
    }
    let mut reader = PackageReader::new(Cursor::new(bytes))?;
    let container: serde_json::Value = serde_json::from_slice(&reader.part("container.json")?).map_err(invalid)?;
    match container.get("format").and_then(serde_json::Value::as_str) {
        Some(CONTAINER_FORMAT) => {}
        Some(f) if f.starts_with("resinsupport-container/") => {
            return Err(invalid(format!("made by a newer ResinSupport ({f}); this one reads {CONTAINER_FORMAT}")));
        }
        _ => return Err(invalid("not a ResinSupport project")),
    }
    let document = reader.part("document.json")?;
    let value: serde_json::Value = serde_json::from_slice(&document).map_err(invalid)?;
    let format = value.get("format").and_then(serde_json::Value::as_str).ok_or_else(|| invalid("the document has no format"))?;
    match format.strip_prefix(FORMAT_NAME).and_then(|v| v.parse::<u64>().ok()) {
        Some(1..=FORMAT_VERSION) => {}
        Some(v) if v > FORMAT_VERSION => {
            return Err(invalid(format!("made by a newer ResinSupport ({format}); this one reads {FORMAT_NAME}{FORMAT_VERSION}")));
        }
        _ => return Err(invalid(format!("unknown document format `{format}`"))),
    }
    let document: Document = serde_json::from_value(value).map_err(invalid)?;
    let mut meshes: BTreeMap<String, Arc<Mesh>> = BTreeMap::new();
    let mut models = Vec::with_capacity(document.models.len());
    for m in document.models {
        if !m.placement.is_finite() {
            return Err(invalid(format!("model {} has a non-finite placement", m.name)));
        }
        let mesh = match meshes.get(&m.mesh_blob) {
            Some(mesh) => mesh.clone(),
            None => {
                let hash = m
                    .mesh_blob
                    .strip_prefix("blobs/")
                    .and_then(|n| n.strip_suffix(".mesh"))
                    .filter(|h| h.len() == 64 && h.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
                    .ok_or_else(|| invalid("invalid mesh reference"))?;
                let bytes = reader.part(&m.mesh_blob)?;
                if digest(&bytes) != hash {
                    return Err(invalid(format!("{} does not match its hash", m.mesh_blob)));
                }
                let mesh = Arc::new(decode_mesh(&bytes)?);
                meshes.insert(m.mesh_blob.clone(), mesh.clone());
                mesh
            }
        };
        models.push(ProjectModel { name: m.name, mesh, placement: m.placement });
    }
    let supports: Vec<ProjectSupport> = document
        .supports
        .into_iter()
        .map(|s| ProjectSupport { model: s.model, tip: s.tip, normal: s.normal, corners: s.corners, parent: s.parent })
        .collect();
    for (i, s) in supports.iter().enumerate() {
        check_support(s, i, models.len())?;
    }
    for b in &document.braces {
        check_brace(b, &supports)?;
    }
    Ok(Project { models, supports, braces: document.braces })
}

/// Read a project file from disk.
#[cfg(not(target_arch = "wasm32"))]
pub fn read_project_file(path: &std::path::Path) -> Result<Project, IoError> {
    let meta = std::fs::metadata(path)?;
    if !meta.is_file() {
        return Err(IoError::Format(format!("{} is not a file", path.display())));
    }
    if meta.len() > MAX_FILE_BYTES {
        return Err(IoError::TooLarge(format!("{} bytes, the limit is {MAX_FILE_BYTES}", meta.len())));
    }
    read_project(&std::fs::read(path)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cube() -> Arc<Mesh> {
        let c = |i: u8| [f32::from(i & 1) * 10.0, f32::from((i >> 1) & 1) * 10.0, f32::from((i >> 2) & 1) * 10.0];
        let quads = [[0, 2, 3, 1], [4, 5, 7, 6], [0, 1, 5, 4], [2, 6, 7, 3], [0, 4, 6, 2], [1, 3, 7, 5]];
        Arc::new(Mesh::new(quads.iter().flat_map(|&[a, b, cc, d]| [[c(a), c(b), c(cc)], [c(a), c(cc), c(d)]]).collect()))
    }

    fn sample() -> Project {
        let mesh = cube();
        let mut moved = Placement::at(Vec3::new(5.0, 5.0, 5.0));
        moved.position = Vec3::new(40.0, -3.25, 7.0);
        moved.rotation = Vec3::new(0.0, 0.0, 30.0);
        moved.scale = Vec3::new(1.5, 1.0, 0.1);
        Project {
            models: vec![
                ProjectModel { name: "cube".into(), mesh: mesh.clone(), placement: Placement::at(Vec3::new(5.0, 5.0, 5.0)) },
                ProjectModel { name: "copy é".into(), mesh, placement: moved },
            ],
            supports: vec![
                ProjectSupport { model: 0, tip: Vec3::new(5.0, 5.0, 0.0), normal: Vec3::new(0.0, 0.0, -1.0), corners: None, parent: None },
                ProjectSupport {
                    model: 1,
                    tip: Vec3::new(1.0, 2.0, 0.0),
                    normal: Vec3::new(0.0, 0.0, -1.0),
                    corners: Some(vec![Vec3::new(41.0, -2.0, 4.0), Vec3::new(45.0, -2.0, 2.0)]),
                    parent: Some(0),
                },
                ProjectSupport { model: 0, tip: Vec3::new(1.0, 1.0, 0.0), normal: Vec3::new(0.0, 0.0, -1.0), corners: None, parent: None },
            ],
            braces: vec![ProjectBrace { a: 0, b: 2, pattern: BracePattern::Cross, angle: 50.0, diameter_ratio: 0.4, head_gap: 1.5, foot_gap: 0.5 }],
        }
    }

    fn parts(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        (0..zip.len())
            .map(|i| {
                let mut f = zip.by_index(i).unwrap();
                let mut b = Vec::new();
                f.read_to_end(&mut b).unwrap();
                (f.name().unwrap().into_owned(), b)
            })
            .collect()
    }

    fn package(parts: &[(&str, &[u8])]) -> Vec<u8> {
        let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, bytes) in parts {
            w.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
            w.write_all(bytes).unwrap();
        }
        w.finish().unwrap().into_inner()
    }

    /// The sample's parts with `document.json` replaced.
    fn with_document(document: &serde_json::Value) -> Vec<u8> {
        let bytes = write_project(&sample()).unwrap();
        let doc = serde_json::to_vec(document).unwrap();
        let mut list: Vec<(String, Vec<u8>)> = parts(&bytes).into_iter().filter(|(n, _)| n != "document.json").collect();
        list.push(("document.json".into(), doc));
        package(&list.iter().map(|(n, b)| (n.as_str(), b.as_slice())).collect::<Vec<_>>())
    }

    fn document() -> serde_json::Value {
        let bytes = write_project(&sample()).unwrap();
        let doc = parts(&bytes).into_iter().find(|(n, _)| n == "document.json").unwrap().1;
        serde_json::from_slice(&doc).unwrap()
    }

    fn refused(bytes: &[u8]) -> String {
        match read_project(bytes) {
            Ok(_) => panic!("read a broken project"),
            Err(e) => e.to_string(),
        }
    }

    #[test]
    fn a_project_round_trips_exactly_and_stores_a_shared_mesh_once() {
        let p = sample();
        let bytes = write_project(&p).unwrap();
        assert!(is_project(&bytes));
        let back = read_project(&bytes).unwrap();
        assert_eq!(back, p);
        assert!(Arc::ptr_eq(&back.models[0].mesh, &back.models[1].mesh));
        let names: Vec<String> = parts(&bytes).into_iter().map(|(n, _)| n).collect();
        assert_eq!(names.iter().filter(|n| n.ends_with(".mesh")).count(), 1);
        assert!(names.contains(&"container.json".to_string()));
        assert_eq!(write_project(&back).unwrap(), bytes, "writing is deterministic");
    }

    #[test]
    fn an_empty_project_round_trips() {
        assert_eq!(read_project(&write_project(&Project::default()).unwrap()).unwrap(), Project::default());
    }

    #[test]
    fn newer_formats_are_refused_with_a_reason() {
        let mut d = document();
        d["format"] = "resinsupport/3".into();
        assert!(refused(&with_document(&d)).contains("newer"));
        let bytes = write_project(&sample()).unwrap();
        let mut list: Vec<(String, Vec<u8>)> = parts(&bytes).into_iter().filter(|(n, _)| n != "container.json").collect();
        list.push(("container.json".into(), br#"{"format":"resinsupport-container/9"}"#.to_vec()));
        assert!(refused(&package(&list.iter().map(|(n, b)| (n.as_str(), b.as_slice())).collect::<Vec<_>>())).contains("newer"));
    }

    #[test]
    fn a_first_format_project_reads_with_no_braces() {
        let mut d = document();
        d["format"] = "resinsupport/1".into();
        d.as_object_mut().unwrap().remove("braces");
        let back = read_project(&with_document(&d)).unwrap();
        assert_eq!((back.supports.len(), back.braces.len()), (3, 0));
    }

    #[test]
    fn hostile_documents_are_refused() {
        let mut d = document();
        d["supports"][0]["model"] = 7.into();
        refused(&with_document(&d));
        let mut d = document();
        d["models"][0]["mesh_blob"] = "blobs/../../etc/passwd.mesh".into();
        refused(&with_document(&d));
        let mut d = document();
        d["models"][0]["mesh_blob"] = format!("blobs/{}.mesh", "0".repeat(64)).into();
        refused(&with_document(&d));
        let mut d = document();
        d["models"][0]["placement"]["scale"][0] = serde_json::Value::Null;
        refused(&with_document(&d));
        for parent in [serde_json::json!(1), serde_json::json!(5), serde_json::json!(-1), serde_json::json!("0")] {
            let mut d = document();
            d["supports"][1]["parent"] = parent;
            refused(&with_document(&d));
        }
        // A brace to itself, to a branch, to no support, or with a non-finite setting.
        for (key, value) in
            [("b", serde_json::json!(0)), ("b", serde_json::json!(1)), ("a", serde_json::json!(9)), ("angle", serde_json::json!(null))]
        {
            let mut d = document();
            d["braces"][0][key] = value;
            refused(&with_document(&d));
        }
        let mut d = document();
        d["braces"][0]["pattern"] = "spiral".into();
        refused(&with_document(&d));
        let mut d = document();
        d["surprise"] = 1.into();
        refused(&with_document(&d));
        let mut d = document();
        d["format"] = "solidcraft/1".into();
        refused(&with_document(&d));
        for doc in [&b"[]"[..], b"{", b"null", &[0xff, 0xfe]] {
            let bytes = write_project(&sample()).unwrap();
            let mut list: Vec<(String, Vec<u8>)> = parts(&bytes).into_iter().filter(|(n, _)| n != "document.json").collect();
            list.push(("document.json".into(), doc.to_vec()));
            refused(&package(&list.iter().map(|(n, b)| (n.as_str(), b.as_slice())).collect::<Vec<_>>()));
        }
    }

    #[test]
    fn hostile_packages_are_refused() {
        let bytes = write_project(&sample()).unwrap();
        for n in [0, 4, 30, bytes.len() / 2, bytes.len() - 1] {
            refused(&bytes[..n]);
        }
        refused(b"solid cube\nendsolid\n");
        // A changed byte in the stored mesh: its CRC or hash no longer matches.
        let list = parts(&bytes);
        let (mesh_name, mesh) = list.iter().find(|(n, _)| n.ends_with(".mesh")).unwrap();
        let at = bytes.windows(mesh.len()).position(|w| w == mesh.as_slice()).unwrap();
        let mut flipped = bytes.clone();
        flipped[at + 40] ^= 1;
        refused(&flipped);
        // The same, with the CRC right: the hash catches it.
        let mut bad = mesh.clone();
        bad[40] ^= 1;
        let others: Vec<(&str, &[u8])> = list.iter().filter(|(n, _)| n != mesh_name).map(|(n, b)| (n.as_str(), b.as_slice())).collect();
        assert!(refused(&package(&[others.as_slice(), &[(mesh_name.as_str(), bad.as_slice())]].concat())).contains("hash"));
        // Missing parts, traversal, duplicates.
        refused(&package(&others));
        refused(&package(&[("container.json", br#"{"format":"resinsupport-container/1"}"#)]));
        refused(&package(&[("../document.json", b"{}"), ("container.json", br#"{"format":"resinsupport-container/1"}"#)]));
    }

    #[test]
    fn hostile_mesh_blobs_are_refused() {
        let good = encode_mesh(&cube()).unwrap();
        assert_eq!(decode_mesh(&good).unwrap(), *cube());
        let mut nan = good.clone();
        nan[MESH_HEADER..MESH_HEADER + 4].copy_from_slice(&f32::NAN.to_le_bytes());
        let mut huge = good.clone();
        huge[8..16].copy_from_slice(&u64::MAX.to_le_bytes());
        let mut short = good.clone();
        short[8..16].copy_from_slice(&13u64.to_le_bytes());
        for b in [&nan[..], &huge, &short, &good[..20], b"RSMESH02", b""] {
            assert!(decode_mesh(b).is_err());
        }
    }

    #[test]
    fn non_finite_input_is_never_written() {
        let mut p = sample();
        p.supports[1].corners = Some(vec![Vec3::new(f64::NAN, 0.0, 0.0)]);
        assert!(write_project(&p).is_err());
        // A branch must join an earlier support: itself or a later one could lead round in a loop.
        for parent in [1, 2] {
            let mut p = sample();
            p.supports[1].parent = Some(parent);
            assert!(write_project(&p).is_err());
        }
        let mut p = sample();
        p.braces[0].b = 1;
        assert!(write_project(&p).is_err(), "a brace to a branch");
        let mut p = sample();
        p.models[0].mesh = Arc::new(Mesh::new(vec![[[f32::INFINITY, 0.0, 0.0], [0.0; 3], [0.0; 3]]]));
        assert!(write_project(&p).is_err());
    }
}
