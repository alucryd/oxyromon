#!/bin/sh

cargo llvm-cov --features server --lcov --output-path lcov.info
cargo llvm-cov --features nod,server,sevenz --lcov --output-path lcov.info
