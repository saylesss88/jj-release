//! Integration tests, run against a reall jj repo in a temp directory
// Note: these tests require `jj-release` and `jj` to be installed on PATH.
// Run `cargo install --path .` before running integration tests.

use std::{fs, path::Path, process::Command};

/// Initialize a colocated jj/git repo in `dir`.
fn init_repo(dir: &Path) {
    Command::new("git")
        .args(["init"])
        .current_dir(dir)
        .output()
        .unwrap();
    Command::new("jj")
        .args(["git", "init", "--colocate"])
        .current_dir(dir)
        .output()
        .unwrap();
    // Set identity so commits have an author.
    Command::new("jj")
        .args(["config", "set", "--repo", "user.email", "test@test.com"])
        .current_dir(dir)
        .output()
        .unwrap();
    Command::new("jj")
        .args(["config", "set", "--repo", "user.name", "Test User"])
        .current_dir(dir)
        .output()
        .unwrap();
}

fn jj_new(dir: &Path, message: &str) {
    Command::new("jj")
        .args(["new", "-m", message])
        .current_dir(dir)
        .output()
        .unwrap();
}

fn jj_release(dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new("jj-release")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap()
}

fn write_cargo_toml(dir: &Path, version: &str) {
    fs::write(
        dir.join("Cargo.toml"),
        format!(
            r#"[package]
name = "test-crate"
            version = "{version}"
            edition = "2024"
        "#
        ),
    )
    .unwrap();
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(dir.join("src/lib.rs"), "").unwrap();
}

#[test]
fn next_version_computes_minor_bump() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path());
    write_cargo_toml(dir.path(), "0.0.0");

    // Initial commit.
    jj_new(dir.path(), "feat: initial implementation");

    // Tag a baseline.
    Command::new("jj")
        .args(["tag", "set", "v0.0.0", "-r", "@-"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    jj_new(dir.path(), "feat: add new feature");

    let out = jj_release(dir.path(), &["next-version"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(stdout.trim(), "0.1.0");
}

#[test]
fn dry_run_shows_correct_version() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path());
    write_cargo_toml(dir.path(), "0.1.0");

    jj_new(dir.path(), "feat: something");
    Command::new("jj")
        .args(["tag", "set", "v0.1.0", "-r", "@"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    jj_new(dir.path(), "fix: patch something");
    jj_new(dir.path(), "Release: please");

    let out = jj_release(dir.path(), &["--dry-run"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success());
    assert!(stdout.contains("0.1.1"));
}

#[test]
fn no_trigger_exits_cleanly() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path());
    write_cargo_toml(dir.path(), "0.1.0");

    jj_new(dir.path(), "feat: something");
    Command::new("jj")
        .args(["tag", "set", "v0.1.0", "-r", "@-"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    jj_new(dir.path(), "fix: no trigger here");

    let out = jj_release(dir.path(), &["--dry-run"]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("No trigger commit found"));
}
