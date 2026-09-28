# Fixing commit messages after a release

If you need to fix a typo in a commit message after a release is finalized
(which marks the commit as immutable), rewriting the message will detach the Git
tag. Because changelog generators rely on Git tags to group history, you must
re-anchor the tag to the new timeline before regenerating the changelog.

1. **Rewrite the commit message:**

```bash
jj --ignore-immutable desc -r <commit_rev> -m 'fix: your new message'
```

(Note: use single quotes `'` if your message contains backticks so your shell
doesn't execute them). 2. Find the new release commit hash: Because history is
rewritten, the release commit generated a new hash. Find it using `jj log`. 3.
Re-anchor the release tag (i.e. The commit `chore: release vX.Y.Z`):

```bash
jj tag set vX.Y.Z -r <NEW_RELEASE_HASH> --allow-move
```

(Note: the further back in history you go, the more tags you'll have to
re-anchor) 4. Regenerate your changelog. The generator will now traverse the
corrected timeline and parse the updated messages.

```bash
rm CHANGELOG.md
jj-release changelog --full -o CHANGELOG.md
```

5. Push/Force-push the corrected history. Since you altered public history,
   force- push the updated branch and tag back to your remote.

```bash
# Push the corrected bookmark (no force need if jj handles the move)
jj git push

# Force-push each re-anchored tag via git directly
# (jj git push doesn't support --force)
git push origin vX.Y.Z --force
```

Or if you have multiple tags to re-anchor, push them all:

```bash
jj git push # pushes the bookmark
# then for each re-anchored tag:
git push origin v0.1.0 --force
git push origin v0.2.0 --force
# etc.
```

Force-pushing is generally considered bad practice in collaborative projects
since others may have fetched the old tags. For a solo project it's fine, just
keep this in mind..
