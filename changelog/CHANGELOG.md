# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased](https://github.com/saylesss88/jj-release/compare/v0.10.0...HEAD)

### ✨ Added

- **(changelog)** add an option for keeping track of both independent workspace members and the root changelog tracking everything at the same time
- **(changelog)** support one changelog for each workspace or a single root changelog

### ♻️ Changed

- **(changelog)** split up changelog.rs into sub-modules to make the code easier to read
- **(changelog)** create a new jj_release_changelog workspace member and move the changelog logic there

### 🛠️ Fixed

- **(lib/Cargo.toml)** pruned features causing issue
- **(config)** move `manifest_backend` key to the top of `release.toml`

### 🪲 Bug

- **(changelog)** every historical release gets todays date

### 📚 Documentation

- **(changelog)** add user facing examples
- **(changelog)** add basic README.md for changelog workspace

