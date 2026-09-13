use std::fs;
use std::path::Path;

use crate::error::Result;

/// Quote a value for safe interpolation into a `/bin/sh` command line.
pub fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// Quote a path for safe interpolation into a `/bin/sh` command line.
pub fn shell_quote_path(path: &Path) -> String {
    shell_quote(&path.to_string_lossy())
}

/// Quote a value for embedding in an AppleScript string literal.
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

pub fn set_mode(path: &Path, mode: u32) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
    Ok(())
}

/// Return the last `lines` lines of `path`, or an empty string when the file
/// does not exist yet (a freshly installed service has no log).
pub fn tail_file(path: &Path, lines: usize) -> Result<String> {
    let data = match fs::read(path) {
        Ok(data) => data,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(String::new()),
        Err(err) => return Err(err.into()),
    };

    let text = String::from_utf8_lossy(&data);
    let parts: Vec<&str> = text.split('\n').collect();
    let start = parts.len().saturating_sub(lines);
    Ok(parts[start..].join("\n"))
}
