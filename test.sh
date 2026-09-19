#!/bin/sh

cargo llvm-cov --workspace --features oxyromon/server --lcov --output-path lcov.info
cargo llvm-cov --workspace --features oxyromon/nod,oxyromon/server,oxyromon/sevenz --lcov --output-path lcov.info
