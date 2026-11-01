# Release process

Padfire's public default branch is `main`. A release branch may be used for
staging, but a version tag must point to a commit that has been reconciled into
`main` and passed the same CI checks.

## Before a release

1. Update `VER` in `padfire.py`, add a dated version section to
   `CHANGELOG.md`, and check all README/packaging references.
2. Run the commands in [CONTRIBUTING.md](CONTRIBUTING.md).
3. Review the complete diff for configuration/data migration, shell execution,
   local IPC, permissions, dependencies, packaging and documentation changes.
4. Verify that the root `LICENSE` covers Padfire-authored source and docs.
   Third-party dependencies and bundled assets retain their own licenses.
5. Complete physical Launchpad Mini MK1/MK2 input, page, pad, LED and
   unplug/replug testing. Verify actual playback, stop, looping/retrigger and
   monitor routing. Record what was tested and what was not.
6. Confirm the exact candidate commit has a successful GitHub Actions run.
7. Merge the candidate into `main` through a reviewed pull request. Resolve
   branch-history conflicts deliberately; do not force-push or retarget a
   conflict-heavy release PR merely to make the graph look linear.

## Tag and publish

After the candidate is on `main`:

```bash
git checkout main
git pull --ff-only
git status --short
python3 padfire.py --version
git tag -a v2.4.0 -m "Padfire v2.4.0"
git push origin v2.4.0
gh release create v2.4.0 --verify-tag --title "Padfire v2.4.0" \
  --notes-file /path/to/release-notes.md
```

Use the actual version and release-notes path for each release. Do not tag a
dirty worktree or a feature branch. Confirm the tag resolves to the intended
main commit and the tag-triggered CI run passes.

## Distribution scope

The Linux release is a source release for GTK3/Python on Linux. GitHub supplies
source archives; no signed binary package, Flatpak or distro package is
currently produced by this repository.

The native Windows port is a separate draft and has its own PySide/Qt
redistribution and LGPL review requirements. A Linux release does not imply
that the Windows draft is ready to publish.
