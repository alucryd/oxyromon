#!/bin/sh

mkdir -p dist
cargo build \
    --release \
    --features server
tar -cJf dist/oxyromon.x86_64-linux-gnu.tar.xz target/release/oxyromon
