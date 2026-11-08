#!/bin/sh
# Makes Oxidraft.app, the macOS app, from a release build (for this Mac, or
# for --target, as the release workflow builds); with --install puts it in
# /Applications (in place of the one there):
#   packaging/macos/make_app.sh [--target <triple>] [--install]
#
# The app is the binary, its icon (assets/icon.icns, from
# packaging/make_icon.py) and Info.plist (which opens DWG and DXF drawings
# with it), signed ad hoc so macOS runs it as built here. Its updater
# replaces the binary inside it and signs it again.
set -eu

install=
target=
while [ $# -gt 0 ]; do
    case "$1" in
        --install) install=1 ;;
        --target) target=$2; shift ;;
        *) echo "usage: $0 [--target <triple>] [--install]" >&2; exit 2 ;;
    esac
    shift
done

root=$(cd "$(dirname "$0")/../.." && pwd)
cd "$root"
dir=${CARGO_TARGET_DIR:-target.nosync}
if [ -n "$target" ]; then
    cargo build --release --locked -p oxidraft-app --target "$target"
    release=$dir/$target/release
else
    cargo build --release --locked -p oxidraft-app
    release=$dir/release
fi

version=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
app=$release/Oxidraft.app
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$release/oxidraft" "$app/Contents/MacOS/oxidraft"
cp assets/icon.icns "$app/Contents/Resources/Oxidraft.icns"
sed "s/@VERSION@/$version/g" packaging/macos/Info.plist > "$app/Contents/Info.plist"
codesign --force --sign - "$app"
echo "Made $app ($version)"

if [ -n "$install" ]; then
    rm -rf /Applications/Oxidraft.app
    ditto "$app" /Applications/Oxidraft.app
    echo "Installed /Applications/Oxidraft.app"
fi
