# AUR

`jj-release` can automatically update your AUR package after a successful
crates.io and Forge release. This step runs at the very end of the pipeline to
ensure the new source tarball is fully available for checksum calculation.

### Prerequisites

1. **Feature flag:** `jj-release` must be built with the `publish-aur` (or
   `full`) feature: `cargo install jj-release --features publish-aur`. The AUR,
   COPR, and Nix packages include it already.
2. **Tools:** `git` and `curl` must be on your `PATH`. `makepkg` is optional
   (see below), so this works from any Linux distro or macOS.
3. **Authentication:** your machine needs SSH access to `aur@aur.archlinux.org`,
   the same setup you'd use to push to the AUR by hand.

---

### Configuration

Add the `[aur]` table to your `release.toml`:

```toml
[publish.aur]
# Required: The exact name of your package on the AUR
package = "my-cli-tool"

# Optional: Override the default Git URL.
# Defaults to ssh://aur@aur.archlinux.org/{package}.git
# repo = "ssh://aur@aur.archlinux.org/custom-repo-name.git"
```

---

### Pipeline Execution

When AUR publishing is enabled, `jj-release` executes the following sequence
after all other release steps succeed:

1. Clones your AUR package repository into a temporary directory.

2. Sets `pkgver` in the `PKGBUILD` to the new version and resets `pkgrel` to
   `1`.

3. Downloads each remote source with `curl` and recomputes its checksums
   (`sha256sums` and/or `b2sums`). Downloads are retried briefly, since
   crates.io can take a few seconds to serve a newly published crate.

4. Regenerates `.SRCINFO` with `makepkg --printsrcinfo`. If `makepkg` isn't
   available, it updates the version, source, and checksum fields in `.SRCINFO`
   directly.
5. Commits the changes as `Update to <version>` and pushes to the AUR.

If the AUR package is already at the new version, this step is skipped, so
rerunning after a partial failure is safe.

---

### Limitations

- Only `sha256sums` and `b2sums` are supported. Other checksum arrays and
  architecture-specific sources (`source_x86_64`) produce a clear error.
- If you edit the `PKGBUILD` by hand, regenerate `.SRCINFO` yourself before
  pushing. Without `makepkg`, `jj-release` only updates version-related fields.

**Dry run**

`jj-release --dry-run` clones the AUR repo, applies the version bump and
checksum updates, and prints the staged diff without committing or pushing.
It still requires SSH access to the AUR and network access to download
sources for checksum calculation.
