#!/usr/bin/env bash
# Run once while online: fills offline/ with everything the game and its build need, so neither
# ever touches the network again (see offline/README.md).
#   toolchain/   JDK 17 + Maven tarballs           (tools/install-toolchain.sh installs from here)
#   forge-*.tar.gz  the pinned Forge source         (tools/setup-forge.sh extracts it if ./forge is missing)
#   m2/          Maven repo: Forge modules + engine deps + plugins  (tools/env.sh switches Maven offline)
#   vendor/      every Rust crate                   (.cargo/config.toml points cargo at it)
#   img/, data/  card images, rulings, showcase     (the fetch-offline bin)
# Idempotent: each step skips what's already there. Pass --images-only to just (re)run the card fetch.
set -euo pipefail
cd "$(dirname "$0")/.."
root=$(pwd)
FORGE_TAG="${FORGE_TAG:-forge-2.0.14}"     # keep in sync with tools/setup-forge.sh
mkdir -p offline

if [ "${1:-}" != "--images-only" ]; then
  echo "== toolchain tarballs"
  tools/install-toolchain.sh
  source tools/env.sh

  echo "== Forge source archive"
  [ -e forge ] || tools/setup-forge.sh --clone-only
  archive="offline/$FORGE_TAG.tar.gz"
  if [ ! -f "$archive" ]; then
    git -C forge/ archive --format=tar.gz --prefix=forge/ -o "$root/$archive.tmp" HEAD
    mv "$archive.tmp" "$archive"
  fi

  echo "== Maven repo (offline/m2)"
  if [ ! -f offline/m2/.complete ]; then
    # Online, into the repo-local m2: records exactly the artifacts and plugins the real build uses.
    export MAVEN_ARGS="-Dmaven.repo.local=$root/offline/m2"
    tools/setup-forge.sh
    (cd engine && mvn -B clean package)
    touch offline/m2/.complete
  fi

  echo "== Rust crates (offline/vendor)"
  mkdir -p .cargo
  rm -f .cargo/config.toml   # vendor must read crates.io, not the previous vendor dir
  cargo vendor --locked --versioned-dirs offline/vendor > offline/cargo-config.toml
  { cat offline/cargo-config.toml; printf '\n[net]\noffline = true\n'; } | sed "s|directory = \"offline/vendor\"|directory = \"$root/offline/vendor\"|" > .cargo/config.toml
fi

echo "== Card images, rulings, showcase"
cargo run --release -p mtg_client --bin fetch-offline -- --out "$root/offline"
du -sh offline/* 2>/dev/null | sed 's/^/  /'
