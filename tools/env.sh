# Source this to get the user-local JDK 17 + Maven on PATH:  source tools/env.sh
export JAVA_HOME="$HOME/.local/opt/jdk-17.0.20+8"
export MAVEN_HOME="$HOME/.local/opt/apache-maven-3.9.16"
export PATH="$JAVA_HOME/bin:$MAVEN_HOME/bin:$PATH"
