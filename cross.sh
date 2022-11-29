#!/bin/sh

mkdir -p dist
for target in aarch64-unknown-linux-gnu aarch64-unknown-linux-musl; do
    CROSS_CONTAINER_ENGINE=podman SKIP_YARN=true cross build \
        --release \
        --target $target \
        --no-default-features \
        --features use-rustls,chd,cso,ird,rvz,benchmark,server
done
tar -cJf dist/oxyromon.aarch64-linux-gnu.tar.xz target/aarch64-unknown-linux-gnu/release/oxyromon
tar -cJf dist/oxyromon.aarch64-linux-musl.tar.xz target/aarch64-unknown-linux-musl/release/oxyromon

for target in x86_64-pc-windows-gnu x86_64-unknown-linux-musl; do
    CROSS_CONTAINER_ENGINE=podman SKIP_YARN=true cross build \
        --release \
        --target $target \
        --no-default-features \
        --features use-rustls,enable-asm,chd,cso,ird,rvz,benchmark,server
done
7z a dist/oxyromon.x86_64-windows-gnu.7z target/x86_64-pc-windows-gnu/release/oxyromon.exe
tar -cJf dist/oxyromon.x86_64-linux-musl.tar.xz target/x86_64-unknown-linux-musl/release/oxyromon
