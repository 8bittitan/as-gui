#!/usr/bin/env bash
# Records Ask about selection to demos/ask-selection.mp4 (or $1): a forum post selected in
# Safari, ⇧⌘A, the panel quoting it, a question about it and the streamed answer.
#
# The browser window is painted on the backdrop before rune records (`demo-ask-selection.json`,
# main window held back by AS_GUI_DEMO_MENU_BAR), and the selection is painted over the
# recording after. Demo mode captures `selection::DEMO_SELECTION`, so POST below must hold
# the same words. The window sits left of the panel, so nothing painted after covers it.
set -euo pipefail
cd "$(dirname "$0")/.."
source scripts/menu-bar.sh

out="${1:-demos/ask-selection.mp4}"
mkdir -p "$(dirname "$out")"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

POST=(
    "Been running the Aero 2 for"
    "marathons, but the cushioning"
    "fades by mile twenty. Is it the"
    "shoe, or should I swap the"
    "insoles instead?"
)

# Seconds into the recording; the chord in the JSON comes after the last line is selected.
SELECT_FROM=0.9
SELECT_STEP=0.16

# Pixels at the recording's 2x scale.
WIN_X=80
WIN_Y=150
WIN_W=700
WIN_H=880
TITLE_H=64
TEXT_X=$((WIN_X + 44))
POST_Y=$((WIN_Y + 260))
LINE_H=48
INK=0x1d1d1f

line() { # text, index, extra drawtext options
    echo "drawtext=fontfile=$FONT:text='$1':fontsize=30:fontcolor=$INK:x=$TEXT_X:y=$((POST_Y + $2 * LINE_H))$3,"
}

reply_bars() {
    local y=$((POST_Y + ${#POST[@]} * LINE_H + 70))
    for width in 520 440 580 300; do
        echo "drawbox=x=$TEXT_X:y=$y:w=$width:h=18:color=0xe4e4e8:t=fill,"
        y=$((y + 44))
    done
}

{
    menu_bar_icon 1
    echo "[0:v]"
    menu_bar Safari 'File     Edit     View     History     Bookmarks     Window     Help'
    cat <<EOF
drawbox=x=$WIN_X:y=$WIN_Y:w=$WIN_W:h=$WIN_H:color=white:t=fill,
drawbox=x=$WIN_X:y=$WIN_Y:w=$WIN_W:h=$TITLE_H:color=0xe9e9ec:t=fill,
drawbox=x=$WIN_X:y=$WIN_Y:w=$WIN_W:h=$WIN_H:color=black@0.12:t=2,
drawtext=fontfile=$FONT:text='●  ●  ●':fontsize=26:fontcolor=0xc8c8cc:x=$((WIN_X + 24)):y=$((WIN_Y + 18)),
drawtext=fontfile=$FONT:text='community.example.com':fontsize=24:fontcolor=0x6e6e73:x=$((WIN_X + 250)):y=$((WIN_Y + 20)),
drawbox=x=$TEXT_X:y=$((WIN_Y + 104)):w=56:h=56:color=0x7c5cff:t=fill,
drawtext=fontfile=$FONT:text='Lumine':fontsize=28:fontcolor=$INK:x=$((TEXT_X + 76)):y=$((WIN_Y + 104)),
drawtext=fontfile=$FONT:text='2h ago':fontsize=24:fontcolor=0x8e8e93:x=$((TEXT_X + 76)):y=$((WIN_Y + 138)),
drawtext=fontfile=$FONT:text='Aero 2 cushioning fade?':fontsize=38:fontcolor=$INK:x=$TEXT_X:y=$((WIN_Y + 190)),
EOF
    for i in "${!POST[@]}"; do line "${POST[$i]}" "$i" ""; done
    reply_bars
    echo "null[base];"
    place_icon base out
} >"$work/backdrop-filter"
ffmpeg -v error -y -i scripts/backdrop.png -i "$MENU_BAR_ICON" -/filter_complex "$work/backdrop-filter" \
    -map '[out]' -frames:v 1 "$work/backdrop.png"

AS_GUI_DEMO=1 AS_GUI_DEMO_MENU_BAR=1 ALGOLIA_AGENT_ID=mock-shopping \
    cargo run --release -- --desktop "$work/backdrop.png" --script scripts/demo-ask-selection.json \
    --video "$work/raw.mp4" --keys

# The selection: each line redrawn on its highlight as the drag reaches it.
{
    for i in "${!POST[@]}"; do
        from=$(echo "$SELECT_FROM + $SELECT_STEP * $i" | bc)
        line "${POST[$i]}" "$i" ":box=1:boxcolor=0xb3d7ff:boxborderw=4|2:enable='gte(t,$from)'"
    done
    echo "null"
} >"$work/selection-filter"
ffmpeg -v error -y -i "$work/raw.mp4" -/filter_complex "$work/selection-filter" \
    -c:v libx264 -pix_fmt yuv420p -crf 18 "$out"
echo "wrote $out"
