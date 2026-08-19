#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/env"

sudo apt update
sudo apt install --assume-yes openssl pkg-config g++ make cmake protobuf-compiler libssl-dev libclang-dev libudev-dev git lz4 clang build-essential jq

# Free space on the runner
df -h
sudo apt -y autoremove --purge
sudo apt -y autoclean
sudo rm -rf /usr/share/dotnet || true
sudo rm -rf /opt/ghc || true
sudo rm -rf "/usr/local/share/boost" || true
sudo rm -rf "${AGENT_TOOLSDIRECTORY:-}" || true
df -h

# Install solc (needed by pallet-revive-fixtures build script, pulled in
# transitively by upstream pallet-revive).
mkdir -p "$HOME/.local/bin"
echo "$HOME/.local/bin" >> "$GITHUB_PATH"
export PATH="$HOME/.local/bin:$PATH"

curl -sL --fail -o "$HOME/.local/bin/solc" \
  "https://github.com/ethereum/solidity/releases/download/v${SOLC_VERSION}/solc-static-linux"
chmod +x "$HOME/.local/bin/solc"
solc --version

curl -L --fail -o "$HOME/.local/bin/resolc" \
  "https://github.com/paritytech/revive/releases/download/v${RESOLC_VERSION}/resolc-x86_64-unknown-linux-musl"
chmod +x "$HOME/.local/bin/resolc"
resolc --version
