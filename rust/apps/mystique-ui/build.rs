//! Tauri build glue: embeds `tauri.conf.json`, the capabilities and the static `ui/`
//! directory. The failure path avoids `expect`/`panic` in this crate.
//!
//! `generate_context!` embeds the frontend at macro-expansion time, which cargo cannot
//! see. The fingerprint below makes a UI-only edit a real input to this crate, so
//! `cargo build` cannot silently serve stale assets.

#![forbid(unsafe_code)]

use std::fs;
use std::path::Path;

fn hash_tree(path: &Path, hash: &mut u64) {
    let Ok(entries) = fs::read_dir(path) else {
        return;
    };
    let mut paths: Vec<_> = entries.flatten().map(|entry| entry.path()).collect();
    paths.sort();
    for entry in paths {
        if entry.is_dir() {
            hash_tree(&entry, hash);
            continue;
        }
        let Ok(bytes) = fs::read(&entry) else {
            continue;
        };
        for byte in bytes {
            *hash ^= u64::from(byte);
            *hash = hash.wrapping_mul(1_099_511_628_211);
        }
    }
}

fn main() {
    println!("cargo:rerun-if-changed=ui");
    println!("cargo:rerun-if-changed=tauri.conf.json");

    let mut fingerprint = 1_469_598_103_934_665_603_u64;
    hash_tree(Path::new("ui"), &mut fingerprint);
    println!("cargo:rustc-env=MYSTIQUE_UI_FINGERPRINT={fingerprint:016x}");

    if let Err(error) = tauri_build::try_build(tauri_build::Attributes::default()) {
        eprintln!("tauri-build failed: {error:#}");
        std::process::exit(1);
    }
}
