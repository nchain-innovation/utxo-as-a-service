# Versioning and releases

## Scheme: one version for the whole repository

UaaS publishes two images — `nchain/innovation-uaas-web` and
`nchain/innovation-uaas-service` — but they are built from **one commit in one
repository** and deployed together by `docker-compose.yml`. They therefore share
a single version, carried by a single git tag.

The alternative, an independent version line per image, was considered and
rejected: it needs two tags per release, and it makes "which commit is this
image from?" a question with two different answers.

Version numbers follow [semantic versioning](https://semver.org): `MAJOR.MINOR.PATCH`.

| Change | Bump |
| --- | --- |
| Breaking REST API change, config key removed or renamed, database schema change requiring a reindex | MAJOR |
| New endpoint, new config key with a default, new indexed data | MINOR |
| Bug fix with no interface change | PATCH |

When it is unclear whether a change is breaking, assume it is.

## The tag is the source of truth

The version is **derived from the git tag**, never hand-edited into the build
script. `multi-build.sh` reads it with `git describe --tags --exact-match` and
refuses to publish when:

* `HEAD` is not at a tag — there is no version to publish;
* the working tree is dirty — the image would not correspond to any commit;
* `pyproject.toml` or `rust/Cargo.toml` declares a version other than the tag —
  the artifact would misreport itself.

Those manifests still carry the number because the Python package and the Rust
crate each declare their own version, but the build refuses to proceed unless
they agree with the tag, so they cannot drift the way the build script's
hand-edited constants did.

## Cutting a release

```bash
# 1. Update the version in both manifests to the new number.
#    Both must match the tag you are about to cut.
$EDITOR pyproject.toml rust/Cargo.toml

# 2. Refresh the lockfiles so they record the new package version.
uv lock
cargo update -p uaas --manifest-path rust/Cargo.toml

# 3. Commit, merge to main, then tag the merge commit.
git tag -a v1.4.0 -m "Release 1.4.0"
git push origin v1.4.0

# 4. Publish from the tagged, clean commit.
./multi-build.sh
```

For a build that is not a release — a throwaway or a test image — set the tag
explicitly instead, which bypasses the tag and manifest checks:

```bash
VERSION=dev-myfeature ./multi-build.sh
```

## Baseline: v1.4.0

Before this scheme, the version was hand-edited in three places and they
disagreed: `multi-build.sh` published the web image as `v1.4` and the service
image as `v1.3`, while both manifests declared `1.3.0`, and the repository had
no git tags at all.

The baseline is **v1.4.0**, not `v1.3.0`, because `nchain/innovation-uaas-web:v1.4`
was already published. Adopting `v1.3.0` as a repo-wide baseline would have
republished the web image under a *lower* number than one already on Docker Hub,
so consumers pulling by tag would have seen the version move backwards.
`innovation-uaas-service` therefore skips from `v1.3` to `v1.4.0` with no
functional change; that is the one-off cost of merging the two version lines.

## Changelog

Releases from v1.4.0 onward should carry a curated `CHANGELOG.md` entry grouped
by `Added / Changed / Fixed / Removed / Security`, written in the same change
that makes the change rather than reconstructed at release time.

No changelog exists yet, and this document does not invent one: the history of
v1.0 to v1.3 is not recoverable from the repository, which has no tags and no
release notes. Start the file at v1.4.0.
