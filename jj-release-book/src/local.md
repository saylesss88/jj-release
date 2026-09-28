# Local releases

`jj-release` doesn't require CI. If you have `jj` on your PATH and are logged in
to crates.io (`cargo login`), you can run it directly (Recommended over CI):

```sh
jj new -m "Release: please"
jj-release
```

Running `jj-release` locally does the full pipeline:

1. Scans for the trigger commit since the last version tag
2. Walks commits and computes the semver bump from conventional commit types
3. Runs pre-flight checks (including `cargo-publish --dry-run`) to ensure the
   release won't fail midway
4. Writes a new section to `CHANGELOG.md`
5. Bumps the version in manifest
6. Creates a `chore: release vX.Y.Z` commit and tags it `vX.Y.Z` locally
7. Runs the actual `cargo publish`
8. Advances the bookmark and pushes to origin
9. Creates a GitHub release (if `create_release = true`)

The `CARGO_REGISTRY_TOKEN` environment variable is only needed in CI where
there's no credentials file.

Fail-Safe Design: `jj-release` delays irreversible remote actions (like
`git push`) until the very end. If a step like `cargo publish` fails, your
remote git repository remains completely untouched. Because you are using
Jujutsu, you can simply `jj abandon` the failed local release commit, fix the
underlying issue, and run `jj-release` again.
