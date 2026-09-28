# Your first release

`jj-release` requires zero configuration for your first release.

If your repository has no existing version tags, `jj-release` automatically
enters **First Release Mode**. It will:

1. Freeze your `Cargo.toml` version exactly as it is (e.g., `0.1.0`).
2. Generate a changelog from the very first commit in your repository.
3. Publish the release and create your initial version tag.

You do not need to manually create a baseline tag.

### Releasing 1.0.0

When you're ready to release 1.0.0, set `force` in `release.toml`:

```toml
[bump]
force = "major"
```

This overrides the automatic bump calculation regardless of commit types. Remove
it after the release.
