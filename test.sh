#!/bin/sh

cargo llvm-cov --features server --lcov --output-path lcov.info
cargo llvm-cov report --open
