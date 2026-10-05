#!/usr/bin/env bash
# Build the browser client into web/dist/ (served by the engine at http://<host>:8080/).
#
#   tools/build-web.sh            # release build (what invitees should get)
#   tools/build-web.sh --dev      # faster build, bigger/slower wasm
#   tools/build-web.sh --opt      # also run wasm-opt -Oz (smaller download, slow to build)
#
# Needs: rustup target wasm32-unknown-unknown, and a wasm-bindgen CLI matching the
# wasm-bindgen crate in Cargo.lock (found on PATH, in trunk's cache, or installed here).
set -euo pipefail
cd "$(dirname "$0")/.."

PROFILE=release
OPT=0
for a in "$@"; do
  case "$a" in
    --dev) PROFILE=dev ;;
    --opt) OPT=1 ;;
    -h|--help) sed -n '2,10p' "$0"; exit 0 ;;
    *) echo "unknown option $a" >&2; exit 2 ;;
  esac
done

rustup target list --installed 2>/dev/null | grep -q wasm32-unknown-unknown || rustup target add wasm32-unknown-unknown

WB_VERSION=$(awk '/^name = "wasm-bindgen"$/{getline; gsub(/version = |"/,""); print; exit}' Cargo.lock)
find_bindgen() {
  for c in "$(command -v wasm-bindgen || true)" "$HOME/.cache/trunk/wasm-bindgen-$WB_VERSION/wasm-bindgen" "$HOME/.cargo/bin/wasm-bindgen"; do
    if [ -n "$c" ] && [ -x "$c" ] && "$c" --version 2>/dev/null | grep -q "$WB_VERSION"; then echo "$c"; return; fi
  done
}
WB=$(find_bindgen)
if [ -z "$WB" ]; then
  echo "[web] installing wasm-bindgen-cli $WB_VERSION (one-time)…"
  cargo install -q wasm-bindgen-cli --version "$WB_VERSION" --locked
  WB="$HOME/.cargo/bin/wasm-bindgen"
fi

echo "[web] cargo build ($PROFILE, wasm32)…"
if [ "$PROFILE" = release ]; then
  cargo build -p arena --bin mtg-arena --release --target wasm32-unknown-unknown
else
  cargo build -p arena --bin mtg-arena --target wasm32-unknown-unknown
fi
TARGET_DIR=$(cargo metadata --format-version 1 --no-deps | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')
SUB=$([ "$PROFILE" = release ] && echo release || echo debug)
WASM="$TARGET_DIR/wasm32-unknown-unknown/$SUB/mtg-arena.wasm"

OUT=web/dist
rm -rf "$OUT"
mkdir -p "$OUT"
echo "[web] wasm-bindgen…"
"$WB" --target web --no-typescript --out-dir "$OUT" --out-name mtg-arena "$WASM"
WASM_FEATURES="--enable-bulk-memory --enable-nontrapping-float-to-int --enable-sign-ext --enable-mutable-globals --enable-reference-types --enable-multivalue"
if command -v wasm-opt >/dev/null; then
  if [ "$OPT" = 1 ]; then
    echo "[web] wasm-opt -Oz (slow)…"
    wasm-opt -Oz --strip-debug --strip-producers $WASM_FEATURES -o "$OUT/mtg-arena_bg.wasm" "$OUT/mtg-arena_bg.wasm"
  else
    # debug sections are a third of the file and the browser never needs them
    wasm-opt -O0 --strip-debug --strip-producers $WASM_FEATURES -o "$OUT/mtg-arena_bg.wasm" "$OUT/mtg-arena_bg.wasm"
  fi
else
  echo "[web] (no wasm-opt on PATH: shipping the unstripped wasm)"
fi
cp web/index.html "$OUT/"
# game assets are fetched over HTTP next to the page (generator scripts stay out)
mkdir -p "$OUT/assets"
(cd crates/arena/assets && tar cf - --exclude=tools .) | (cd "$OUT/assets" && tar xf -)
# the engine serves these .gz twins to browsers that accept gzip (DefaultServlet "precompressed")
gzip -9 -k -f "$OUT/mtg-arena_bg.wasm" "$OUT/mtg-arena.js"
du -sh "$OUT" "$OUT/mtg-arena_bg.wasm" "$OUT/mtg-arena_bg.wasm.gz" | sed 's/^/[web] /'
echo "[web] done → $OUT (the engine serves it automatically; open http://localhost:8080/)"
