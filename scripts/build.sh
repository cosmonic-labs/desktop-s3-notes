#!/usr/bin/env bash
# Build the component and push it to Cosmonic Desktop's built-in registry.
# Usage: scripts/build.sh [registry]   (default: oci.localhost:8200)
set -euo pipefail
cd "$(dirname "$0")/.."
REGISTRY="${1:-oci.localhost:8200}"
NAME="desktop-s3-notes"
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"

# `(implements ..)` import names need wasm-component-ld 0.5.30 or newer; the
# linker bundled with rustc may be older and fails with
# "invalid leading byte (0x2) for import name".
MIN_LD="0.5.30"
LINKER="$(command -v wasm-component-ld || true)"
HAVE="$([ -n "$LINKER" ] && "$LINKER" --version | awk '{print $2}' || echo 0)"
if [ "$(printf '%s\n%s\n' "$MIN_LD" "$HAVE" | sort -V | head -1)" != "$MIN_LD" ]; then
  echo "Installing wasm-component-ld >= $MIN_LD (found: ${HAVE/#0/none})..."
  cargo install --locked wasm-component-ld
  LINKER="$(command -v wasm-component-ld)"
fi
echo "linker: $("$LINKER" --version)"

rustup target add wasm32-wasip2 >/dev/null 2>&1 || true
CARGO_TARGET_WASM32_WASIP2_LINKER="$LINKER" cargo build --release --target wasm32-wasip2

WASM="target/wasm32-wasip2/release/${NAME//-/_}.wasm"
REF="$REGISTRY/apps/$NAME:$VERSION"
wash oci push --insecure "$REF" "$WASM"
echo "Pushed $REF"
