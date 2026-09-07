//! Build glue (manual §2.4, ADR-0007).
//!
//! `build.rs` owns the C++ adapter; CMake is retired to a standalone linkage
//! check off the build path. One build system, one dependency graph, one
//! `cargo build`.

use std::path::PathBuf;

fn main() {
    // Non-Windows: nothing to build. The core still compiles, so `cargo check`
    // stays useful on a non-Windows machine.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        println!("cargo:warning=skipping the WinFsp adapter: target is not Windows");
        return;
    }

    let winfsp = winfsp_dir();
    println!("cargo:rerun-if-changed=../winfsp-adapter");
    println!("cargo:rerun-if-env-changed=WINFSP_DIR");

    cc::Build::new()
        .cpp(true)
        .std("c++20")
        .include(format!("{winfsp}/inc"))
        .include("../winfsp-adapter/include")
        .file("../winfsp-adapter/src/host.cpp")
        .file("../winfsp-adapter/src/callbacks.cpp")
        .compile("space_winfsp_adapter");

    println!("cargo:rustc-link-search=native={winfsp}/lib");
    println!("cargo:rustc-link-lib=dylib=winfsp-x64");

    // The delay-load pair. `winfsp-x64.dll` lives in
    // `C:\Program Files (x86)\WinFsp\bin`, NOT System32, so a direct link
    // builds cleanly and fails at launch with "winfsp-x64.dll was not found".
    // With delay-loading plus `FspLoad(nullptr)` as the first call in
    // space_adapter_mount, WinFsp locates its own DLL through its registry
    // entry.
    println!("cargo:rustc-link-lib=delayimp");
    println!("cargo:rustc-link-arg=/DELAYLOAD:winfsp-x64.dll");
}

/// Locate the WinFsp SDK: the registry first, then the default install path.
fn winfsp_dir() -> String {
    if let Ok(dir) = std::env::var("WINFSP_DIR") {
        return dir.replace('\\', "/");
    }

    if let Some(dir) = winfsp_dir_from_registry() {
        return dir;
    }

    let default = PathBuf::from(r"C:\Program Files (x86)\WinFsp");
    if default.join("inc").is_dir() {
        return default.to_string_lossy().replace('\\', "/");
    }

    // Fail loudly with the fix, rather than emitting a link error 200 lines
    // later that names a header nobody recognises.
    panic!(
        "WinFsp SDK not found. Install WinFsp (winget install -e --id WinFsp.WinFsp) \
         or set WINFSP_DIR to its install directory."
    );
}

/// `HKLM\SOFTWARE\WOW6432Node\WinFsp\InstallDir`, read via `reg query` so the
/// build script needs no registry crate.
///
/// A `reg query` line looks like:
///
/// ```text
///     InstallDir    REG_SZ    C:\Program Files (x86)\WinFsp\
/// ```
///
/// The value is everything after `REG_SZ` and may itself contain spaces, so it
/// is taken by offset rather than by splitting on whitespace.
fn winfsp_dir_from_registry() -> Option<String> {
    const KEYS: [&str; 2] = [r"HKLM\SOFTWARE\WOW6432Node\WinFsp", r"HKLM\SOFTWARE\WinFsp"];

    for key in KEYS {
        let Ok(out) = std::process::Command::new("reg")
            .args(["query", key, "/v", "InstallDir"])
            .output()
        else {
            continue;
        };
        if !out.status.success() {
            continue;
        }

        let text = String::from_utf8_lossy(&out.stdout);
        for line in text.lines() {
            if !line.contains("InstallDir") {
                continue;
            }
            let Some(idx) = line.find("REG_SZ") else {
                continue;
            };
            let value = line[idx + "REG_SZ".len()..].trim();
            if value.is_empty() {
                continue;
            }
            let dir = value.trim_end_matches('\\').replace('\\', "/");
            if PathBuf::from(&dir).join("inc").is_dir() {
                return Some(dir);
            }
        }
    }
    None
}
