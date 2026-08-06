#!/bin/sh

(cd frontend && trunk build --release)
cargo build \
    --release \
    --features server
