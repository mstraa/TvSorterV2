#!/usr/bin/env bash
# Shared build steps for TvSorter (Rust backend + React frontend).
# Sourced by the install and update scripts inside the LXC. Builds the
# frontend bundle first (rust-embed bakes it into the binary), then the
# release binary.
set -Eeuo pipefail

build_frontend() {
  local app_dir="$1"
  echo "[TvSorter Build] Building frontend"
  ( cd "${app_dir}/frontend" && npm ci && npm run build )
}

build_backend() {
  local app_dir="$1"
  echo "[TvSorter Build] Building Rust release binary"
  ( cd "${app_dir}" \
      && CARGO_HOME="${CARGO_HOME:-/opt/rust/cargo}" \
         RUSTUP_HOME="${RUSTUP_HOME:-/opt/rust/rustup}" \
         "${CARGO_HOME:-/opt/rust/cargo}/bin/cargo" build --release )
}

build_app() {
  local app_dir="$1"
  build_frontend "$app_dir"
  build_backend "$app_dir"
}
