#!/usr/bin/env bash
# Mimicway - build from source on Debian/Ubuntu.
# Usage: ./scripts/bootstrap-linux.sh
# Safe to run several times: installed tools are kept.
#
# Rust and Node.js are checked, not installed: their installers are scripts served by their projects, and piping one
# from the network to a shell runs whatever the server sends that day, unpinned. When one is missing, the script says
# where its project documents the install, and stops.
set -euo pipefail

step() { echo -e "\n=== $1 ==="; }
ok()   { echo "  OK: $1"; }
skip() { echo "  SKIP: $1"; }

# True when a version (x.y or x.y.z) is at least a minimum (x.y).
version_at_least() {
    local major minor min_major min_minor
    IFS=. read -r major minor _ <<< "$1"
    IFS=. read -r min_major min_minor _ <<< "$2"
    (( major > min_major || (major == min_major && minor >= min_minor) ))
}

MISSING=0

step "1/6 - Rust toolchain"
# rustup puts cargo in ~/.cargo/bin, which a shell opened before the install does not have on its PATH yet.
source "$HOME/.cargo/env" 2>/dev/null || true
RUST_VERSION=$(rustc --version 2>/dev/null | awk '{print $2}') || true
if command -v cargo &>/dev/null && [ -n "$RUST_VERSION" ] && version_at_least "$RUST_VERSION" 1.85; then
    ok "rustc already installed ($(rustc --version))"
else
    echo "  MISSING: Rust 1.85 or later, with cargo${RUST_VERSION:+ (found $RUST_VERSION)}."
    echo "  Install it with rustup, as https://rustup.rs shows (or update it: rustup update stable)."
    MISSING=1
fi

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
NODE_VERSION=$(node --version 2>/dev/null) || true
NODE_VERSION=${NODE_VERSION#v}
if command -v npm &>/dev/null && [ -n "$NODE_VERSION" ] && version_at_least "$NODE_VERSION" 22.12; then
    ok "Node.js already installed (v$NODE_VERSION)"
else
    echo "  MISSING: Node.js 22.12 or later, with npm${NODE_VERSION:+ (found $NODE_VERSION)}."
    echo "  Install the LTS release as https://nodejs.org/en/download shows (a version manager, a package manager"
    echo "  that provides 22.12 or later, or the official archive)."
    MISSING=1
fi

if [ "$MISSING" -ne 0 ]; then
    echo ""
    echo "Install what is missing above, open a new shell, then run this script again."
    exit 1
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
