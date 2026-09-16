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

    if [ -n "$(git status --porcelain)" ]; then
        echo "error: the working tree is dirty; the image would not match tag $VERSION." >&2
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
