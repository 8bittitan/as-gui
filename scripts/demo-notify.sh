#!/usr/bin/env bash
# Records the background-reply notification to demos/notify.mp4 (or $1): a question sent to the
# agent, a browser painted over the recording as the user switches away, the reply settling in
# the background, a macOS notification banner dropping in, and a click returning to the app and
# its settled answer.
#
# Headless recordings can't show system notifications (and an unbundled `cargo run` build
# can't post one), so ffmpeg paints the browser and the banner over the recording, the way
# demo-menu-bar paints the menu bar. The banner's body holds the reply's first line, as
# `notify::first_line` would trim it.
set -euo pipefail
cd "$(dirname "$0")/.."

out="${1:-demos/notify.mp4}"
mkdir -p "$(dirname "$out")"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

AS_GUI_DEMO=1 cargo run --release -- --script scripts/demo-notify.json --video "$work/raw.mp4" --pointer

source scripts/menu-bar.sh
font=$FONT

# Seconds into the recording; SEND matches the tap in demo-notify.json.
SEND=3.6
SWITCH=4.2
NOTIFY=7.2
CLICK=9.8

away="enable='between(t,$SWITCH,$CLICK)'"
banner="enable='between(t,$NOTIFY,$CLICK)'"

# The recording is the app's window, so a full-frame browser reads as having switched away.
# Its post is the same running-shoes thread demo-ask-selection paints, for continuity.
TITLE_Y=210
POST_Y=300
LINE_H=48
INK=0x1d1d1f
TEXT_X=88

line() { # text, index
    echo "drawtext=fontfile=$font:text='$1':fontsize=30:fontcolor=$INK:x=$TEXT_X:y=$((POST_Y + $2 * LINE_H)):$away,"
}

{
    echo "[1:v]scale=52:52[icon];"
    echo "[0:v]"
    cat <<EOF
drawbox=x=0:y=0:w=iw:h=ih:color=white:t=fill:$away,
drawbox=x=0:y=0:w=iw:h=64:color=0xe9e9ec:t=fill:$away,
drawbox=x=0:y=0:w=iw:h=ih:color=black@0.12:t=2:$away,
drawtext=fontfile=$font:text='●  ●  ●':fontsize=26:fontcolor=0xc8c8cc:x=24:y=18:$away,
drawtext=fontfile=$font:text='community.example.com':fontsize=24:fontcolor=0x6e6e73:x=w/2-tw/2:y=20:$away,
drawtext=fontfile=$font:text='Aero 2 cushioning fade?':fontsize=38:fontcolor=$INK:x=$TEXT_X:y=$TITLE_Y:$away,
EOF
    line "Been running the Aero 2 for" 0
    line "marathons, but the cushioning" 1
    line "fades by mile twenty. Is it the" 2
    line "shoe, or should I swap the" 3
    line "insoles instead?" 4
    reply_y=$((POST_Y + 5 * LINE_H + 70))
    for width in 1100 940 1220 640; do
        echo "drawbox=x=$TEXT_X:y=$reply_y:w=$width:h=18:color=0xe4e4e8:t=fill:$away,"
        reply_y=$((reply_y + 44))
    done

    # The notification banner, top-right: icon, the agent's name, the reply's first line, now.
    BAN_X="iw-808"
    echo "drawbox=x=$BAN_X:y=48:w=760:h=200:color=0x2b2b30@0.96:t=fill:$banner,"
    echo "drawbox=x=$BAN_X:y=48:w=760:h=200:color=white@0.14:t=2:$banner,"
    echo "drawbox=x=iw-772:y=122:w=52:h=52:color=0x7c5cff:t=fill:$banner,"
    echo "drawtext=fontfile=$font:text='Mock shopping assistant':fontsize=28:fontcolor=white:x=w-692:y=92:$banner,"
    echo "drawtext=fontfile=$font:text='Wide feet work best with a Cushioned Series':fontsize=26:fontcolor=white@0.8:x=w-692:y=138:$banner,"
    echo "drawtext=fontfile=$font:text='shoe — the search above pulled the current guides.':fontsize=26:fontcolor=white@0.8:x=w-692:y=174:$banner,"
    echo "drawtext=fontfile=$font:text='now':fontsize=24:fontcolor=white@0.5:x=w-76-tw:y=66:$banner,"
    echo "null[base];"
    echo "[base][icon]overlay=x=main_w-770:y=124:enable='between(t,$NOTIFY,$CLICK)'[out]"
} >"$work/filter"

ffmpeg -v error -y -i "$work/raw.mp4" -i "$MENU_BAR_ICON" -/filter_complex "$work/filter" \
    -map "[out]" -c:v libx264 -pix_fmt yuv420p -crf 18 "$out"
echo "wrote $out"
