#!/bin/sh

mkdir -p dist
cargo build \
    --release \
    --no-default-features \
    --features use-native-tls,enable-asm,chd,cso,ird,rvz,benchmark,server
tar -cJf dist/oxyromon.x86_64-linux-gnu.tar.xz target/release/oxyromon
