#!/bin/sh

cargo llvm-cov --workspace --features oxyromon/server --lcov --output-path lcov.info
