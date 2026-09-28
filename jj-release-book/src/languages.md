# Other languages

`jj-release` supports multiple manifest formats via `manifest_backend`:

| Language   | `manifest_backend =` | Version file   | Publish command |
| ---------- | -------------------- | -------------- | --------------- |
| Rust       | `"cargo"` (default)  | `Cargo.toml`   | `cargo publish` |
| JavaScript | `"npm"`              | `package.json` | `npm publish`   |
| Golang     | `"go"`               | tag-only       | –               |

<!-- prettier-ignore -->
> [!NOTE]
> npm and Golang support is implemented but not yet battle-tested in production.
> Feedback welcome if you use `jj-release` with these languages.
