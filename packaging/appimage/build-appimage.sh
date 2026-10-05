#!/usr/bin/env bash
# Builds mtg-duelstone-x86_64.AppImage: the release binary in usr/bin and everything it loads at
# runtime (assets, engine jar, Forge data, a Java runtime) in usr/share/mtg-duelstone, where
# mtg_client::launcher::bundle_dirs() looks. appimagetool and its runtime are downloaded on first
# use into packaging/appimage/tools.
#   ./packaging/appimage/build-appimage.sh            (SKIP_BUILD=1 reuses the last release build)
# Needs Rust, JDK 17, the engine jar and Forge (see stage-bundle.sh).
#
# No libraries are bundled: the binary only links libc, libm, libgcc_s, libasound, libudev,
# libwayland-client and libffi, which every desktop distribution has, and Vulkan/GL must come
# from the system's driver anyway.
set -euo pipefail
cd "$(dirname "$0")/../.."
here=packaging/appimage
tools="$here/tools"
target=${CARGO_TARGET_DIR:-target}
mkdir -p "$tools"
fetch() {
    [[ -s "$tools/$1" ]] || { curl -fL --retry 5 -o "$tools/$1" "$2"; chmod +x "$tools/$1"; }
}
fetch appimagetool-x86_64.AppImage https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage
# appimagetool would download the runtime on every run, without retrying.
fetch runtime-x86_64 https://github.com/AppImage/type2-runtime/releases/download/continuous/runtime-x86_64

if [[ "${SKIP_BUILD:-0}" != 1 ]]; then
    cargo build --release -p arena -p mtg_client --bin mtg-duelstone --bin fetch-offline
fi

appdir="$here/AppDir"
rm -rf "$appdir"
mkdir -p "$appdir/usr/bin" "$appdir/usr/share/applications"
install -m755 "$target/release/mtg-duelstone" "$appdir/usr/bin/mtg-duelstone"
strip "$appdir/usr/bin/mtg-duelstone"
# One-time card image fetch for release users (images are not redistributed); see offline/README.md
install -m755 "$target/release/fetch-offline" "$appdir/usr/bin/fetch-offline"
strip "$appdir/usr/bin/fetch-offline"
./packaging/stage-bundle.sh "$appdir/usr/share/mtg-duelstone"

install -m644 packaging/mtg-duelstone.desktop "$appdir/usr/share/applications/"
for s in 16 32 48 64 128 256 512; do
    install -Dm644 "packaging/icons/mtg-duelstone-$s.png" "$appdir/usr/share/icons/hicolor/${s}x${s}/apps/mtg-duelstone.png"
done
install -Dm644 packaging/mtg-duelstone.svg "$appdir/usr/share/icons/hicolor/scalable/apps/mtg-duelstone.svg"
cp packaging/mtg-duelstone.desktop "$appdir/"
cp packaging/icons/mtg-duelstone-256.png "$appdir/mtg-duelstone.png"
ln -s mtg-duelstone.png "$appdir/.DirIcon"
# The binary finds its resources through /proc/self/exe, which resolves this link.
ln -s usr/bin/mtg-duelstone "$appdir/AppRun"

# Extract-and-run: CI containers have no FUSE to mount appimagetool itself.
ARCH=x86_64 APPIMAGE_EXTRACT_AND_RUN=1 "$tools/appimagetool-x86_64.AppImage" \
    --no-appstream --runtime-file "$tools/runtime-x86_64" "$appdir" mtg-duelstone-x86_64.AppImage
echo "Built mtg-duelstone-x86_64.AppImage"
