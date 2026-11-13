#!/usr/bin/env bash
# Cuts a release: runs the checks, bumps the version in Cargo.toml and Cargo.lock,
# writes the version's section into CHANGELOG.md, then commits and tags.
# Usage: scripts/release.sh <version>   e.g. scripts/release.sh 0.2.0
set -euo pipefail
cd "$(dirname "$0")/.."

if [[ $# -ne 1 ]]; then
    echo "usage: scripts/release.sh <version>   e.g. scripts/release.sh 0.2.0" >&2
    exit 1
fi
version="${1#v}"
if ! [[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo "not a semver version: $1" >&2
    exit 1
fi

if [[ -n "$(git status --porcelain)" ]]; then
    echo "the working tree is dirty; commit or stash first" >&2
    exit 1
fi

current=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n1)
newer() {
    local a=(${1//./ }) b=(${2//./ })
    for i in 0 1 2; do
        (( ${a[i]} < ${b[i]} )) && return 0
        (( ${a[i]} > ${b[i]} )) && return 1
    done
    return 1
}
if ! newer "$version" "$current"; then
    echo "$version does not come after the current version $current" >&2
    exit 1
fi

just check

sed -i '' "s/^version = \"$current\"/version = \"$version\"/" Cargo.toml
cargo update -p as-gui
git cliff --unreleased --tag "v$version" -p CHANGELOG.md

git add Cargo.toml Cargo.lock CHANGELOG.md
git commit -m "chore(release): v$version"
git tag "v$version"

echo "released v$version; publish with: git push && git push --tags"
