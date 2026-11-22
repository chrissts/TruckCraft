#!/usr/bin/env bash
set -euo pipefail

cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."
app_id=io.github.chrissts.TruckCraft
output=${TRUCKCRAFT_DIST_DIR:-"$PWD/dist"}
mkdir -p -- "$output"
output=$(cd -- "$output" && pwd)
work=$(mktemp -d)
trap 'rm -rf -- "$work"' EXIT

mkdir -p "$work/source"
git archive HEAD | tar -x -C "$work/source"
cp -R release/flatpak "$work/source/release/"
cp "release/flatpak/$app_id.yml" "$work/"
(
    cd "$work/source"
    mkdir -p .cargo
    cargo vendor --locked --versioned-dirs vendor/registry > .cargo/config.toml
)

flatpak-builder --user --install-deps-from=flathub --state-dir="$work/state" \
    --repo="$work/repo" --default-branch=stable "$work/build" "$work/$app_id.yml"
flatpak build-bundle --runtime-repo=https://dl.flathub.org/repo/flathub.flatpakrepo \
    "$work/repo" "$output/TruckCraft.flatpak" "$app_id" stable
