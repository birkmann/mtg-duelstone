# Source this to get the user-local JDK 17 + Maven on PATH:  source tools/env.sh
# Only when they are installed: on CI (or with a system JDK) a dangling JAVA_HOME would break Maven.
if [ -d "$HOME/.local/opt/jdk-17.0.20+8" ]; then
  export JAVA_HOME="$HOME/.local/opt/jdk-17.0.20+8"
  export PATH="$JAVA_HOME/bin:$PATH"
fi
if [ -d "$HOME/.local/opt/apache-maven-3.9.16" ]; then
  export MAVEN_HOME="$HOME/.local/opt/apache-maven-3.9.16"
  export PATH="$MAVEN_HOME/bin:$PATH"
fi
# Once tools/fetch-offline.sh has captured every Maven artifact, build from that repo and never go
# online (Maven 3.9 reads MAVEN_ARGS on every invocation).
_mtg_root="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")/.." && pwd)"
if [ -f "$_mtg_root/offline/m2/.complete" ]; then
  export MAVEN_ARGS="-o -Dmaven.repo.local=$_mtg_root/offline/m2"
fi
unset _mtg_root
