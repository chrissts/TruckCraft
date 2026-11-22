#!/usr/bin/env bash
set -euo pipefail

app_id=io.github.chrissts.TruckCraft
game='Euro Truck Simulator 2'
data=${XDG_DATA_HOME:-"$HOME/.local/share"}
native="$data/$game"
proton="$data/Steam/steamapps/compatdata/227300/pfx/drive_c/users/steamuser/Documents/$game"
mkdir -p -- "$native" "$proton"
custom=$(mktemp -d)
trap 'rmdir -- "$custom"' EXIT

check() {
    local folder=$1 mode=$2
    shift 2
    flatpak run --user "$@" --env="TMPDIR=$folder" \
        --env="TRUCKCRAFT_CHECK_SAVE_DISCOVERY=$mode" "$app_id" --package-check
}

check "$native" default
check "$proton" default
check "$custom" custom --filesystem="$custom"
