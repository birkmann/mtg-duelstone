#!/usr/bin/env bash
# Get Forge at the pinned release tag and install its modules into the Maven repo so forge-ws-server
# can build. Without network it extracts offline/$FORGE_TAG.tar.gz (written by tools/fetch-offline.sh)
# and installs into offline/m2 (tools/env.sh sets that up).
# Usage: tools/setup-forge.sh [--clone-only]   (idempotent; re-run after bumping FORGE_TAG)
set -euo pipefail
cd "$(dirname "$0")/.."
FORGE_TAG="${FORGE_TAG:-forge-2.0.14}"     # keep in sync with <forge.version> in engine/pom.xml
source tools/env.sh 2>/dev/null || true

if [ ! -e forge ]; then
  if [ -f "offline/$FORGE_TAG.tar.gz" ]; then
    tar xzf "offline/$FORGE_TAG.tar.gz"
  else
    git clone --depth 1 --branch "$FORGE_TAG" https://github.com/Card-Forge/forge.git forge
  fi
else
  echo "./forge already exists (checkout or symlink) — leaving it untouched"
fi
[ "${1:-}" = "--clone-only" ] && exit 0

if ! command -v java >/dev/null || ! command -v mvn >/dev/null; then
  echo "Need JDK 17 + Maven on PATH. Either install them (pacman -S jdk17-openjdk maven) or run tools/install-toolchain.sh" >&2
  exit 1
fi

cd forge
mvn -B -pl forge-core,forge-game,forge-ai,forge-gui -am install -Dmaven.test.skip=true -Dcheckstyle.skip=true
echo "Forge $FORGE_TAG installed to the Maven repo. Assets dir: $(pwd)/forge-gui"
