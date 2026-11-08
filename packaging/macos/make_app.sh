#!/bin/sh
# Makes Oxidraft.app, the macOS app, from a release build; with --install
# puts it in /Applications (in place of the one there):
#   packaging/macos/make_app.sh [--install]
#
# The app is the binary, its icon (assets/icon.icns, from
# packaging/make_icon.py) and Info.plist, signed ad hoc so macOS runs it as
# built here. Its updater replaces the binary inside it, as it would a bare one.
set -eu

root=$(cd "$(dirname "$0")/../.." && pwd)
cd "$root"
cargo build --release --locked -p oxidraft-app

release=${CARGO_TARGET_DIR:-target.nosync}/release
version=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
app=$release/Oxidraft.app
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$release/oxidraft" "$app/Contents/MacOS/oxidraft"
cp assets/icon.icns "$app/Contents/Resources/Oxidraft.icns"
sed "s/@VERSION@/$version/g" packaging/macos/Info.plist > "$app/Contents/Info.plist"
codesign --force --sign - "$app"
echo "Made $app ($version)"

if [ "${1:-}" = "--install" ]; then
    rm -rf /Applications/Oxidraft.app
    ditto "$app" /Applications/Oxidraft.app
    echo "Installed /Applications/Oxidraft.app"
fi
