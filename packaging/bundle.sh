#!/usr/bin/env bash
# Assembles target/bundle/Agent Studio GUI.app from the release binary, ad-hoc signed
# (Apple Silicon refuses a binary with no signature at all), and with `dmg` also the disk image.
set -euo pipefail
cd "$(dirname "$0")/.."

name="Agent Studio GUI"
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
out=target/bundle
app="$out/$name.app"

cargo build --release --locked --no-default-features

rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp target/release/as-gui "$app/Contents/MacOS/as-gui"
cp assets/AppIcon.icns "$app/Contents/Resources/AppIcon.icns"
sed "s/@VERSION@/$version/g" packaging/Info.plist > "$app/Contents/Info.plist"
codesign --force --deep --sign - "$app"
echo "built $app"

if [[ "${1:-}" == "dmg" ]]; then
    dmg="$out/Agent-Studio-GUI-$version.dmg"
    stage=$(mktemp -d)
    cp -R "$app" "$stage/"
    ln -s /Applications "$stage/Applications"
    rm -f "$dmg"
    hdiutil create -volname "$name" -srcfolder "$stage" -ov -format UDZO "$dmg" >/dev/null
    rm -rf "$stage"
    echo "built $dmg"
fi
