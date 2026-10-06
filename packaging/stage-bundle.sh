#!/usr/bin/env bash
# Lays out everything the game needs at runtime next to the binary, in the layout
# mtg_client::launcher::bundle_dirs() expects:
#   DEST/assets/                 the Bevy assets (fonts, textures, shaders, sfx)
#   DEST/engine/forge-ws-server.jar
#   DEST/decks/                  starter decks (the engine finds them by walking up from its jar)
#   DEST/forge/res/              Forge's rules data, card scripts zipped into cardsfolder.zip
#   DEST/jre/                    a jlink'd Java runtime with only the modules the engine uses
#   DEST/web/                    the browser client invitees open (tools/build-web.sh → web/dist),
#                                when it has been built
#   DEST/offline/data/           rulings + showcase data (committed)
#   DEST/offline/img/            card images, only with --with-images: Wizards' art stays out of
#                                public releases; such users run the bundled fetch-offline once
# Shared by build-appimage.sh and build-app.sh.
#   ./packaging/stage-bundle.sh DEST [--with-images]
# Needs JDK 17 (jlink, jdeps, jar; $JAVA_HOME or on PATH), the engine jar and a Forge checkout.
set -euo pipefail
cd "$(dirname "$0")/.."
dest=${1:?usage: stage-bundle.sh DEST [--with-images]}
with_images=${2:-}
jar=${ENGINE_JAR:-engine/target/forge-ws-server.jar}
forge=${FORGE_GUI:-forge/forge-gui}
bin_dir=${JAVA_HOME:+$JAVA_HOME/bin/}

[[ -f "$jar" ]] || { echo "missing $jar: build it with (cd engine && mvn -q -B package)" >&2; exit 1; }
[[ -d "$forge/res/cardsfolder" ]] || { echo "missing $forge/res: run tools/setup-forge.sh" >&2; exit 1; }

rm -rf "$dest"
mkdir -p "$dest/engine" "$dest/forge"
cp -R crates/arena/assets "$dest/assets"
rm -rf "$dest/assets/tools"
cp "$jar" "$dest/engine/forge-ws-server.jar"
cp -R decks "$dest/decks"
cp LICENSE "$dest/"
cp "$forge/LICENSE.txt" "$dest/forge/"
if [[ -f web/dist/index.html ]]; then
    cp -R web/dist "$dest/web"
else
    echo "note: no web/dist (tools/build-web.sh): invitees will need the desktop app" >&2
fi
mkdir -p "$dest/offline"
cp -R offline/README.md "$dest/offline/"
[[ -d offline/data ]] && cp -R offline/data "$dest/offline/"
if [ "$with_images" = "--with-images" ]; then
    [[ -f offline/img/index.json ]] || { echo "missing offline/img: run tools/fetch-offline.sh" >&2; exit 1; }
    mkdir -p "$dest/offline/img"
    cp -R offline/img/index.json offline/img/large offline/img/art_crop offline/img/showcase "$dest/offline/img/"
fi

# Forge's res minus what only its own GUIs use (Adventure mode, music, sounds, skins, tutorials)
# and the card-name translations: the engine runs in English.
mkdir -p "$dest/forge/res"
for d in "$forge"/res/*; do
    case "$(basename "$d")" in
        adventure|music|sound|skins|tutorial|cardsfolder) ;;
        *) cp -R "$d" "$dest/forge/res/" ;;
    esac
done
rm -f "$dest"/forge/res/languages/cardnames-*.txt
# ~30k small card scripts as one zip: Forge reads cardsfolder/cardsfolder.zip when it exists, and
# one file is much faster to copy, mount and drag into Applications.
mkdir -p "$dest/forge/res/cardsfolder"
# `jar cfM` writes a plain zip and comes with the JDK we need anyway.
"${bin_dir}jar" cfM "$dest/forge/res/cardsfolder/cardsfolder.zip" -C "$forge/res/cardsfolder" .

# jdeps finds what the engine's classes reference; the extras are loaded reflectively: extra
# charsets and the zip file system. No TLS: the engine never goes online.
modules=$("${bin_dir}jdeps" --print-module-deps --ignore-missing-deps --multi-release 17 "$jar" | tail -n1)
modules="$modules,jdk.charsets,jdk.zipfs"
echo "jlink modules: $modules"
"${bin_dir}jlink" --add-modules "$modules" --output "$dest/jre" \
    --strip-debug --no-header-files --no-man-pages --compress=2
du -sh "$dest"/* | sed 's/^/  /'
