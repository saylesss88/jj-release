# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased](https://github.com/saylesss88/jj-release/compare/v0.10.0...HEAD)

### ✨ Added

- **(changelog)** add an option for keeping track of both independent workspace members and the root changelog tracking everything at the same time
- dependency bump propagation
- add check to ensure the same tag isn't created twice
- **(workspace)** write detect_member_tags function
- **(detect)** resolve workspace member globs when detecting versioning
- **(init)** detect tag prefixes from existing tags
- **(workspace)** detect crates/ workspaces with glob patterns
- **(changelog)** add --write flag to changelog subcommand
- **(changelog)** tie in support for a changelog per workspace member
- **(changelog)** support one changelog for each workspace or a single root changelog
- **(changelog)** add code to support ## Unreleased header
- **(bump)** use latest tag as the baseline for version calculation
- **(vcs)** fetch release tags directly via jj
- implement `sync_lockfile` method for `ManifestBackend`

### ♻️ Changed

- **(workspace)** move workspace.rs into workspace/ dir and split into submodules
- **BREAKING** split PreparedRelease into unified and independent plans
- **(changelog)** create a new jj_release_changelog workspace member and move the changelog logic there
- **(pipeline)** transition from registry checks to tag boundaries

### 🛠️ Fixed

- **(pipeline/prepare)** return None from prepare_release when an independent release has nothing to release
- **(test)** failing test missing parameter
- **(pipeline/release)** ensure that commits and tags still execute when publishing is off
- use statements calling old changelog module
- **(config)** move `manifest_backend` key to the top of `release.toml`
- use prepared.baseline_version for pr.rs and release.rs in the cli
- use statements & paths
- imports/use statements
- imports

### 🪲 Bug

- **(changelog)** every historical release gets todays date
- **(validate)** add check to make sure release.toml members match the members defined in Cargo.toml

## [0.10.0](https://github.com/saylesss88/jj-release/compare/v0.9.0...v0.10.0) - 2026-10-10

### ✨ Added

- **(aur/copr)** tie in required
- **(changelog)** add link to Comparing changes for changelog headers

### 🛠️ Fixed

- **(changelog)** read the actual Git remote URL from the repo
- **(tests/regestry)** add ignore so nix commands work
- temporarily add required = false to generated release.toml
- use statements, levels of function & Type calls
- publish.aur and publish.copr default to required
- **(workflow)** add write permissions for latest apple release

### 📚 Documentation

- **(config)** add doc comment for required field
- **(lib/README)** README edits
- **(lib/README)** add new examples to README
- **(example)** add bump_deps example
- **(example)** add workspace_info example
- **(lib/Cargo.toml)** fix metadata to point to correct docs.rs for the lib as well as have repository metadata lead to jj-release/lib

## [0.9.0](https://github.com/saylesss88/jj-release/compare/v0.8.0...v0.9.0) - 2026-09-29

### ✨ Added

- **(copr)** add optional local version bump
- change from [aur] to [publish.aur] to be more explicit, same for copr
- tie in publishing aur and copr to `run_publish`
- **(workspace)** replace call to `cargo-edit` with `toml_edit` removing the need for `cargo-edit` on independent workspaces
- add [copr] to default relaese.toml
- **(distro)** add AUR/COPR pre-flight checks and dry-run output
- **(release)** run distro pre-flight checks before publishing
- add Fedora COPR version bump and release
- add [aur] package ... to generated release.toml
- **(aur)** tie in aur publishing to dry-run and validate
- implement auto-publish new version for Arch Linux AUR
- add full option for features
- **(forge)** feature-gate forge backends
- arbitrary file replacements and native flake.nix bumping

### ♻️ Changed

- **(prepare)** slim down `prepare_release` with helper functions
- **(manifest)** split manifest.rs into per-backend modules
- **(manifest)** create bump module and move `apply_replacements`, and `bump_flake_nix` to `bump.rs`

### 🚀 Performance

- **(aur)** remove `ureq` dependency and use curl so the core logic is free from heavy network deps
- feature-gate ureq behind gitea and forgejo features and remove them from the default features list
- **(registry)** replace `ureq` with local `cargo search`

### 🛠️ Fixed

- **(copr)** dry run comment
- clippy lints, correct paths
- remove aur output from validate function
- **(manifest)** preserve package.json key order and trailing newline
- move WorkspaceManifest into manifest.rs
- commit ignore for registry tests

## [0.8.0](https://github.com/saylesss88/jj-release/compare/v0.7.0...v0.8.0) - 2026-09-24

### ✨ Added

- **(changelog)** add angular type prefixes to the default headers
- **(changelog)** add `emoji_mapping` to allow users to choose the emojis used in changelog headers
- **(changelog)** add `emoji_headers` option and `format_header` function
- **(changelog)** add flexible commit mapping, exclusions, and strict mode
- **(config)** add `strict` field to `ChangelogConfig` to catch non-conventional commits

### ♻️ Changed

- **(changelog)** add const for keep a changelog header to reduce duplication

### 🛠️ Fixed

- **(changelog)** add a space after **scope** changelog entries

### 📚 Documentation

- add logo to README

## [0.7.0](https://github.com/saylesss88/jj-release/compare/v0.6.0...v0.7.0) - 2026-09-23

### ✨ Added

- add generate_release_toml to lib for programmatic config generation

### ♻️ Changed

- **(binary)** move what can be moved into the lib
- add detect_workspace to lib and simplify init using it

### 🛠️ Fixed

- **(examples)** add warning to `run_release` example
- add blank line after changelog headers
- **(changelog)** add check to prevent duplicate headers

### 📚 Documentation

- add `init` example to demonstrate lib functionality
- add `run_release` full pipeline example
- add `detect_env` example
- add `prepare_release` example
- add `full_changelog` example

## [0.6.0](https://github.com/saylesss88/jj-release/compare/v0.5.2...v0.6.0) - 2026-09-22

### ✨ Added

- add Gitea forge support via shared GiteaCompatClient with Forgejo

### ♻️ Changed

- extract helper functions to slim down release_pipeline

### 🛠️ Fixed

- add --no-verify to preflight check
- doc comment in lib.rs to use jj_release_core
- **(README)** correct version and exit codes

### 🪲 Bug

- **(changelog)** prepend_to_file was duplicating the header info per release

## [0.5.1](https://github.com/saylesss88/jj-release/compare/v0.5.0...v0.5.1) - 2026-09-22

### ♻️ Changed

- **(pipeline)** move into pipeline/ with sub-modules
- migrate error handling from anyhow to thiserror
- split into jj_release library crate and jj-release CLI crate

### 🛠️ Fixed

- **(jj)** remove `#[doc(hidden)]` from useful public types
- **(pipeline)** lib code shouldn't call process::exit
- **(forge)** move helper functions only used in tests into the test module and drop unused ones
- tests to use new default of not publishing to cargo

