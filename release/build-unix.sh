#!/usr/bin/env bash
set -euo pipefail

root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd "$root"
platform=${1:?Usage: bash release/build-unix.sh ubuntu\|fedora\|arch\|macos}
case "$platform:$(uname -s)" in
    ubuntu:Linux|fedora:Linux|arch:Linux)
        # shellcheck disable=SC1091
        . /etc/os-release
        if [[ "$platform" != "$ID" && ! ( "$platform" == arch && "$ID" == cachyos ) ]]; then
            echo "Build the $platform package on $platform." >&2
            exit 1
        fi
        targets=("$(rustc -vV | sed -n 's/^host: //p')")
        ;;
    macos:Darwin)
        targets=(aarch64-apple-darwin x86_64-apple-darwin)
        export MACOSX_DEPLOYMENT_TARGET=12.0
        # Strip the combined binary with Apple's tool before packaging and signing.
        export CARGO_PROFILE_RELEASE_STRIP=none
        ;;
    *)
        echo "Use a native Ubuntu, Fedora, Arch Linux, CachyOS, or macOS build environment." >&2
        exit 1
        ;;
esac

target_dir=${CARGO_TARGET_DIR:-"$root/target"}
registry=${CARGO_HOME:-"$HOME/.cargo"}/registry/src
mkdir -p -- "$target_dir"
target_dir=$(cd -- "$target_dir" && pwd)
export CARGO_TARGET_DIR="$target_dir"
export TRUCKCRAFT_BUNDLE_LOCAL_ART=0
export CARGO_ENCODED_RUSTFLAGS="--remap-path-prefix=$HOME=/home"$'\x1f'"--remap-path-prefix=$registry=/cargo/registry/src"$'\x1f'"--remap-path-prefix=$root=/src"$'\x1f'"--remap-path-prefix=$target_dir=/target"

work=$(mktemp -d)
trap 'rm -f -- "$work/dependencies.txt" "$work/truckcraft"; rmdir -- "$work"' EXIT
for target in "${targets[@]}"; do
    cargo build --locked --release -p truckcraft --target "$target"
    cargo tree --locked -p truckcraft -e normal --prefix none --target "$target" >> "$work/dependencies.txt"
done

binary="$target_dir/${targets[0]}/release/truckcraft"
if [[ "$platform" == macos ]]; then
    lipo -create -output "$work/truckcraft" "$binary" "$target_dir/${targets[1]}/release/truckcraft"
    xcrun strip -S -x "$work/truckcraft"
    binary="$work/truckcraft"
fi

python3 release/package.py "$platform" --binary "$binary" --registry "$registry" \
    --dependency-tree "$work/dependencies.txt" --output "${TRUCKCRAFT_DIST_DIR:-"$root/dist"}"
