use std::path::PathBuf;

/// Copy llama-server sidecar binary and its shared libraries from
/// `src-tauri/binaries/` into the Cargo output directory so the Tauri
/// shell plugin can find them at runtime.
///
/// Tauri's shell plugin resolves `sidecar("llama-server")` as
/// `{exe_dir}/llama-server`, where `exe_dir` in dev mode is
/// `target/{profile}/`.  We place the binary and its dylibs there.
fn copy_sidecar_to_output() {
    let out_dir = std::env::var("OUT_DIR").unwrap();
    // OUT_DIR is e.g. target/debug/build/<crate>-<hash>/out
    // Walk up to the profile directory (target/debug or target/release).
    let profile_dir: PathBuf = PathBuf::from(&out_dir)
        .ancestors()
        .nth(3)
        .expect("OUT_DIR too shallow")
        .to_path_buf();

    let binaries_src = PathBuf::from("binaries");

    if !binaries_src.exists() {
        // Nothing downloaded yet — skip silently.
        return;
    }

    // Detect target triple for the sidecar filename suffix
    let target = std::env::var("TARGET").unwrap_or_default();
    let exe_suffix = if target.contains("windows") {
        ".exe"
    } else {
        ""
    };
    let src_name = format!("llama-server-{target}{exe_suffix}");
    let dst_name = format!("llama-server{exe_suffix}");

    let src_binary = binaries_src.join(&src_name);
    let dst_binary = profile_dir.join(&dst_name);
    if src_binary.exists() {
        // Only copy if source is newer or destination doesn't exist
        let should_copy = !dst_binary.exists() || {
            let src_modified = std::fs::metadata(&src_binary)
                .and_then(|m| m.modified())
                .ok();
            let dst_modified = std::fs::metadata(&dst_binary)
                .and_then(|m| m.modified())
                .ok();
            match (src_modified, dst_modified) {
                (Some(s), Some(d)) => s > d,
                _ => true,
            }
        };
        if should_copy {
            std::fs::copy(&src_binary, &dst_binary).expect("copy llama-server binary");
            println!(
                "cargo:warning=Copied sidecar: {src_name} -> {}",
                profile_dir.display()
            );
        }
    }

    // Copy shared libraries (.dylib / .so / .dll) needed by the sidecar.
    // They must be in the same directory as the binary (@loader_path rpath).
    // On Linux, sonames look like libfoo.so.0 — the extension is "0", not "so",
    // so we check whether the filename contains ".so" as a substring instead.
    if let Ok(entries) = std::fs::read_dir(&binaries_src) {
        for entry in entries.flatten() {
            let path = entry.path();
            let name_str = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            let is_lib = ext == "dylib" || ext == "dll" || name_str.contains(".so");
            if is_lib {
                let name = path.file_name().unwrap();
                let dst = profile_dir.join(name);
                let should_copy = !dst.exists() || {
                    let src_mod = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
                    let dst_mod = std::fs::metadata(&dst).and_then(|m| m.modified()).ok();
                    match (src_mod, dst_mod) {
                        (Some(s), Some(d)) => s > d,
                        _ => true,
                    }
                };
                if should_copy {
                    std::fs::copy(&path, &dst).expect("copy shared library");
                }
            }
        }
    }

    // Re-run this script if the source binaries change
    println!("cargo:rerun-if-changed=binaries");
}

fn main() {
    copy_sidecar_to_output();
    tauri_build::build();
}
