#!/usr/bin/env bash
# Download the prepared v0.3.1 build kit and run it locally on a Mac.
set -euo pipefail
cd "$(dirname "$0")"
[[ $(uname -s) == Darwin ]] || { echo 'Run this command on your Mac.' >&2; exit 1; }
[[ $# -le 1 && ( $# -eq 0 || "${1:-}" == --upload ) ]] || {
    echo 'Usage: ./BUILD-MACOS.command [--upload]' >&2
    exit 1
}
export PATH="$HOME/.cargo/bin:/opt/homebrew/bin:/usr/local/bin:$PATH"
command -v gh >/dev/null || { echo 'Install GitHub CLI and the other prerequisites in BUILD-MACOS.txt.' >&2; exit 1; }
gh auth status >/dev/null || { echo 'Run gh auth login with access to birkmann/mtg-duelstone.' >&2; exit 1; }

work="$PWD/target/macos-build-kit-v0.3.1"
mkdir -p "$work"
gh release download v0.3.1 --repo birkmann/mtg-duelstone \
    --pattern mtg-duelstone-macos-build-kit.tar.gz \
    --pattern mtg-duelstone-macos-build-kit.tar.gz.sha256 \
    --dir "$work" --clobber
(
    cd "$work"
    shasum -a 256 -c mtg-duelstone-macos-build-kit.tar.gz.sha256
    tar -xzf mtg-duelstone-macos-build-kit.tar.gz
)
exec bash "$work/mtg-duelstone-0.3.1/BUILD-MACOS.command" "$@"
