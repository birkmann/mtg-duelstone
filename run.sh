#!/usr/bin/env bash
# Build the engine jar if stale, then run the Bevy app (it autolaunches the engine jar). Extra args go to the app.
set -euo pipefail
cd "$(dirname "$0")"
source tools/env.sh 2>/dev/null || true
if [ ! -d forge/forge-gui/res/cardsfolder ]; then echo "Forge not set up — run tools/setup-forge.sh first" >&2; exit 1; fi
JAR=engine/target/forge-ws-server.jar
if [ ! -f "$JAR" ] || [ -n "$(find engine/src engine/pom.xml -newer "$JAR" -type f 2>/dev/null | head -1)" ]; then
  (cd engine && mvn -q -B package)
fi
exec cargo run --release -p arena -- "$@"
