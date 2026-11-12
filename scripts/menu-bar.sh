# Sourced by the demo scripts: a painted macOS menu bar for ffmpeg, at the recordings' 2x scale.

FONT=/System/Library/Fonts/SFNS.ttf
MENU_BAR_ICON=assets/MenuBarIcon.png
BAR_H=48
ICON_X=1820

# The filter chain for the bar itself, ending in a comma: the frontmost app's menus and the clock.
menu_bar() { # frontmost app, its menus
    cat <<EOF
drawbox=x=0:y=0:w=iw:h=$BAR_H:color=0x14142a@0.55:t=fill,
drawtext=fontfile=$FONT:text='$1':fontsize=26:fontcolor=white:x=44:y=11,
drawtext=fontfile=$FONT:text='$2':fontsize=26:fontcolor=white@0.9:x=152:y=11,
drawtext=fontfile=$FONT:text='Thu 12 Nov  09\:41':fontsize=26:fontcolor=white:x=w-tw-40:y=11,
EOF
}

# The as-gui icon, white as a template image draws on a dark bar: input $1 in, [icon] out.
menu_bar_icon() {
    echo "[$1:v]scale=36:36,lutrgb=r=255:g=255:b=255[icon];"
}

# Puts [icon] on [$1] into [$2].
place_icon() {
    echo "[$1][icon]overlay=x=$ICON_X:y=6[$2]"
}
