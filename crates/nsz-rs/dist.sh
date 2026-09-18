#!/bin/bash

set -euo pipefail

# Each target starts from a clean target/: build scripts left by a host build
# are linked against the host's glibc, which older cross images can't run.
mkdir -p dist

export CROSS_CONTAINER_ENGINE=podman
export PATH="/opt/llvm-mingw/llvm-mingw-ucrt/bin/:$PATH"

for target in aarch64-unknown-linux-gnu aarch64-unknown-linux-musl x86_64-unknown-linux-gnu x86_64-unknown-linux-musl; do
    cargo clean
    cross build \
        --release \
        --target $target
    tar -cJf dist/nszrs.${target/-unknown/}.tar.xz target/$target/release/nszrs
done

for target in x86_64-pc-windows-gnullvm; do
    cargo clean
    PATH=/opt/llvm-mingw/llvm-mingw-ucrt/bin/:/usr/bin cross build \
        --release \
        --target $target
    7z a dist/nszrs.${target/-pc/}.7z target/$target/release/nszrs.exe
done

for target in aarch64-apple-darwin x86_64-apple-darwin; do
    cargo clean
    cross build \
        --release \
        --target $target
    rcodesign sign target/$target/release/nszrs
    tar -cJf dist/nszrs.${target/-unknown/}.tar.xz target/$target/release/nszrs
done

cargo clean
