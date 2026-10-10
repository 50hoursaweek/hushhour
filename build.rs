//! Give every build a number, shown in the menu's About section.
//!
//! `build-number` in the project root is read, incremented and written back
//! each time the crate is rebuilt because something under `src` or `assets`
//! changed, so a running copy can always be matched to a build.

use std::fs;
use std::path::Path;

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let path = root.join("build-number");
    let current: u64 = fs::read_to_string(&path).ok().and_then(|s| s.trim().parse().ok()).unwrap_or(0);
    let next = current + 1;
    // Best effort: a read-only checkout still builds, with the old number.
    let _ = fs::write(&path, format!("{next}\n"));
    println!("cargo:rustc-env=INKPAD_BUILD={next}");
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=assets");
    println!("cargo:rerun-if-changed=Cargo.toml");
}
