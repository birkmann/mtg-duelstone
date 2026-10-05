#!/usr/bin/env bash
# Clone Forge at the pinned release tag and install its modules into ~/.m2 so forge-ws-server can build.
# Usage: tools/setup-forge.sh            (idempotent; re-run after bumping FORGE_TAG)
set -euo pipefail
cd "$(dirname "$0")/.."
FORGE_TAG="${FORGE_TAG:-forge-2.0.14}"     # keep in sync with <forge.version> in engine/pom.xml
source tools/env.sh 2>/dev/null || true

if ! command -v java >/dev/null || ! command -v mvn >/dev/null; then
  echo "Need JDK 17 + Maven on PATH. Either install them (pacman -S jdk17-openjdk maven) or run tools/install-toolchain.sh" >&2
  exit 1
fi

if [ ! -e forge ]; then
  git clone --depth 1 --branch "$FORGE_TAG" https://github.com/Card-Forge/forge.git forge
else
  echo "./forge already exists (checkout or symlink) — leaving it untouched"
fi

cd forge
mvn -B -pl forge-core,forge-game,forge-ai,forge-gui -am install -Dmaven.test.skip=true -Dcheckstyle.skip=true
echo "Forge $FORGE_TAG installed to ~/.m2. Assets dir: $(pwd)/forge-gui"
