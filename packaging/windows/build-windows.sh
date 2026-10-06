#!/usr/bin/env bash
# Builds the Windows release into target/windows:
#   "MTG Duelstone/"                          mtg-duelstone.exe with everything it loads at runtime
#                                             (assets, engine jar, Forge data, a Java runtime) next to
#                                             it, where mtg_client::launcher::bundle_dirs() looks
#   mtg-duelstone-windows-x86_64.zip          that folder, to unzip and run
#   mtg-duelstone-windows-x86_64-setup.exe    the installer (mtg-duelstone.iss), when Inno Setup 6 is
#                                             installed; REQUIRE_INSTALLER=1 makes its absence an error
#   ./packaging/windows/build-windows.sh      (SKIP_BUILD=1 reuses the last release build)
# Runs in Git Bash. Needs Rust (MSVC toolchain), JDK 17 for Windows ($JAVA_HOME), the engine jar and
# Forge (see stage-bundle.sh), and 7-Zip or zip.
#
# Nothing is code-signed, so SmartScreen warns about the installer and the game until they have a
# download reputation ("More info" → "Run anyway"). The exe gets its icon from crates/arena/build.rs.
set -euo pipefail
cd "$(dirname "$0")/../.."
here=packaging/windows
target=${CARGO_TARGET_DIR:-target}
out=target/windows
name="MTG Duelstone"
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n1)

if [[ "${SKIP_BUILD:-0}" != 1 ]]; then
    cargo build --release -p arena -p mtg_client --bin mtg-duelstone --bin fetch-offline
fi
# The browser client the bundled engine serves to invitees (CI builds it once and sets SKIP_WEB=1).
if [[ "${SKIP_WEB:-0}" != 1 ]]; then
    tools/build-web.sh
fi

app="$out/$name"
mkdir -p "$out"
./packaging/stage-bundle.sh "$app"
cp "$target/release/mtg-duelstone.exe" "$app/"
# One-time card image fetch for release users (images are not redistributed); see offline/README.md
cp "$target/release/fetch-offline.exe" "$app/"

zip="mtg-duelstone-windows-x86_64.zip"
rm -f "$out/$zip"
if command -v 7z >/dev/null; then
    (cd "$out" && 7z a -tzip -mx=7 "$zip" "$name" >/dev/null)
else
    (cd "$out" && zip -qr -9 "$zip" "$name")
fi

iscc=$(command -v iscc || command -v ISCC || true)
for p in "/c/Program Files (x86)/Inno Setup 6/ISCC.exe" "/c/Program Files/Inno Setup 6/ISCC.exe"; do
    if [[ -z "$iscc" && -f "$p" ]]; then iscc=$p; fi
done
if [[ -n "$iscc" ]]; then
    winpath() { if command -v cygpath >/dev/null; then cygpath -w "$1"; else echo "$1"; fi; }
    # Git Bash would rewrite the /D… switches as if they were paths.
    MSYS2_ARG_CONV_EXCL='*' "$iscc" /Q "/DAppVersion=$version" "/DStageDir=$(winpath "$PWD/$app")" \
        "/DOutputDir=$(winpath "$PWD/$out")" "$(winpath "$PWD/$here/mtg-duelstone.iss")"
    echo "Built $out/$zip and $out/mtg-duelstone-windows-x86_64-setup.exe"
elif [[ "${REQUIRE_INSTALLER:-0}" == 1 ]]; then
    echo "Inno Setup 6 (ISCC.exe) not found" >&2
    exit 1
else
    echo "Built $out/$zip (no installer: Inno Setup 6 not found)"
fi
