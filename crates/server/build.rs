//! Embeds build metadata (git commit and rustc version) into `nova-server` at compile time.
//!
//! Both lookups degrade to `"unknown"` on failure (for example, when building from a source
//! archive with no `.git` directory) rather than failing the build.

use std::process::Command;

fn main() {
    println!("cargo:rustc-env=NOVA_BUILD_GIT_SHA={}", git_sha());
    println!("cargo:rustc-env=NOVA_BUILD_RUSTC_VERSION={}", rustc_version());
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../../.git/HEAD");
}

fn git_sha() -> String {
    Command::new("git")
        .args(["rev-parse", "--short=8", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|sha| sha.trim().to_owned())
        .unwrap_or_else(|| "unknown".to_owned())
}

fn rustc_version() -> String {
    let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".to_owned());
    Command::new(rustc)
        .arg("--version")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .and_then(|full| full.split_whitespace().nth(1).map(str::to_owned))
        .unwrap_or_else(|| "unknown".to_owned())
}
