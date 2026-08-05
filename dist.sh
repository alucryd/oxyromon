#!/bin/bash

mkdir -p dist

export CROSS_CONTAINER_ENGINE=podman
export PATH="/opt/llvm-mingw/llvm-mingw-ucrt/bin/:$PATH"
export SKIP_TRUNK=true

for target in aarch64-unknown-linux-gnu aarch64-unknown-linux-musl x86_64-unknown-linux-gnu x86_64-unknown-linux-musl; do
    (cd frontend && trunk build --release)
    cross build \
        --release \
        --target $target \
        --features server
    tar -cJf dist/oxyromon.${target/-unknown/}.tar.xz target/$target/release/oxyromon
    cargo clean
done

for target in x86_64-pc-windows-gnullvm; do
    (cd frontend && trunk build --release)
    PATH=/opt/llvm-mingw/llvm-mingw-ucrt/bin/:/usr/bin cross build \
        --release \
        --target $target \
        --features server
    7z a dist/oxyromon.${target/-pc/}.7z target/$target/release/oxyromon.exe
    cargo clean
done

for target in aarch64-apple-darwin x86_64-apple-darwin; do
    (cd frontend && trunk build --release)
    cross build \
        --release \
        --target $target \
        --features server
    rcodesign sign target/$target/release/oxyromon
    tar -cJf dist/oxyromon.${target/-unknown/}.tar.xz target/$target/release/oxyromon
    cargo clean
done

# The desktop app is built for the host only. Tauri links against the platform's
# own webview, which the cross images do not carry and which has no cross
# toolchain to begin with, so the other targets have to be built on their own
# operating system.
#
# NO_STRIP because linuxdeploy bundles a binutils too old to read the .relr.dyn
# sections modern distributions ship, and fails the AppImage on the first
# library it tries to strip.
(cd frontend && trunk build --release)
(cd desktop && NO_STRIP=true cargo tauri build)
cp desktop/target/release/bundle/*/*.{deb,rpm,AppImage} dist/
