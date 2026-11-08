# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.11.0] - 2026-11-08

### ✨ Added

- **(changelog)** support one changelog for each workspace or a single root changelog

### ♻️ Changed

- **(changelog)** pass the release date from the lib
- **(changelog)** move what can be in the changelog crate into it
- **(changelog)** make render.rs more idiomatic and readable
- **(changelog)** reorganize module and extract workspace helpers
- **(changelog)** split up changelog.rs into sub-modules to make the code easier to read
- **(changelog)** create a new jj_release_changelog workspace member and move the changelog logic there

### 🛠️ Fixed

- **(cli)** add global = true to Cli fields
- **(changelog)** using .get(..11) is fragile rewrite has_changelog_title
- **(lib)** tie everything in and tweaks for working with complex workspaces in different modes
- **(changelog)** split header check into own function
- **(lib/Cargo.toml)** pruned features causing issue
- **(config)** move `manifest_backend` key to the top of `release.toml`

### 🪲 Bug

- independent + `Both` layout mismatch
- **(changelog)** every historical release gets todays date

### 📚 Documentation

- **(changelog)** add user facing examples
- **(changelog)** add basic README.md for changelog workspace

