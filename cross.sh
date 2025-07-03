#!/bin/bash

mkdir -p dist

export CROSS_CONTAINER_ENGINE=podman
export PATH="/opt/llvm-mingw/llvm-mingw-ucrt/bin/:$PATH"
export SKIP_YARN=true

for target in aarch64-unknown-linux-gnu aarch64-unknown-linux-musl x86_64-unknown-linux-gnu x86_64-unknown-linux-musl; do
    yarn install
    yarn build
    cross build \
        --release \
        --target $target \
        --features server
    tar -cJf dist/oxyromon.${target/-unknown/}.tar.xz target/$target/release/oxyromon
    cargo clean
done

for target in x86_64-pc-windows-gnullvm; do
    yarn install
    yarn build
    cross build \
        --release \
        --target $target \
        --features server
    7z a dist/oxyromon.${target/-pc/}.7z target/$target/release/oxyromon.exe
    cargo clean
done

for target in aarch64-apple-darwin x86_64-apple-darwin; do
    yarn install
    yarn build
    cross build \
        --release \
        --target $target \
        --features server
    rcodesign sign target/$target/release/oxyromon
    tar -cJf dist/oxyromon.${target/-unknown/}.tar.xz target/$target/release/oxyromon
    cargo clean
done
