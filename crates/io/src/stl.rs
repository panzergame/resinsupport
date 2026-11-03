//! STL import, binary and ASCII. Files are hostile: sizes are checked before anything is
//! allocated, every number is checked, and nothing here panics.

use resin_geom::Mesh;

use crate::IoError;

/// More triangles than this is refused (a 2.5 GB binary file): the display copy alone would
/// need several gigabytes.
pub const MAX_TRIANGLES: usize = 50_000_000;
/// Largest file read at all. ASCII spends about 250 bytes per triangle.
pub const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024 * 1024;

const HEADER: usize = 80;
const RECORD: usize = 50;

/// A read STL.
#[derive(Clone, Debug, PartialEq)]
pub struct StlImport {
    pub mesh: Mesh,
    /// Triangles skipped: a non-finite corner, or no area.
    pub dropped: usize,
    /// The ASCII `solid` name, when there is one.
    pub name: Option<String>,
}

/// Read an STL from memory, binary or ASCII.
pub fn read_stl(bytes: &[u8]) -> Result<StlImport, IoError> {
    if let Some(count) = binary_count(bytes) {
        return read_binary(bytes, count);
    }
    if looks_ascii(bytes) {
        return read_ascii(bytes);
    }
    Err(IoError::Format("not an STL file: neither a binary triangle table nor ASCII `solid` text".into()))
}

/// Read an STL file from disk.
#[cfg(not(target_arch = "wasm32"))]
pub fn read_stl_file(path: &std::path::Path) -> Result<StlImport, IoError> {
    let len = std::fs::metadata(path)?.len();
    if len > MAX_FILE_BYTES {
        return Err(IoError::TooLarge(format!("{len} bytes, the limit is {MAX_FILE_BYTES}")));
    }
    read_stl(&std::fs::read(path)?)
}

/// The triangle count when `bytes` is a binary STL: the count field matches the length exactly,
/// or the file carries trailing bytes after a complete table (some exporters pad). ASCII files
/// that happen to start with `solid` are binary only if the count also fits.
fn binary_count(bytes: &[u8]) -> Option<usize> {
    let field = bytes.get(HEADER..HEADER + 4)?;
    let count = u32::from_le_bytes([field[0], field[1], field[2], field[3]]) as usize;
    let needed = count.checked_mul(RECORD)?.checked_add(HEADER + 4)?;
    let exact = needed == bytes.len();
    let padded = needed < bytes.len() && !looks_ascii(bytes);
    (exact || padded).then_some(count)
}

fn looks_ascii(bytes: &[u8]) -> bool {
    let start = bytes.iter().position(|b| !b.is_ascii_whitespace()).unwrap_or(bytes.len());
    bytes.get(start..start + 5).is_some_and(|s| s.eq_ignore_ascii_case(b"solid"))
}

