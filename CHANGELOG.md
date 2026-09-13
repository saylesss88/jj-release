# Changelog

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
