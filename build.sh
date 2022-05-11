#!/bin/sh

cargo build \
    --release \
    --no-default-features \
    --features use-native-tls,enable-asm,chd,cso,ird,rvz,benchmark,server
