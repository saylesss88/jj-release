# Introduction

<p align="center">
  <img src="https://raw.githubusercontent.com/saylesss88/jj-release/main/assets/jj-release-lockup.svg"
       alt="jj-release: semantic releases & changelogs for jj" width="400">
</p>

Automated semantic releases and changelog generation for
[Jujutsu](https://github.com/jj-vcs/jj) repositories.

Unlike tools built around mutable Git branches (like `release-plz` or
`cargo-release`), `jj-release` is designed natively for `jj`. It uses `jj`'s
revset language to walk commit history, operates on movable bookmarks, and uses
a commit-message trigger that fits naturally into the `jj` workflow.
