//! Recently opened and saved files, one path per line in the config directory.

use std::path::{Path, PathBuf};

const MAX: usize = 8;

fn path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("inkpad").join("recent"))
}

/// The list file is tiny; anything bigger is ignored rather than parsed.
const MAX_FILE_BYTES: u64 = 64 * 1024;

/// Load the list, dropping files that no longer exist.
pub fn load() -> Vec<PathBuf> {
    let Some(p) = path() else { return Vec::new() };
    let Some(text) = crate::prefs::read_bounded(&p, MAX_FILE_BYTES) else { return Vec::new() };
    text.lines().filter(|l| !l.is_empty()).map(PathBuf::from).filter(|p| p.is_file()).take(MAX).collect()
}

/// Move `file` to the front of the list and persist it. The list is updated even
/// when persisting fails; the error is returned so the UI can mention it.
pub fn push(list: &mut Vec<PathBuf>, file: &Path) -> Result<(), String> {
    // Absolute and resolved, so the same file opened via different paths appears once.
    let file = std::fs::canonicalize(file).unwrap_or_else(|_| file.to_owned());
    list.retain(|p| *p != file);
    list.insert(0, file);
    list.truncate(MAX);
    if let Some(p) = path() {
        // One path per line: names that cannot be written that way (a newline, or
        // bytes that are not UTF-8) stay in the menu for this session but are not persisted.
        let text: String = list
            .iter()
            .filter_map(|p| p.to_str())
            .filter(|s| !s.contains(['\n', '\r']))
            .map(|s| format!("{s}\n"))
            .collect();
        crate::prefs::write_atomic(&p, &text).map_err(|e| format!("Could not save the recent files list: {e}"))?;
    }
    Ok(())
}

/// A short form of the folder for the menu: the home directory becomes `~`.
pub fn folder_label(file: &Path) -> String {
    let dir = file.parent().unwrap_or(file);
    match dirs::home_dir() {
        Some(home) if dir.starts_with(&home) => {
            let rest = dir.strip_prefix(&home).unwrap_or(dir);
            if rest.as_os_str().is_empty() { "~".to_owned() } else { format!("~/{}", rest.display()) }
        }
        _ => dir.display().to_string(),
    }
}