fn read_binary(bytes: &[u8], count: usize) -> Result<StlImport, IoError> {
    if count > MAX_TRIANGLES {
        return Err(IoError::TooLarge(format!("{count} triangles, the limit is {MAX_TRIANGLES}")));
    }
    let table = bytes.get(HEADER + 4..).ok_or_else(|| IoError::Format("truncated binary STL".into()))?;
    let mut triangles = Vec::with_capacity(count);
    let mut dropped = 0;
    for record in table.chunks_exact(RECORD).take(count) {
        // Skip the stored normal (12 bytes): it is often wrong, the winding is what counts.
        let f = |i: usize| {
            let o = 12 + i * 4;
            record.get(o..o + 4).map_or(f32::NAN, |b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        };
        let tri = [[f(0), f(1), f(2)], [f(3), f(4), f(5)], [f(6), f(7), f(8)]];
        push(&mut triangles, &mut dropped, tri);
    }
    Ok(StlImport { mesh: Mesh::new(triangles), dropped, name: None })
}

fn read_ascii(bytes: &[u8]) -> Result<StlImport, IoError> {
    let text = String::from_utf8_lossy(bytes);
    let mut name = None;
    let mut triangles = Vec::new();
    let mut dropped = 0;
    let mut corners: Vec<[f32; 3]> = Vec::with_capacity(3);
    for line in text.lines() {
        let mut words = line.split_ascii_whitespace();
        match words.next().map(str::to_ascii_lowercase).as_deref() {
            Some("solid") if name.is_none() => {
                let rest: Vec<&str> = words.collect();
                if !rest.is_empty() {
                    name = Some(rest.join(" "));
                }
            }
            Some("facet") => corners.clear(),
            Some("vertex") => {
                let mut v = [f32::NAN; 3];
                for c in &mut v {
                    *c = words.next().and_then(|w| w.parse::<f32>().ok()).unwrap_or(f32::NAN);
                }
                corners.push(v);
            }
            Some("endfacet") => {
                if let [a, b, c] = corners[..] {
                    if triangles.len() >= MAX_TRIANGLES {
                        return Err(IoError::TooLarge(format!("more than {MAX_TRIANGLES} triangles")));
                    }
                    push(&mut triangles, &mut dropped, [a, b, c]);
                } else {
                    dropped += 1;
                }
                corners.clear();
            }
            _ => {}
        }
    }
    if triangles.is_empty() && dropped == 0 {
        return Err(IoError::Format("ASCII STL with no facets".into()));
    }
    Ok(StlImport { mesh: Mesh::new(triangles), dropped, name })
}

fn push(triangles: &mut Vec<[[f32; 3]; 3]>, dropped: &mut usize, tri: [[f32; 3]; 3]) {
    if tri.iter().flatten().all(|c| c.is_finite()) && Mesh::face_normal(&tri).is_some() {
        triangles.push(tri);
    } else {
        *dropped += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binary(tris: &[[[f32; 3]; 3]]) -> Vec<u8> {
        let mut out = vec![0u8; HEADER];
        out.extend_from_slice(&(tris.len() as u32).to_le_bytes());
        for t in tris {
            out.extend_from_slice(&[0u8; 12]);
            for c in t.iter().flatten() {
                out.extend_from_slice(&c.to_le_bytes());
            }
            out.extend_from_slice(&[0u8; 2]);
        }
        out
    }

    const TRI: [[f32; 3]; 3] = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];

    #[test]
    fn reads_binary() {
        let r = read_stl(&binary(&[TRI, TRI])).unwrap();
        assert_eq!((r.mesh.triangle_count(), r.dropped), (2, 0));
        assert_eq!(r.mesh.triangles[0], TRI);
    }

    #[test]
    fn binary_whose_header_says_solid_is_still_binary() {
        let mut b = binary(&[TRI]);
        b[..5].copy_from_slice(b"solid");
        assert_eq!(read_stl(&b).unwrap().mesh.triangle_count(), 1);
    }

    #[test]
    fn reads_ascii_with_name_and_odd_spacing() {
        let s =
            "  solid  my part\n facet normal 0 0 1\n  outer loop\n   vertex 0 0 0\n vertex 1 0 0\n\tvertex 0 1 0\n endloop\n endfacet\nendsolid\n";
        let r = read_stl(s.as_bytes()).unwrap();
        assert_eq!(r.mesh.triangles, vec![TRI]);
        assert_eq!(r.name.as_deref(), Some("my part"));
    }

    #[test]
    fn drops_bad_triangles_and_keeps_the_rest() {
        let nan = [[f32::NAN, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
        let flat = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]];
        let r = read_stl(&binary(&[TRI, nan, flat])).unwrap();
        assert_eq!((r.mesh.triangle_count(), r.dropped), (1, 2));
        let s = "solid\nfacet\nvertex 0 0 0\nvertex 1 0\nvertex 0 1 0\nendfacet\nfacet\nvertex 0 0 0\nendfacet\nendsolid";
        let r = read_stl(s.as_bytes()).unwrap();
        assert_eq!((r.mesh.triangle_count(), r.dropped), (0, 2));
    }

    #[test]
    fn hostile_files_are_errors_not_panics() {
        let mut huge = vec![0u8; HEADER];
        huge.extend_from_slice(&u32::MAX.to_le_bytes());
        let mut truncated = binary(&[TRI, TRI]);
        truncated.truncate(truncated.len() - 10);
        for bytes in [vec![], vec![0u8; 10], vec![0xff; 83], huge, truncated, b"solid\nendsolid".to_vec(), vec![0xff; 1000]] {
            assert!(read_stl(&bytes).is_err(), "{} bytes", bytes.len());
        }
    }

    #[test]
    fn count_over_the_cap_is_refused_before_allocating() {
        let count = (MAX_TRIANGLES + 1) as u32;
        let mut b = vec![0u8; HEADER];
        b.extend_from_slice(&count.to_le_bytes());
        // Claim the table is there without building it: only the length check sees it.
        assert!(matches!(read_binary(&b, count as usize), Err(IoError::TooLarge(_))));
    }
}
