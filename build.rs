//! Give every build a number, shown in the menu's About section.
//!
//! `build-number` in the project root is read, incremented and written back
//! each time the crate is rebuilt because something under `src` or `assets`
//! changed, so a running copy can always be matched to a build. For
//! reproducible builds (Flathub, CI) set `HUSHHOUR_BUILD` in the environment: it
//! is embedded as is and the file is left alone.

use std::fs;
use std::path::Path;

fn main() {
    println!("cargo:rerun-if-env-changed=HUSHHOUR_BUILD");
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=assets");
    println!("cargo:rerun-if-changed=Cargo.toml");

    if let Ok(fixed) = std::env::var("HUSHHOUR_BUILD") {
        println!("cargo:rustc-env=HUSHHOUR_BUILD={fixed}");
        return;
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let path = root.join("build-number");
    let current: u64 = fs::read_to_string(&path).ok().and_then(|s| s.trim().parse().ok()).unwrap_or(0);
    let next = current.saturating_add(1);
    // Embed the incremented number only if it could be recorded; a read-only tree
    // keeps reporting the number it has.
    let embedded = match fs::write(&path, format!("{next}\n")) {
        Ok(()) => next,
        Err(_) => current,
    };
    println!("cargo:rustc-env=HUSHHOUR_BUILD={embedded}");
}
