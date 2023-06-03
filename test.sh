#!/bin/sh

cargo llvm-cov --features benchmark,server --lcov --output-path lcov.info
cargo llvm-cov report --open
