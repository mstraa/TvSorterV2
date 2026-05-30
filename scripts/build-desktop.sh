#!/usr/bin/env bash
# Build the TvSorter desktop app (Tauri) for the host platform.
#
# This is the "embedded server" build: the produced app starts the same axum
# server used for the LXC deployment on a loopback port and shows it in a native
# webview. It is a STANDALONE deliverable and is never built by the LXC install
# (which only runs `cargo build --release` on the root crate).
#
# Usage:
#   scripts/build-desktop.sh            # bundle for the current OS (.app/.dmg, .msi, .deb/.AppImage)
#   scripts/build-desktop.sh --no-bundle  # build the binary only, skip installers
#
# Cross-compiling for other OSes is not done here — build on (or in CI on) each
# target OS. macOS -> .app/.dmg, Windows -> .msi/.exe, Linux -> .deb/.AppImage.
set -Eeuo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

if ! command -v cargo-tauri >/dev/null 2>&1 && ! cargo tauri --version >/dev/null 2>&1; then
  echo "error: tauri CLI not found. Install it with: cargo install tauri-cli --version '^2'" >&2
  exit 1
fi

echo "[TvSorter Desktop] Building (frontend bundle is built by Tauri's beforeBuildCommand)"
# `tauri build` runs beforeBuildCommand (frontend npm build, which rust-embed
# bakes into the binary) then compiles + bundles the src-tauri crate.
cargo tauri build "$@"

echo "[TvSorter Desktop] Done. Artifacts under src-tauri/target/release/bundle/"
