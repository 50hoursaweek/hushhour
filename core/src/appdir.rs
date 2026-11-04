//! The app's own directory under an XDG base (config, data). The app was called Inkpad until
//! 0.2.0 and HushHour until 0.7.0; a directory left under an old name is renamed on first use,
//! so settings, the recent-files list and the user's music carry over.

use std::path::{Path, PathBuf};

/// The directory name since 0.7.0.
pub const NAME: &str = "slowpage";
/// Earlier names, newest first.
pub const OLD_NAMES: [&str; 2] = ["hushhour", "inkpad"];

/// `base/slowpage`, after renaming the newest old directory to it when only that exists.
/// When the rename fails, the old directory is used as it is: nothing is copied or left
/// half-moved, and the next launch tries again.
pub fn resolve(base: &Path) -> PathBuf {
    let new = base.join(NAME);
    if new.exists() {
        return new;
    }
    for old in OLD_NAMES.map(|n| base.join(n)) {
        if old.is_dir() {
            return match std::fs::rename(&old, &new) {
                Ok(()) => new,
                Err(_) => old,
            };
        }
    }
    new
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("slowpage-appdir-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn hushhour_directory_moves_with_its_files() {
        let b = base("hushhour");
        std::fs::create_dir(b.join("hushhour")).unwrap();
        std::fs::write(b.join("hushhour/preferences"), "theme = dark\n").unwrap();
        std::fs::create_dir(b.join("inkpad")).unwrap();
        assert_eq!(resolve(&b), b.join("slowpage"));
        assert_eq!(std::fs::read_to_string(b.join("slowpage/preferences")).unwrap(), "theme = dark\n");
        assert!(!b.join("hushhour").exists());
        assert!(b.join("inkpad").exists(), "only the newest old directory is taken");
        std::fs::remove_dir_all(&b).unwrap();
    }

    #[test]
    fn inkpad_directory_moves_when_it_is_the_only_one() {
        let b = base("inkpad");
        std::fs::create_dir(b.join("inkpad")).unwrap();
        assert_eq!(resolve(&b), b.join("slowpage"));
        assert!(b.join("slowpage").is_dir() && !b.join("inkpad").exists());
        std::fs::remove_dir_all(&b).unwrap();
    }

    #[test]
    fn an_existing_slowpage_directory_wins() {
        let b = base("both");
        std::fs::create_dir(b.join("slowpage")).unwrap();
        std::fs::create_dir(b.join("hushhour")).unwrap();
        assert_eq!(resolve(&b), b.join("slowpage"));
        assert!(b.join("hushhour").exists(), "left alone");
        std::fs::remove_dir_all(&b).unwrap();
    }

    #[test]
    fn nothing_there_gives_the_new_name() {
        let b = base("fresh");
        assert_eq!(resolve(&b), b.join("slowpage"));
        assert!(!b.join("slowpage").exists(), "created only when something is saved");
        std::fs::remove_dir_all(&b).unwrap();
    }
}
