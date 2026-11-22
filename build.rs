//! Records the build number (how many commits there are) and the commit it was built from,
//! for the version shown in the app ("v0.1.0 · build 24").

use std::process::Command;

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn main() {
    // Again after a commit or checkout, or a source change (which may make it "-dirty").
    for path in [
        ".git/HEAD",
        ".git/index",
        ".git/refs",
        "src",
        "vendor",
        "Cargo.toml",
    ] {
        println!("cargo:rerun-if-changed={path}");
    }
    let build = git(&["rev-list", "--count", "HEAD"]).unwrap_or_else(|| "0".into());
    let mut commit = git(&["rev-parse", "--short", "HEAD"]).unwrap_or_else(|| "unknown".into());
    // Built from uncommitted changes: the commit alone doesn't say what's in it.
    if git(&["status", "--porcelain", "--untracked-files=no"]).is_some_and(|s| !s.is_empty()) {
        commit += "-dirty";
    }
    println!("cargo:rustc-env=BUILD_NUMBER={build}");
    println!("cargo:rustc-env=BUILD_COMMIT={commit}");
}
