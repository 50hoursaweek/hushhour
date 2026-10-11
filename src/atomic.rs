//! Atomic file replacement for documents and state files.
//!
//! The content goes to a temporary file created exclusively (so a pre-existing
//! path or symlink at that name can never be followed or overwritten), with a
//! private mode from the start, is flushed to disk, takes over the target's
//! permissions and is then renamed over the target; the directory is synced so
//! the rename itself survives a crash. On any failure the target is untouched
//! and the temporary file, which this call created, is removed. There is no
//! fallback to writing in place.
//!
//! A target that is a symbolic link is resolved first, so the real file is
//! replaced and the link keeps pointing at it.

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// How many colliding temporary names to try before giving up.
const ATTEMPTS: u32 = 32;

/// Replace `target` with `bytes` atomically.
pub fn write(target: &Path, bytes: &[u8]) -> io::Result<()> {
    // Write through symlinks to the real file, keeping the link intact.
    let target = match std::fs::canonicalize(target) {
        Ok(real) => real,
        Err(_) => target.to_path_buf(),
    };
    let dir = target.parent().filter(|d| !d.as_os_str().is_empty()).map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."));
    let name = target.file_name().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no file name"))?;

    let (temp, mut file) = create_exclusive(&dir, name)?;
    let result = (|| {
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        // Keep the target's permissions (a private document stays private; a shared one shared).
        if let Ok(meta) = std::fs::metadata(&target) {
            std::fs::set_permissions(&temp, meta.permissions())?;
        }
        std::fs::rename(&temp, &target)?;
        // Make the new directory entry durable too; not all filesystems support syncing a directory.
        if let Ok(d) = std::fs::File::open(&dir) {
            let _ = d.sync_all();
        }
        Ok(())
    })();
    if result.is_err() {
        // Only ever removes the file this call created exclusively.
        let _ = std::fs::remove_file(&temp);
    }
    result
}

/// Create a fresh temporary file next to the target with a name nobody else can
/// have prepared, owner-only readable.
fn create_exclusive(dir: &Path, name: &std::ffi::OsStr) -> io::Result<(PathBuf, std::fs::File)> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut last = io::Error::new(io::ErrorKind::AlreadyExists, "could not create a temporary file");
    for _ in 0..ATTEMPTS {
        let nonce = {
            let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(0);
            let c = COUNTER.fetch_add(1, Ordering::Relaxed);
            // Mixed so two processes starting in the same nanosecond still differ.
            (t ^ (u64::from(std::process::id()) << 32) ^ c.wrapping_mul(0x9E37_79B9_7F4A_7C15)) & 0xFFFF_FFFF_FFFF
        };
        let mut temp_name = std::ffi::OsString::from(".");
        temp_name.push(name);
        temp_name.push(format!(".{nonce:012x}.tmp"));
        let temp = dir.join(temp_name);
        match std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&temp) {
            Ok(file) => return Ok((temp, file)),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => last = e,
            Err(e) => return Err(e),
        }
    }
    Err(last)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("inkpad-atomic-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn replaces_content_and_keeps_permissions() {
        let dir = scratch("replace");
        let target = dir.join("doc.md");
        std::fs::write(&target, "old").unwrap();
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o640)).unwrap();
        write(&target, b"new").unwrap();
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "new");
        assert_eq!(std::fs::metadata(&target).unwrap().permissions().mode() & 0o777, 0o640);
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1, "no temporary file left behind");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn creates_new_files_privately() {
        let dir = scratch("new");
        let target = dir.join("fresh.md");
        write(&target, b"x").unwrap();
        assert_eq!(std::fs::metadata(&target).unwrap().permissions().mode() & 0o777, 0o600);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn failure_leaves_the_original_intact() {
        let dir = scratch("fail");
        let target = dir.join("doc.md");
        std::fs::write(&target, "precious").unwrap();
        // A read-only directory: the temporary file cannot be created.
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o500)).unwrap();
        let result = write(&target, b"new");
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        if std::env::var_os("USER").as_deref() == Some(std::ffi::OsStr::new("root")) {
            return; // root ignores directory permissions
        }
        assert!(result.is_err());
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "precious");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn writes_through_a_symlinked_target() {
        use std::os::unix::fs::symlink;
        let dir = scratch("symlink");
        let real = dir.join("real.md");
        let link = dir.join("link.md");
        std::fs::write(&real, "old").unwrap();
        symlink(&real, &link).unwrap();
        write(&link, b"new").unwrap();
        assert_eq!(std::fs::read_to_string(&real).unwrap(), "new");
        assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink(), "the link survives");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_planted_symlink_cannot_capture_the_temporary_file() {
        use std::os::unix::fs::symlink;
        let dir = scratch("planted");
        let victim = dir.join("victim.md");
        std::fs::write(&victim, "sentinel").unwrap();
        let target = dir.join("doc.md");
        std::fs::write(&target, "old").unwrap();
        // An attacker cannot know the nonce, but even a lucky guess fails: the name
        // is created exclusively, so plant links at every name of the old scheme.
        symlink(&victim, dir.join(format!(".doc.md.inkpad-{}.tmp", std::process::id()))).unwrap();
        write(&target, b"new").unwrap();
        assert_eq!(std::fs::read_to_string(&victim).unwrap(), "sentinel");
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "new");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
