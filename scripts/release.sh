#!/usr/bin/env bash
# Cuts a release: runs the checks, bumps the version in Cargo.toml and Cargo.lock,
# writes the version's section into CHANGELOG.md, then commits and tags.
# Usage: scripts/release.sh <major|minor|patch>   e.g. scripts/release.sh minor
set -euo pipefail
cd "$(dirname "$0")/.."

usage="usage: scripts/release.sh <major|minor|patch>   e.g. scripts/release.sh minor"
if [[ $# -ne 1 ]]; then
    echo "$usage" >&2
    exit 1
fi

current=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n1)
if ! [[ "$current" =~ ^([0-9]+)\.([0-9]+)\.([0-9]+)$ ]]; then
    echo "the version in Cargo.toml is not plain semver: $current" >&2
    exit 1
fi
major=${BASH_REMATCH[1]} minor=${BASH_REMATCH[2]} patch=${BASH_REMATCH[3]}

case "$1" in
    major) version="$((major + 1)).0.0" ;;
    minor) version="$major.$((minor + 1)).0" ;;
    patch) version="$major.$minor.$((patch + 1))" ;;
    *)
        echo "not a bump: $1" >&2
        echo "$usage" >&2
        exit 1
        ;;
esac

if [[ -n "$(git status --porcelain)" ]]; then
    echo "the working tree is dirty; commit or stash first" >&2
    exit 1
fi

echo "releasing v$version (was $current)"

just check

sed -i '' "s/^version = \"$current\"/version = \"$version\"/" Cargo.toml
cargo update -p as-gui
git cliff --unreleased --tag "v$version" -p CHANGELOG.md

git add Cargo.toml Cargo.lock CHANGELOG.md
git commit -m "chore(release): v$version"
git tag "v$version"

echo "released v$version; publish with: git push && git push --tags"
