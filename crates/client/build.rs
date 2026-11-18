//! The commit this client was built from, for its tickets (`tickets.rs`) and its log.
fn main() {
    let head = std::process::Command::new("git").args(["rev-parse", "--short=10", "HEAD"]).output().ok().filter(|o| o.status.success());
    let commit = head.and_then(|o| String::from_utf8(o.stdout).ok()).map_or_else(|| "?".to_string(), |s| s.trim().to_string());
    let dirty = std::process::Command::new("git").args(["status", "--porcelain", "--untracked-files=no"]).output().ok().is_some_and(|o| !o.stdout.is_empty());
    println!("cargo:rustc-env=BYTEFIELD_COMMIT={commit}{}", if dirty { "+" } else { "" });
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    println!("cargo:rerun-if-changed=../../.git/refs/heads");
    println!("cargo:rerun-if-changed=../../.git/index");
}
