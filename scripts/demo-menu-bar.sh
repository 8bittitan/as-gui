#!/usr/bin/env bash
# Records the menu bar demo to demos/menu-bar.mp4 (or $1): the as-gui icon in the menu bar, its
# menu opening, Quick Ask picked, then a question and its streamed answer.
#
# Headless recordings can't draw the native status item, so rune records the panel over the
# backdrop (`scripts/demo-menu-bar.json`, the main window held back by AS_GUI_DEMO_MENU_BAR)
# and ffmpeg paints the menu bar and the menu over it. The menu's rows mirror
# `menubar::menu_items`; the panel opens at MENU_CLOSE, when the script presses Quick Ask's
# chord, which runs the same toggle the menu item does.
set -euo pipefail
cd "$(dirname "$0")/.."

out="${1:-demos/menu-bar.mp4}"
mkdir -p "$(dirname "$out")"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

AS_GUI_DEMO=1 AS_GUI_DEMO_MENU_BAR=1 ALGOLIA_AGENT_ID=mock-shopping \
    cargo run --release -- --desktop scripts/backdrop.png --script scripts/demo-menu-bar.json \
    --video "$work/raw.mp4" --pointer

source scripts/menu-bar.sh
font=$FONT

# Seconds into the recording; MENU_CLOSE matches the chord in demo-menu-bar.json.
MENU_OPEN=1.3
ROW_HOVER=2.2
MENU_CLOSE=3.0

# Pixels at the recording's 2x scale.
MENU_X=1808
MENU_Y=52
MENU_W=560
ROW_H=44
ROW_Y=62
TEXT_X=$((MENU_X + 28))
HINT_RIGHT=$((MENU_X + MENU_W - 28))

open="enable='between(t,$MENU_OPEN,$MENU_CLOSE)'"
hover="enable='between(t,$ROW_HOVER,$MENU_CLOSE)'"

row_y() { echo $((ROW_Y + $1 + 8)); }
item() { # label, y offset inside the menu, colour
    echo "drawtext=fontfile=$font:text='$1':fontsize=26:fontcolor=$3:x=$TEXT_X:y=$(row_y "$2"):$open,"
}
hint() { # keycaps, y offset inside the menu
    echo "drawtext=fontfile=$font:text='$1':fontsize=26:fontcolor=white@0.55:x=$HINT_RIGHT-tw:y=$(row_y "$2"):$open,"
}

cat >"$work/filter" <<EOF
$(menu_bar_icon 1)
[0:v]
$(menu_bar Finder 'File     Edit     View     Go     Window     Help')
drawbox=x=$((ICON_X - 12)):y=4:w=60:h=40:color=white@0.25:t=fill:$open,
drawbox=x=$MENU_X:y=$MENU_Y:w=$MENU_W:h=324:color=0x2b2b30@0.96:t=fill:$open,
drawbox=x=$MENU_X:y=$MENU_Y:w=$MENU_W:h=324:color=white@0.14:t=2:$open,
drawbox=x=$((MENU_X + 10)):y=$ROW_Y:w=$((MENU_W - 20)):h=$ROW_H:color=0x0a64d8:t=fill:$hover,
$(item 'Quick Ask' 0 white)
$(hint '⇧⌘Space' 0)
$(item 'Open as-gui' $ROW_H white)
$(item 'New conversation' $((ROW_H * 2)) white)
$(hint '⌘N' $((ROW_H * 2)))
$(item 'Settings…' $((ROW_H * 3)) white)
$(hint '⌘\,' $((ROW_H * 3)))
drawbox=x=$((MENU_X + 20)):y=$((ROW_Y + ROW_H * 4 + 9)):w=$((MENU_W - 40)):h=2:color=white@0.14:t=fill:$open,
$(item 'Mock shopping assistant' $((ROW_H * 4 + 20)) white@0.4)
drawbox=x=$((MENU_X + 20)):y=$((ROW_Y + ROW_H * 5 + 29)):w=$((MENU_W - 40)):h=2:color=white@0.14:t=fill:$open,
$(item 'Quit as-gui' $((ROW_H * 5 + 40)) white)
$(hint '⌘Q' $((ROW_H * 5 + 40)))
null[base];
$(place_icon base out)
EOF

ffmpeg -v error -y -i "$work/raw.mp4" -i "$MENU_BAR_ICON" -/filter_complex "$work/filter" -map '[out]' \
    -c:v libx264 -pix_fmt yuv420p -crf 18 "$out"
echo "wrote $out"
