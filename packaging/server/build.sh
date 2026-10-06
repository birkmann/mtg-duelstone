#!/usr/bin/env bash
# Build the public engine image (see Dockerfile): fresh engine jar + browser client, staged with
# stage-bundle.sh, then `docker build` on the staged directory.
#
#   packaging/server/build.sh [--tag mtg-duelstone-server] [--stage-only]
#
# --stage-only stops after staging (target/server-bundle), e.g. to run it without Docker:
#   target/server-bundle/jre/bin/java -jar target/server-bundle/engine/forge-ws-server.jar --public \
#     --assets target/server-bundle/forge --offline offline --static target/server-bundle/web
set -euo pipefail
cd "$(dirname "$0")/../.."
source tools/env.sh 2>/dev/null || true

TAG=mtg-duelstone-server
STAGE_ONLY=0
while [ $# -gt 0 ]; do
  case "$1" in
    --tag) TAG=$2; shift ;;
    --stage-only) STAGE_ONLY=1 ;;
    -h|--help) sed -n '2,10p' "$0"; exit 0 ;;
    *) echo "unknown option $1" >&2; exit 2 ;;
  esac
  shift
done

(cd engine && mvn -q -B package)
# the release build: this is what every visitor downloads
tools/build-web.sh
OUT=target/server-bundle
packaging/stage-bundle.sh "$OUT"
cp packaging/server/Dockerfile "$OUT/Dockerfile"
# the image never needs to build anything, and the context should not drag the Dockerfile in twice
printf 'Dockerfile\n' > "$OUT/.dockerignore"

if [ "$STAGE_ONLY" = 1 ]; then echo "[server] staged → $OUT"; exit 0; fi
docker build -t "$TAG" "$OUT"
echo "[server] built image $TAG — see packaging/server/README.md to run it"
