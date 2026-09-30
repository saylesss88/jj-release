# Fedora COPR

`jj-release` can build a source RPM for each release and submit it to your
[COPR](https://copr.fedorainfracloud.org) project. It runs at the end of the
pipeline, after the crates.io publish, since the RPM is built from the published
crate.

## Prerequisites

1. **Feature flag:** `jj-release` must be built with the `publish-copr` (or
   `full`) feature: `cargo install jj-release --features publish-copr`.
2. **A Fedora (or other RPM-based) machine** with:

```sh
   sudo dnf install copr-cli rpm-build cargo-rpm-macros curl tar xz cargo
```

3. **COPR API access:** log in at
   [copr.fedorainfracloud.org/api](https://copr.fedorainfracloud.org/api), copy
   the configuration block, and save it to `~/.config/copr`.
4. **An existing COPR project** with the chroots you want to build for.

---

## The spec file

Keep your RPM spec in your repository. `jj-release` builds from a temporary copy
with the new version and never modifies the original. The spec must have:

- `%global crate <crate-name>`: used to download the published `.crate`
- a `Version:` line: replaced with the new version
- `SourceN: vendor.tar.xz`: the vendored dependencies, since COPR builders have
  no network access

A spec generated with `rust2rpm` and adjusted for vendoring works as is:

```spec
Source0:        %{crates_source}
Source1:        vendor.tar.xz

%prep
%autosetup -n %{crate}-%{version} -p1 -a 1
%cargo_prep -v vendor
```

Remove `%generate_buildrequires` / `%cargo_generate_buildrequires`, since
dependencies come from the vendor tarball instead of Fedora packages. To build
with Cargo features, pass them to the macros, e.g. `%cargo_build -f full` and
`%cargo_install -f full`.

<!-- prettier-ignore -->
> [!NOTE]
> If you want the local `.spec` version to stay in sync with each release, set
> `update_local = true` in your `[publish.copr]` config:
>
> ```toml
> [publish.copr]
> project = "sayless88/jj-release"
> spec = "copr/rust-jj-release.spec"
> update_local = true
> required = false
> ```
>
> Without this, only the temporary copy used to build the SRPM is updated, the
> spec file in your repository is left unchanged.

> `required` controls whether an COPR failure aborts the release or just warns.
> Set `required = true` if you want a failure to be treated as fatal.

---

## Configuration

Add a `[copr]` table to your `release.toml`:

```toml
[publish.copr]
# Required: your COPR project as owner/project
project = "your-username/my-cli-tool"
required = false

# Required: path to the spec, relative to the repo root
spec = "packaging/rust-my-cli-tool.spec"

# Optional: wait for the COPR build to finish (default: false)
# wait = true
```

---

## What it does

After all other release steps succeed, `jj-release`:

1. Copies the spec to a temporary directory and sets `Version:` to the new
   version.
2. Downloads the published `.crate` from crates.io with `curl`, retrying briefly
   while the CDN catches up.
3. Extracts it, runs `cargo vendor`, and packs the result as `vendor.tar.xz`.
4. Builds a source RPM with `rpmbuild -bs`.
5. Submits it with `copr-cli build`. By default it doesn't wait for the build;
   set `wait = true` to follow it and have failures reported.

A COPR failure is reported as a warning rather than failing the release, since
the crates.io publish, tag, and forge release have already happened.

### Limitations

- Pre-release versions (e.g. `1.0.0-rc.1`) are not supported yet, since RPM
  versions can't contain `-`.
- Skipped when `publish.cargo = false`, since the RPM is built from the
  crates.io release.
- Skipped for workspaces with independent versioning.

### Checking your setup

`jj-release validate` checks that the required tools are installed, the spec has
the required fields, `cargo-rpm-macros` is available, and `copr-cli` is
authenticated. The same checks run automatically before every release, so a
broken setup fails before anything is published.

`jj-release --dry-run` lists the COPR submission a release would make, without
building or submitting anything.
