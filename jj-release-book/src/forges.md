# Forges

`jj-release` supports multiple forges for release creation and PR/MR opening:

| Forge            | `forge =`   | CLI required | Release | PR/MR  | Status                                                              |
| ---------------- | ----------- | ------------ | ------- | ------ | ------------------------------------------------------------------- |
| GitHub (default) | `"github"`  | `gh`         | ✓       | ✓      | ✓ tested                                                            |
| GitLab           | `"gitlab"`  | `glab`       | ✓       | ✓ (MR) | ✓ tested                                                            |
| Forgejo/Codeberg | `"forgejo"` | none (REST)  | ✓       | ✓      | ✓ tested on Codeberg, release creation not yet supported (see note) |
| Gitea            | `"gitea"`   | none (REST)  | ✓       | ✓      | implemented, untested                                               |
| None             | `"none"`    | –            | –       | –      | –                                                                   |

For GitLab, set `forge_url` if using a self-hosted instance:

```toml
[release]
forge = "gitlab"
forge_url = "https://gitlab.example.com"
```

For Forgejo/Codeberg, set `forge_url` to your instance and provide a token:

```toml
[release]
forge = "forgejo"
forge_url = "https://codeberg.org"
```

```sh
export FORGEJO_TOKEN=your-token
```

For Gitea, set `forge_url` to your instance and provide a token:

```toml
[release]
forge = "gitea"
forge_url = "https://gitea.com"
```

```sh
export GITEA_TOKEN=your-token
```

<!-- prettier-ignore -->
> [!NOTE]
> Forgejo/Codeberg and Gitea: tagging, pushing, and PR creation all work correctly.
> `create_release = true` is not yet supported. The release API requires
> annotated git tags but `jj` creates lightweight tags. Keep `create_release =
>  false`.

The token needs `repository` scope, create one at
`https://codeberg.org/user/settings/applications`.

| Forge   | Authentication                                                      |
| ------- | ------------------------------------------------------------------- |
| GitHub  | `gh auth login` locally or `GITHUB_TOKEN` in GitHub Actions         |
| GitLab  | `glab auth login` locally or the token/env mechanism used by `glab` |
| Forgejo | `FORGEJO_TOKEN` and `forge_url`                                     |
| Gitea   | `GITEA_TOKEN` and `forge_url`                                       |

