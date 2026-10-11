//! Recently opened and saved files, one path per line in the config directory.

use std::path::{Path, PathBuf};

const MAX: usize = 8;

fn path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("inkpad").join("recent"))
}

/// Load the list, dropping files that no longer exist.
pub fn load() -> Vec<PathBuf> {
    let Some(text) = path().and_then(|p| std::fs::read_to_string(p).ok()) else { return Vec::new() };
    text.lines().map(PathBuf::from).filter(|p| p.is_file()).take(MAX).collect()
}

/// Move `file` to the front of the list and persist it.
pub fn push(list: &mut Vec<PathBuf>, file: &Path) {
    // Absolute and resolved, so the same file opened via different paths appears once.
    let file = std::fs::canonicalize(file).unwrap_or_else(|_| file.to_owned());
    list.retain(|p| *p != file);
    list.insert(0, file);
    list.truncate(MAX);
    if let Some(p) = path() {
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let text: String = list.iter().map(|p| format!("{}\n", p.display())).collect();
        let _ = std::fs::write(p, text);
    }
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
