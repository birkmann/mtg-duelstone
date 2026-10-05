#!/usr/bin/env bash
# User-local JDK 17 (Temurin) + Maven 3.9 under ~/.local/opt — no root needed. Then: source tools/env.sh
set -euo pipefail
mkdir -p ~/.local/opt && cd ~/.local/opt
if [ ! -d jdk-17.0.20+8 ]; then
  curl -sL -o jdk17.tar.gz "https://api.adoptium.net/v3/binary/latest/17/ga/linux/x64/jdk/hotspot/normal/eclipse?project=jdk"
  tar xzf jdk17.tar.gz && rm jdk17.tar.gz
fi
if [ ! -d apache-maven-3.9.16 ]; then
  curl -sL -o mvn.tar.gz "https://dlcdn.apache.org/maven/maven-3/3.9.16/binaries/apache-maven-3.9.16-bin.tar.gz"
  tar xzf mvn.tar.gz && rm mvn.tar.gz
fi
ls -d ~/.local/opt/jdk-17* ~/.local/opt/apache-maven-*
