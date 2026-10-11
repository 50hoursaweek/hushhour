//! Recently opened and saved files, one path per line in the config directory.
//!
//! Updating the list and persisting it are separate steps, so the application can keep
//! the menu current without writing anything (tests, diagnostic runs).

use std::path::{Path, PathBuf};

const MAX: usize = 8;

/// Where the list is stored for this user, if a config directory exists.
pub fn path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("inkpad").join("recent"))
}

/// The list file is tiny; anything bigger is ignored rather than parsed.
const MAX_FILE_BYTES: u64 = 64 * 1024;

/// Load the user's list, dropping files that no longer exist.
pub fn load() -> Vec<PathBuf> {
    path().map(|p| load_from(&p)).unwrap_or_default()
}

/// Load the list stored at `store`, dropping files that no longer exist.
pub fn load_from(store: &Path) -> Vec<PathBuf> {
    let Some(text) = crate::prefs::read_bounded(store, MAX_FILE_BYTES) else { return Vec::new() };
    text.lines().filter(|l| !l.is_empty()).map(PathBuf::from).filter(|p| p.is_file()).take(MAX).collect()
}

/// Move `file` to the front of the in-memory list. Nothing is written.
pub fn remember(list: &mut Vec<PathBuf>, file: &Path) {
    // Absolute and resolved, so the same file opened via different paths appears once.
    let file = std::fs::canonicalize(file).unwrap_or_else(|_| file.to_owned());
    list.retain(|p| *p != file);
    list.insert(0, file);
    list.truncate(MAX);
}

/// Write `list` to `store`, one path per line. Names that cannot be written that way (a
/// newline, or bytes that are not UTF-8) stay in the menu for this session but are left out.
pub fn save_to(store: &Path, list: &[PathBuf]) -> Result<(), String> {
    let text: String = list
        .iter()
        .filter_map(|p| p.to_str())
        .filter(|s| !s.contains(['\n', '\r']))
        .map(|s| format!("{s}\n"))
        .collect();
    crate::prefs::write_atomic(store, &text).map_err(|e| format!("Could not save the recent files list: {e}"))
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
