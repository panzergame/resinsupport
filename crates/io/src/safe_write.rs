//! Crash-safe file writes: the new content goes to a temporary file next to the target, is
//! flushed to disk, and replaces the target in one rename. A crash or power cut at any moment
//! leaves the old file or the new one, never a torn mix. The previous version can be kept as
//! `<file>.bak`, written the same way.
//!
//! Ported from SolidCraft `crates/io/src/safe_write.rs` at `4bdffef0`, reduced: the backup is
//! always a copy (no hard link), and no stale temporary files are swept.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

/// The backup of a file written with `backup: true`.
pub fn backup_path(path: &Path) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(".bak");
    PathBuf::from(s)
}

fn temp_beside(path: &Path, tag: &str) -> PathBuf {
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "file".into());
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0);
    path.with_file_name(format!(".{name}.{tag}.{}.{nanos}.tmp", std::process::id()))
}

fn write_synced(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut f = OpenOptions::new().write(true).create_new(true).open(path)?;
    f.write_all(bytes)?;
    f.sync_all()
}

/// Flush the directory entry (the rename) to disk where the platform allows it.
fn sync_dir(path: &Path) {
    #[cfg(unix)]
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty())
        && let Ok(d) = fs::File::open(dir)
    {
        let _ = d.sync_all();
    }
    #[cfg(not(unix))]
    let _ = path;
}

/// Most links followed before a path is called a loop.
const MAX_LINK_HOPS: usize = 40;

/// The file a save has to replace: `path`, or where a symbolic link at `path` (or a chain of them)
/// leads, so the link survives the save.
fn resolve_links(path: &Path) -> std::io::Result<PathBuf> {
    let mut at = path.to_path_buf();
    for _ in 0..MAX_LINK_HOPS {
        if !fs::symlink_metadata(&at).is_ok_and(|m| m.file_type().is_symlink()) {
            return Ok(at);
        }
        let to = fs::read_link(&at)?;
        at = if to.is_absolute() { to } else { at.parent().map(|d| d.join(&to)).unwrap_or(to) };
    }
    Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, format!("{}: too many levels of symbolic links", path.display())))
}

/// Give the file that replaces the old one the permissions it had: a private file must not come
/// back readable by everyone.
fn keep_permissions(old: &Option<fs::Permissions>, new: &Path) {
    if let Some(p) = old {
        let _ = fs::set_permissions(new, p.clone());
    }
}

/// The waits between attempts at a rename Windows refuses (an antivirus or indexer holding a file
/// written a moment earlier): doubling from 1 ms, then 250 ms at a time, about 2 s in all.
const RENAME_WAITS_MS: [u64; 15] = [1, 2, 4, 8, 16, 32, 64, 128, 250, 250, 250, 250, 250, 250, 250];

fn replace_synced(source: &Path, destination: &Path) -> std::io::Result<()> {
    let mut result = fs::rename(source, destination);
    // Access denied (5) or a sharing violation (32); a read-only target is refused for good.
    let read_only = fs::metadata(destination).is_ok_and(|m| m.permissions().readonly());
    if !cfg!(windows) || read_only {
        return result;
    }
    for ms in RENAME_WAITS_MS {
        if !result.as_ref().is_err_and(|e| matches!(e.raw_os_error(), Some(5 | 32))) {
            return result;
        }
        std::thread::sleep(std::time::Duration::from_millis(ms));
        result = fs::rename(source, destination);
    }
    result
}

/// Write `bytes` to `path` atomically; with `backup`, the file being replaced is kept as
/// `<path>.bak` first. A symbolic link at `path` is followed (the file it points to is replaced,
/// the link stays), and the file keeps the permissions it had.
pub fn write_atomic(path: &Path, bytes: &[u8], backup: bool) -> std::io::Result<()> {
    let path = resolve_links(path)?;
    let permissions = fs::metadata(&path).ok().filter(|m| m.is_file()).map(|m| m.permissions());
    let tmp = temp_beside(&path, "new");
    if let Err(e) = write_synced(&tmp, bytes) {
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }
    keep_permissions(&permissions, &tmp);
    if backup && path.is_file() {
        // Copied, not moved, so the target exists at every moment.
        let bak_tmp = temp_beside(&path, "bak");
        let r = fs::read(&path)
            .and_then(|old| write_synced(&bak_tmp, &old))
            .map(|_| keep_permissions(&permissions, &bak_tmp))
            .and_then(|_| replace_synced(&bak_tmp, &backup_path(&path)));
        if let Err(e) = r {
            let _ = fs::remove_file(&bak_tmp);
            let _ = fs::remove_file(&tmp);
            return Err(e);
        }
    }
    if let Err(e) = replace_synced(&tmp, &path) {
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }
    sync_dir(&path);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_save_replaces_the_file_and_keeps_the_old_one_as_its_backup() {
        let dir = std::env::temp_dir().join(format!("resin-io-safe-write-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.rsup");
        write_atomic(&path, b"one", true).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"one");
        assert!(!backup_path(&path).exists(), "nothing to back up the first time");
        write_atomic(&path, b"two", true).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"two");
        assert_eq!(fs::read(backup_path(&path)).unwrap(), b"one");
        let left: Vec<_> = fs::read_dir(&dir).unwrap().filter_map(|e| e.ok()).filter(|e| e.file_name().to_string_lossy().ends_with(".tmp")).collect();
        assert!(left.is_empty(), "no temporary file is left behind");
        assert!(write_atomic(&dir.join("missing").join("b.rsup"), b"x", true).is_err());
        fs::remove_dir_all(dir).ok();
    }
}
