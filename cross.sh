#!/bin/sh

for target in aarch64-unknown-linux-gnu aarch64-unknown-linux-musl; do
    SKIP_YARN=true cross build \
        --release \
        --target=$target \
        --no-default-features \
        --features use-rustls,chd,cso,ird,rvz,benchmark,server
done

for target in x86_64-pc-windows-gnu x86_64-unknown-linux-musl; do
    SKIP_YARN=true cross build \
        --release \
        --target=$target \
        --no-default-features \
        --features use-rustls,enable-asm,chd,cso,ird,rvz,benchmark,server
done
