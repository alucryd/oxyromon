#!/bin/sh
# Build the oxyromon server binary and stage it where Tauri expects to find the
# sidecar declared as `externalBin` in tauri.conf.json, i.e. suffixed with the
# target triple.
#
# Run automatically by `cargo tauri dev` / `cargo tauri build`; safe to run by
# hand from anywhere.
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
triple=${TARGET_TRIPLE:-$(rustc -vV | sed -n 's/^host: //p')}

case "$triple" in
*-windows-*) suffix=.exe ;;
*) suffix= ;;
esac

cd "$root"
cargo build --release --features server
mkdir -p desktop/binaries
cp "target/release/oxyromon${suffix}" "desktop/binaries/oxyromon-${triple}${suffix}"
echo "staged desktop/binaries/oxyromon-${triple}${suffix}"
