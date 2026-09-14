use std::fs;
use std::path::Path;

use crate::error::Result;

/// Quote a value for safe interpolation into a `/bin/sh` command line.
#[cfg(target_os = "macos")]
pub fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// Quote a path for safe interpolation into a `/bin/sh` command line.
#[cfg(target_os = "macos")]
pub fn shell_quote_path(path: &Path) -> String {
    shell_quote(&path.to_string_lossy())
}

/// Quote a value for embedding in an AppleScript string literal.
#[cfg(target_os = "macos")]
pub fn apple_script_quote(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\\\""))
}

pub fn file_exists(path: &Path) -> bool {
    path.exists()
}

pub fn copy_file(src: &Path, dst: &Path, mode: u32) -> Result<()> {
    let data = fs::read(src)?;
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(dst, data)?;
    set_mode(dst, mode)
}

/// Apply a unix permission mode. Windows has no equivalent — file access is
/// governed by ACLs inherited from the parent directory — so this is a no-op
/// there and callers can stay platform-agnostic.
#[cfg(target_os = "macos")]
pub fn set_mode(path: &Path, mode: u32) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub fn set_mode(_path: &Path, _mode: u32) -> Result<()> {
    Ok(())
}

/// Return the last `lines` lines of `path`, or an empty string when the file
/// does not exist yet (a freshly installed service has no log).
///
/// Reads backwards in bounded chunks instead of loading the whole file: the
/// service log grows without bound, so a full read on every status poll is
/// both slow and memory-hungry.
pub fn tail_file(path: &Path, lines: usize) -> Result<String> {
    use std::io::{Read, Seek, SeekFrom};

    let mut file = match fs::File::open(path) {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(String::new()),
        Err(err) => return Err(err.into()),
    };

    const CHUNK: u64 = 64 * 1024;
    let mut position = file.metadata()?.len();
    let mut newlines = 0usize;
    let mut chunks: Vec<Vec<u8>> = Vec::new();

    // Walk backwards until we have more newlines than requested lines.
    while position > 0 && newlines <= lines {
        let size = CHUNK.min(position) as usize;
        position -= size as u64;
        file.seek(SeekFrom::Start(position))?;
        let mut buffer = vec![0u8; size];
        file.read_exact(&mut buffer)?;
        newlines += buffer.iter().filter(|byte| **byte == b'\n').count();
        chunks.push(buffer);
    }

    let mut data = Vec::new();
    for chunk in chunks.iter().rev() {
        data.extend_from_slice(chunk);
    }

    let text = String::from_utf8_lossy(&data);
    let parts: Vec<&str> = text.split('\n').collect();
    let start = parts.len().saturating_sub(lines);
    Ok(parts[start..].join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn tail_file_returns_last_lines() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("log");
        let mut file = fs::File::create(&path).unwrap();
        for line in 1..=10 {
            writeln!(file, "line {line}").unwrap();
        }
        drop(file);

        // The trailing newline yields a final empty element, matching split.
        assert_eq!(tail_file(&path, 4).unwrap(), "line 8\nline 9\nline 10\n");
    }

    #[test]
    fn tail_file_spans_multiple_chunks() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("big.log");
        let mut file = fs::File::create(&path).unwrap();
        // Well past the 64 KiB chunk size so the backward reader loops.
        for row in 0..20_000 {
            writeln!(file, "row {row:06}").unwrap();
        }
        drop(file);

        let tail = tail_file(&path, 3).unwrap();
        let lines: Vec<&str> = tail.split('\n').collect();
        assert_eq!(lines[0], "row 019998");
        assert_eq!(lines[1], "row 019999");
        assert_eq!(lines[2], "");
    }

    #[test]
    fn tail_file_missing_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("does-not-exist.log");
        assert_eq!(tail_file(&missing, 5).unwrap(), "");
    }
}
