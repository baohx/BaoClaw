# Release Guide

## Version Policy

BaoClaw uses one monorepo version. The root `package.json` is the source of
truth, and every inner package manifest and lockfile uses the same version.

Use Semantic Versioning:

- Major: breaking core-to-gateway protocol changes
- Minor: backwards-compatible features
- Patch: backwards-compatible fixes

## Release Flow

1. Update the version in the root and inner package manifests with `npm version
X.Y.Z --no-git-tag-version`.
2. Update the matching `CHANGELOG.md` section and verify all package versions
   and lockfiles agree.
3. Run `npm run verify-all` and the gateway test commands.
4. Commit the version and changelog changes.
5. Create an annotated tag: `git tag -a vX.Y.Z -m "BaoClaw vX.Y.Z"`.
6. Push the commit and tag together: `git push origin HEAD vX.Y.Z`.

Do not create a tag before the release commit is reviewed. Existing release
tags are immutable; publish a new patch version when a release needs correction.

## Dry Run

To rehearse the flow, use a temporary branch and a patch version:

```bash
git switch -c release-dry-run
npm version 2.1.1 --no-git-tag-version
git diff -- package.json */package.json */package-lock.json
git switch -
git branch -D release-dry-run
```
