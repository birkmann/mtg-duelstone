#!/usr/bin/env bash
# Build the engine jar if stale, then run the Bevy app (it autolaunches the engine jar). Extra args go to the app.
#
#   ./run.sh [app args…]        desktop client (+ engine)
#   ./run.sh --web [app args…]  same, after (re)building the browser client if stale, so invite links
#                               work for friends who only have a browser
#   ./run.sh --serve [--port N] [--public-url URL]
#                               no desktop window: just the engine serving the browser client —
#                               everyone, host included, plays at http://<this machine>:<port>/
set -euo pipefail
cd "$(dirname "$0")"
source tools/env.sh 2>/dev/null || true
if [ ! -d forge/forge-gui/res/cardsfolder ]; then echo "Forge not set up — run tools/setup-forge.sh first" >&2; exit 1; fi
JAR=engine/target/forge-ws-server.jar
if [ ! -f "$JAR" ] || [ -n "$(find engine/src engine/pom.xml -newer "$JAR" -type f 2>/dev/null | head -1)" ]; then
  (cd engine && mvn -q -B package)
fi

web_stale() {
  local w=web/dist/mtg-arena_bg.wasm
  [ ! -f "$w" ] || [ -n "$(find crates web/index.html Cargo.toml Cargo.lock -newer "$w" -type f \( -name '*.rs' -o -name '*.toml' -o -name '*.lock' -o -name '*.html' -o -name '*.wgsl' \) 2>/dev/null | head -1)" ]
}

case "${1:-}" in
  --web)
    shift
    if web_stale; then tools/build-web.sh; fi
    ;;
  --serve)
    shift
    if web_stale; then tools/build-web.sh; fi
    exec java -jar "$JAR" --assets forge/forge-gui --static web/dist "$@"
    ;;
esac
exec cargo run --release -p arena -- "$@"
