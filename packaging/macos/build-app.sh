#!/usr/bin/env bash
# Builds "MTG Duelstone.app" and mtg-duelstone-macos-<arch>.dmg into target/macos: the release
# binary in Contents/MacOS and everything it loads at runtime (assets, engine jar, Forge data, a
# Java runtime) in Contents/Resources, where mtg_client::launcher::bundle_dirs() looks.
#   ./packaging/macos/build-app.sh                    (SKIP_BUILD=1 reuses the last release build)
# Needs Rust, JDK 17 ($JAVA_HOME; its jlink builds the bundled runtime, so use a JDK for the same
# CPU as the app), the engine jar, Forge (see stage-bundle.sh) and the Xcode command line tools.
# The app is signed ad hoc (no Developer ID), so a downloaded copy has to be allowed once under
# System Settings → Privacy & Security.
#
# mtg-duelstone.icns is packaging/mtg-duelstone.svg on the macOS icon grid (an 824 px tile with a
# 100 px margin on a 1024 px canvas).
set -euo pipefail
cd "$(dirname "$0")/../.."
here=packaging/macos
target=${CARGO_TARGET_DIR:-target}
out=target/macos
arch=$(uname -m)
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n1)

if [[ "${SKIP_BUILD:-0}" != 1 ]]; then
    cargo build --release -p arena
fi

app="$out/MTG Duelstone.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS"
install -m755 "$target/release/mtg-duelstone" "$app/Contents/MacOS/mtg-duelstone"
strip -x "$app/Contents/MacOS/mtg-duelstone"
./packaging/stage-bundle.sh "$app/Contents/Resources"
install -m644 "$here/mtg-duelstone.icns" "$app/Contents/Resources/"
sed "s/@VERSION@/$version/g" "$here/Info.plist" > "$app/Contents/Info.plist"

# Apple silicon refuses to run unsigned code, and --deep only signs nested code in the standard
# bundle folders: sign every Mach-O file of the Java runtime ourselves, then the bundle.
find "$app/Contents/Resources/jre" -type f -print0 | while IFS= read -r -d '' f; do
    if file -b "$f" | grep -q Mach-O; then codesign --force --sign - "$f"; fi
done
codesign --force --deep --sign - "$app"
codesign --verify --deep --strict "$app"

dmg="$out/mtg-duelstone-macos-$arch.dmg"
stage="$out/dmg"
rm -rf "$stage" "$dmg"
mkdir -p "$stage"
cp -R "$app" "$stage/"
ln -s /Applications "$stage/Applications"
hdiutil create -volname "MTG Duelstone $version" -srcfolder "$stage" -fs HFS+ -format UDZO -ov "$dmg"
rm -rf "$stage"
echo "Built $app and $dmg"
