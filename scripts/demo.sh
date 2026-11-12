#!/usr/bin/env bash
# Records a demo video into demos/: the main tour by default, one of quick-ask, menu-bar,
# ask-selection or notify, or all of them in turn. Each demo is demos/<name>.mp4.
set -euo pipefail
cd "$(dirname "$0")/.."

DEMOS=(tour quick-ask menu-bar ask-selection notify)

record() {
    local out="demos/$1.mp4"
    case "$1" in
        tour)
            AS_GUI_DEMO=1 cargo run --release -- --script scripts/demo.json --video "$out" --keys --pointer ;;
        quick-ask)
            AS_GUI_DEMO=1 cargo run --release -- --desktop scripts/backdrop.png --script scripts/demo-quick-ask.json --video "$out" --keys --pointer ;;
        menu-bar | ask-selection | notify)
            "scripts/demo-$1.sh" "$out" ;;
    esac
}

name="${1:-tour}"
if [[ "$name" != all && " ${DEMOS[*]} " != *" $name "* ]]; then
    echo "unknown demo '$name': pick ${DEMOS[*]} or all" >&2
    exit 1
fi

mkdir -p demos
if [[ "$name" == all ]]; then
    for demo in "${DEMOS[@]}"; do
        echo "recording $demo"
        record "$demo"
    done
else
    record "$name"
fi
