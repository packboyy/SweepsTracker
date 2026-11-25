#!/bin/sh
# Publishes a release on GitHub, which is what the app's "Check for updates" looks for.
#
#   scripts/release.sh 0.2.0
#
# Sets the version in Cargo.toml, commits and pushes that, builds the Mac app, and creates
# release v0.2.0 with the zipped app attached (and notes listing the commits since the
# last release). Needs a clean working tree on main and the GitHub CLI (gh) logged in.
# Publishing it starts .github/workflows/release-builds.yml, which adds the Windows and
# Linux builds to the release.
set -eu
cd "$(dirname "$0")/.."

VERSION="${1:-}"
case "$VERSION" in
    [0-9]*.[0-9]*.[0-9]*) ;;
    *) echo "Usage: scripts/release.sh X.Y.Z (e.g. 0.2.0)" >&2; exit 1 ;;
esac
TAG="v$VERSION"

if [ -n "$(git status --porcelain --untracked-files=no)" ]; then
    echo "Commit or stash your changes first." >&2
    exit 1
fi
if [ "$(git branch --show-current)" != "main" ]; then
    echo "Releases are made from main." >&2
    exit 1
fi
if git rev-parse -q --verify "refs/tags/$TAG" >/dev/null; then
    echo "$TAG already exists." >&2
    exit 1
fi

# Version bump (Cargo.lock records the version too), committed and pushed.
sed -i '' "s/^version = \".*\"/version = \"$VERSION\"/" Cargo.toml
cargo build --release
git add Cargo.toml Cargo.lock
git commit -m "Release $TAG"
git push

# Build the app (built from the release commit, so not "-dirty") and zip it for download.
scripts/bundle-mac.sh
ZIP="dist/SweepsTracker-$TAG-mac.zip"
rm -f "$ZIP"
ditto -c -k --keepParent "dist/Sweeps Tracker.app" "$ZIP"

gh release create "$TAG" "$ZIP" \
    --title "Sweeps Tracker $TAG" \
    --target main \
    --generate-notes
echo "Released $TAG"
