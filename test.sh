#!/bin/sh

export CARGO_INCREMENTAL=0
export RUSTFLAGS='-Cinstrument-coverage'
export LLVM_PROFILE_FILE='cargo-test-%p-%m.profraw'

cargo test --features benchmark,server
grcov . --binary-path ./target/debug -s . -t lcov --branch --ignore-not-existing --llvm --filter covered --ignore "/*" -o ./lcov.info
grcov . --binary-path ./target/debug -s . -t html --branch --ignore-not-existing --llvm --filter covered --ignore "/*" -o ./target/coverage/html
rm *.profraw
