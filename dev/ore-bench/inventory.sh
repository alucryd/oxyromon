#!/bin/sh
# The file inventory over B5's GameCube discs and every other disc of their
# sets and regional versions, by way of each-disc.sh.
#
#   sh dev/ore-bench/inventory.sh [RESULTS]
#
# See each-disc.sh for LIBRARY and WORK.
set -eu

WORK=${WORK:-target/inventory}

cargo build --release -p ore-bench
mkdir -p "$WORK"
{
    grep -v '^#' formats/results/2026-11-11-gc-b5/discs.txt
    grep -v '^#' formats/corpus/gc-wii.txt | grep 'Nintendo - GameCube/' |
        grep -E 'Resident Evil 4 \(USA\) \(Disc 1\)|Code - Veronica X \(USA\) \(Disc 1\)|Tales of Symphonia|Baten Kaitos'
} | sort -u -k3 >"$WORK/discs.txt"
WORK=$WORK sh "$(dirname "$0")/each-disc.sh" "$WORK/discs.txt" "${1:-$WORK/inventory.tsv}" \
    target/release/inventory
