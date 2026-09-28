# Recovering from a failed release

If a release fails midway through (e.g., `cargo publish` fails due to registry
validation), you can cleanly revert the entire pipeline using Jujutsu's
operation log:

1. Find the operation right before you ran `jj-release` (look for the
   `Release: please` commit):

```bash
jj op log
```

2. Restore the repo to that operation:

```bash
jj op restore <operation_hash>
```

3. Discard the mutated files (like `Cargo.toml` bumps) from your working copy:

```bash
jj restore
```

<!-- prettier-ignore -->
> [!NOTE]
> `jj-release` runs `cargo publish --dry-run` as a pre-flight check, but this
> only validates local compilation. Server-side registry rejections (like
> invalid URLs or missing permissions) will still cause the pipeline to fail
> during the final push.
