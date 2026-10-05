#!/usr/bin/env bash
# User-local JDK 17 (Temurin) + Maven 3.9 under ~/.local/opt — no root needed. Then: source tools/env.sh
# The tarballs are kept in offline/toolchain/ so a later install needs no network.
set -euo pipefail
cache="$(cd "$(dirname "$0")/.." && pwd)/offline/toolchain"
mkdir -p ~/.local/opt "$cache" && cd ~/.local/opt
fetch() { # url file installed-dir: cache the tarball; offline is fine once it's installed
  [ -s "$cache/$2" ] || { curl -fsSL -o "$cache/$2.tmp" "$1" && mv "$cache/$2.tmp" "$cache/$2"; } || [ -d "$3" ]
}
fetch "https://api.adoptium.net/v3/binary/version/jdk-17.0.20%2B8/linux/x64/jdk/hotspot/normal/eclipse?project=jdk" jdk-17.0.20+8.tar.gz jdk-17.0.20+8
fetch "https://archive.apache.org/dist/maven/maven-3/3.9.16/binaries/apache-maven-3.9.16-bin.tar.gz" apache-maven-3.9.16-bin.tar.gz apache-maven-3.9.16
[ -d jdk-17.0.20+8 ] || tar xzf "$cache/jdk-17.0.20+8.tar.gz"
[ -d apache-maven-3.9.16 ] || tar xzf "$cache/apache-maven-3.9.16-bin.tar.gz"
ls -d ~/.local/opt/jdk-17* ~/.local/opt/apache-maven-*
