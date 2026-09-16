#!/bin/bash

# This build file creates the uaas web and rust services for use with the nChain rnd prototyping projects.
# For a faster build, you can use the cloud builder (cloud-nchain-rndprototyping).
# Please check the allowed build minutes, as exceeding them may affect ability to build.
# Uncomment the --builder flag to enable the cloud builder, and comment out the --platform flag.
#
# The image tag is derived from the git tag, not hand-edited here, so the
# published image, the commit and the declared version cannot disagree.
# See docs/Versioning.md. To publish, tag the commit first:
#
#     git tag -a v1.4.0 -m "Release 1.4.0" && git push origin v1.4.0
#
# Set VERSION explicitly only for a throwaway build that is not a release.

set -euo pipefail

VERSION="${VERSION:-}"

if [ -z "$VERSION" ]; then
    if ! VERSION="$(git describe --tags --exact-match 2>/dev/null)"; then
        echo "error: HEAD is not at a release tag, so there is no version to publish." >&2
        echo "       Tag the release first, or set VERSION=... for a non-release build." >&2
        exit 1
    fi

    # Any tracked modification means HEAD no longer describes the tree, so the
    # image could not be reproduced from tag $VERSION.
    if [ -n "$(git status --porcelain --untracked-files=no)" ]; then
        echo "error: tracked files are modified; the image would not match tag $VERSION." >&2
        git status --short --untracked-files=no >&2
        exit 1
    fi

    # Untracked files matter only when they sit inside the docker build context
    # that the Dockerfiles actually COPY, since there is no .dockerignore and
    # anything in those paths is baked into a layer. Untracked notes elsewhere
    # in the repository cannot reach either image, so they do not block a
    # release. Keep this list in step with the COPY lines in Python_Dockerfile
    # and Rust_Dockerfile.
    context_paths=(rust python/src data/uaasr.toml pyproject.toml uv.lock)
    untracked="$(git ls-files --others --exclude-standard -- "${context_paths[@]}")"
    if [ -n "$untracked" ]; then
        echo "error: untracked files inside the docker build context would be" >&2
        echo "       baked into the image but are not in tag $VERSION:" >&2
        echo "$untracked" | sed 's/^/         /' >&2
        exit 1
    fi

    # One repo-wide version: both images ship from one commit under one tag,
    # so the manifests must agree with it or the artifact would misreport
    # its own version. Strip the leading v to compare against the manifests.
    expected="${VERSION#v}"
    for manifest in pyproject.toml rust/Cargo.toml; do
        declared="$(sed -n 's/^version = "\(.*\)"/\1/p' "$manifest" | head -1)"
        if [ "$declared" != "$expected" ]; then
            echo "error: $manifest declares $declared but the tag is $VERSION." >&2
            exit 1
        fi
    done
fi

echo "Publishing version $VERSION"

# Project Id1:  (uaas-web)
BASE_TAG1=uaas-web
PUBLISH_TAG1=nchain/innovation-$BASE_TAG1:$VERSION

# multi build, tag and push base images
docker buildx build  --platform linux/amd64,linux/arm64 --push -t "$PUBLISH_TAG1" --file Python_Dockerfile .

# Project Id2:  (uaas-rest)
BASE_TAG2=uaas-service
PUBLISH_TAG2=nchain/innovation-$BASE_TAG2:$VERSION

# multi build, tag and push base images
docker buildx build  --platform linux/amd64,linux/arm64 --push -t "$PUBLISH_TAG2" --file Rust_Dockerfile .
