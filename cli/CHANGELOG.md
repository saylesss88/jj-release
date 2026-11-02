# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased](https://github.com/saylesss88/jj-release/compare/v0.10.0...HEAD)

### ✨ Added

- dependency bump propagation
- **(init)** detect tag prefixes from existing tags
- **(changelog)** add --write flag to changelog subcommand
- **(changelog)** tie in support for a changelog per workspace member
- **(changelog)** support one changelog for each workspace or a single root changelog
- **(changelog)** add code to support ## Unreleased header

### ♻️ Changed

- **BREAKING** split PreparedRelease into unified and independent plans
- **(main)** slim down fn run with helper functions
- **(changelog)** create a new jj_release_changelog workspace member and move the changelog logic there

### 🛠️ Fixed

- use statements calling old changelog module
- **(config)** move `manifest_backend` key to the top of `release.toml`
- use prepared.baseline_version for pr.rs and release.rs in the cli

## [0.10.0](https://github.com/saylesss88/jj-release/compare/v0.9.0...v0.10.0) - 2026-10-10

### ✨ Added

- **(changelog)** add link to Comparing changes for changelog headers

## [0.9.0](https://github.com/saylesss88/jj-release/compare/v0.8.0...v0.9.0) - 2026-09-29

### ✨ Added

- change from [aur] to [publish.aur] to be more explicit, same for copr
- add cargo.metadata for cargo-binstall
- **(distro)** add AUR/COPR pre-flight checks and dry-run output
- add Fedora COPR version bump and release
- **(aur)** tie in aur publishing to dry-run and validate
- implement auto-publish new version for Arch Linux AUR
- add full option for features
- **(forge)** feature-gate forge backends
- arbitrary file replacements and native flake.nix bumping

### ♻️ Changed

- **(pr)** move from implicit macro to a parameterized macro outside of the function
- **(prepare)** slim down `prepare_release` with helper functions
- **(release)** slim down prepare_release with helper functions
- **(manifest)** create bump module and move `apply_replacements`, and `bump_flake_nix` to `bump.rs`

### 🚀 Performance

- feature-gate ureq behind gitea and forgejo features and remove them from the default features list

## [0.8.0](https://github.com/saylesss88/jj-release/compare/v0.7.0...v0.8.0) - 2026-09-24

### ✨ Added

- **(changelog)** add flexible commit mapping, exclusions, and strict mode
- **(main)** add no_publish to the Cli struct & pass into run

### 🛠️ Fixed

- **(release)** add a y/n prompt for when cargo.publish = false to warn users and remind them

## [0.7.0](https://github.com/saylesss88/jj-release/compare/v0.6.0...v0.7.0) - 2026-09-23

### ♻️ Changed

- **(init)** use the new `generate_release_toml` in the init command to reduce duplication
- **(binary)** move what can be moved into the lib
- add detect_workspace to lib and simplify init using it

## [0.6.0](https://github.com/saylesss88/jj-release/compare/v0.5.2...v0.6.0) - 2026-09-22

### ✨ Added

- add Gitea forge support via shared GiteaCompatClient with Forgejo

### ♻️ Changed

- extract helper functions to slim down release_pipeline

## [0.5.2](https://github.com/saylesss88/jj-release/compare/v0.5.1...v0.5.2) - 2026-09-22

### 🛠️ Fixed

- explicitly define readme path for crates.io

## [0.5.1](https://github.com/saylesss88/jj-release/compare/v0.5.0...v0.5.1) - 2026-09-22

### ♻️ Changed

- migrate error handling from anyhow to thiserror
- split into jj_release library crate and jj-release CLI crate

### 🛠️ Fixed

- tests to use new default of not publishing to cargo

