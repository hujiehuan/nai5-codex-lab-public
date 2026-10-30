fn main() {
    // Software revision is independent of the downloaded codex release.
    let revision = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"]).output().ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_owned())
        .filter(|s| s.len() == 40 && s.bytes().all(|b| b.is_ascii_hexdigit()))
        .unwrap_or_else(|| "unknown".into());
    println!("cargo:rustc-env=APP_BUILD_REVISION={revision}");
    for reference in ["HEAD", "refs/heads/main", "packed-refs"] {
        if let Ok(output) = std::process::Command::new("git").args(["rev-parse", "--git-path", reference]).output() {
            if output.status.success() {
                println!("cargo:rerun-if-changed={}", String::from_utf8_lossy(&output.stdout).trim());
            }
        }
    }
    tauri_build::build()
}
