# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.5.2] - 2026-09-21

### Fixed
- explicitly define readme path for crates.io

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.5.1] - 2026-09-21

### Changed
- **(pipeline)**move into pipeline/ with sub-modules
- error handling to use thiserror for the lib
- separate into lib and cli workspaces

### Fixed
- **(jj)**remove doc hiddden from useful public types
- **(pipeline)**lib code shouldn't call process::exit
- **(forge)**move helper functions only used in tests into the test module and drop unused ones
- tests to use new default of not publishing to cargo

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.5.0] - 2026-09-20

### Added
- **(pipeline)**implement zero-config first release mode
- **(cli)**add contextual hint when release trigger is missing
- **(pipeline)**wire publish pre-flight into validate command
- **(publish)**add dry-run check method to backends
- add latest_version_on_crates_io for registry-based version baseline
- add cargo-semver-check to validate command
- run cargo-semver-checks to auto-upgrade bump to Major on API breaking changes
- add semver_checks config option and validate check
- add crates.io version check to validate subcommand
- add crates.io preflight check to prevent duplicate version publishing
- add registry module with version_exists_on_crates_io check
- add read_name to manifest for reading crate name from Cargo.toml

### Changed
- **(release)**extract workspace publishing logic
- **(forge)**separate argument generation from execution
- **(pipeline)**stream validation results sequentially
- **(publish)**capture dry-run output and clarify validate logs
- **(pipeline)**slim down validate with helper functions
- **(forge)**create PrRequest struct to reduce arguments to create_pr in all locations & change call sites to match
- **(forge)**create 'post' helper method to simplify create_release & create_pr in impl ForgeBackend for ForgejoForge

### Fixed
- default cargo publish to false in init and config
- **(config)**set default to false for publishing to crates.io and make it opt-in
- **(detect)**add correct format for codeberg ssh
- **(validate)**treat missing version tag as success for first releases
- **(workspace)**ignore unpublished dependencies during topological sort
- **(cli)**re-enable clap help menu generation
- **(pipeline)**prevent remote git push if publish fails
- **(release)**crates.io already doesn't allow you to publish duplicate versions, remove check for it in run_publish
- **(lib)**make test_helpers module pub(crate)
- suppress cargo-semver-checks output on --dry-run
- add v1.0.0 message before bumping to major in validate
- add non_exhaustive to public structs that may change
- default to false for semver_checks_major_upgrade
- add v1.0.0 check before bumping to major
- add non_exhaustive to struct PublishConfig
- format Cargo.toml
- use statements, levels of function & Type calls
- use statements

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.4.0] - 2026-09-16

### Added
- **(pipeline)**create first tag if one doesn't exist rather than warning user
- implement ForgejoForge with REST API for releases and PRs
- add parse_remote_owner_repo for extracting owner/repo from git remote URLs

### Fixed
- **(pipeline)**validate check for CARGO_REGISTRY_TOKEN too strict
- call sites of member_bumps
- **(workspace)**in member_bumps add per-member tag prefixes when computing since on each member
- suppress unified version info in independent workspace dry-run output
- create per-member release commits and tags in independent workspace mode
- **(release)**skip workspace-level tag and version bump in independent mode

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.3.0] - 2026-09-15

### Added
- **(init)**use crate name for independent tags
- **(release)**wire per-member tagging into independent workspace release pipeline
- **(workspace)**add member_tag_name helper for per-member tag naming in independent workspaces
- add optional tag_prefix to WorkspaceMember for independent versioning
- **(init)**tie in detect_versioning
- add detect_versioning to auto-detect unified vs independent workspace versioning
- show per-member version bumps in dry-run for independent workspace versioning
- add --full flag to changelog subcommand for generating complete history
- add render_full_changelog for generating complete changelog history
- add member_bumps for per-member bump computation in independent workspaces
- add log_commits_for_path to JjBackend trait for path-filtered commit walking
- add bump_member_version via cargo set-version for independent workspace versioning
- **(workspace)**add member_versions using cargo_metadata for independent workspace versioning

### Changed
- **(main)**create commands module and move functions out of main
- extract run_publish helper to slim down release_pipeline
- extract MockBackend to shared test_helpers module

### Fixed
- remove unused dependencies
- **(changelog)**use Keep a Changelog header

## [0.2.0] - 2026-09-13

### Added
- add validate subcommand
- require version tag before release with helpful hint, add require_tag config option
- include conventional commit scopes in changelog output
- **(changelog)**add -o/--output flag to changelog subcommand for standalone file generation
- add preflight checks for forge CLI availability with CliError
- wire CliError into main with proper exit codes
- add CliError with exit codes and report function
- add init subcommand with auto-detection of forge, language, and workspace members
- **(main)**add Init to Subcommand & write init function
- add IO layer to detect module for forge, language, and tool detection
- add detect module with forge_from_url and language_from_files
- **(main)**add trigger detection and current version lines to release_pipeline
- **(jj)**add check_identity guard to JjBackend to either fail completely or succeed
- add to re-exports, add top-level doc comments
- wire workspace support into release pipeline
- add WorkspaceManifest implementing ManifestBackend for unified versioning
- add ordered_members topological sort for workspace publish order
- add workspace support
- start laying out forgejo support TODO
- implement resolve_bump to reduce duplication
- extract prepare_release into pipeline module
- wire PublishBackend through release pipeline
- add PublishBackend trait with NoPublish, CargoPublish, NpmPublish impls
- **(main)**tie in NpmManifest
- add NpmManifest for package.json version management
- add GoManifest and manifest_backend config field
- **(main)**update run() to construct the forge from config instead of github_release
- **(config)**add forge & forge_url to ReleaseConfig
- **(forge)**add GitLab support
- wire ForgeBackend through release pipeline and pr subcommand
- add GitHubForge implementation using gh CLI
- add ForgeBackend trait with NoForge noop implementation
- layout forge module with failing test
- add pr subcommand for opening release PRs

### Changed
- reduce args to print_changelog and print_next_version by using ReleaseContext
- replace github_release bool, with forge config field
- **(main)**create a lib and move sub-modules there
- wire ManifestBackend through release pipeline
- extract ManifestBackend trait with CargoManifest impl

### Fixed
- use statements and correct level for functions/types
- **(main)**add workspace description to output of --dry-run
- **(config)**test now defaults to github
- don't bump version on Pr's
- **(jj)**remove JJ_CONFIG= line in ShellBackend::run
- private/public items in crate

## [0.1.0] - 2026-09-08

### Added
- add new [changelog] section
- add prepend_to_file for changelog generation
- add render_changelog_section with keep-a-changelog format
- layout changelog module with tests
- add list_tags to JjBackend trait and implement latest_version_tag
- add Tag struct and highest_semver_tag function
- initial project scaffolding

### Changed
- make compute_bump a pure function taking &[CommitInfo]

### Fixed
- friction of failed publish
- wire in changelog generation
- compute bump from last version tag instead of root
