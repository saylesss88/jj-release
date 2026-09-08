# Changelog

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
