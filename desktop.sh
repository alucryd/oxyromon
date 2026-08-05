#!/bin/sh

# linuxdeploy bundles a binutils too old to read the .relr.dyn sections modern
# distributions ship, and fails the AppImage on the first library it strips.
export NO_STRIP=true

cd desktop && cargo tauri build
