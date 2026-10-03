#!/usr/bin/env bash
# Mimicway - build from source on Debian/Ubuntu.
# Usage: ./scripts/bootstrap-linux.sh
# Safe to run several times: installed tools are kept.
set -euo pipefail

step() { echo -e "\n=== $1 ==="; }
ok()   { echo "  OK: $1"; }
skip() { echo "  SKIP: $1"; }

step "1/6 - Rust toolchain"
if command -v rustc &>/dev/null; then
    ok "rustc already installed ($(rustc --version))"
else
    echo "  Installing Rust with rustup..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    source "$HOME/.cargo/env"
    ok "Rust installed ($(rustc --version))"
fi
source "$HOME/.cargo/env" 2>/dev/null || true

step "2/6 - System packages (Debian/Ubuntu)"
# A C toolchain for the few crates with C parts. TLS is pure Rust (rustls): no OpenSSL needed.
if command -v apt-get &>/dev/null; then
    NEEDED=""
    for pkg in build-essential pkg-config; do
        dpkg -s "$pkg" &>/dev/null || NEEDED="$NEEDED $pkg"
    done
    if [ -n "$NEEDED" ]; then
        echo "  Installing:$NEEDED"
        sudo apt-get update -qq && sudo apt-get install -y -qq $NEEDED
        ok "Packages installed"
    else
        skip "Packages already present"
    fi
else
    echo "  WARN: apt-get not available. Make sure a C toolchain (gcc or clang, make) and pkg-config are installed."
fi

step "3/6 - Node.js"
if command -v node &>/dev/null; then
    ok "Node.js already installed ($(node --version))"
else
    echo "  Installing Node.js 24 LTS..."
    if command -v curl &>/dev/null; then
        curl -fsSL https://deb.nodesource.com/setup_24.x | sudo -E bash -
        sudo apt-get install -y -qq nodejs
        ok "Node.js installed ($(node --version))"
    else
        echo "  WARN: install Node.js 22.12 or later yourself."
    fi
fi

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

step "4/6 - UI dependencies"
cd "$PROJECT_DIR/frontend"
# Exactly the versions of package-lock.json, without running package install scripts.
npm ci --ignore-scripts
ok "npm ci done"

step "5/6 - UI build"
npm run build
ok "UI built in frontend/dist/"

step "6/6 - Server build"
cd "$PROJECT_DIR"
cargo build --release --locked
ok "Server built in target/release/"

echo ""
echo "================================================================"
echo "  Mimicway is ready. Start it with:"
echo "  ./target/release/mimicway"
echo "  then open http://localhost:7342"
echo "================================================================"
