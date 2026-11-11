#!/bin/sh
# Dedup across discs over the inventory's 30 GameCube discs, by way of
# each-disc.sh, then the totals per scheme.
#
#   sh dev/ore-bench/dedup.sh [TOTALS]
#
# See each-disc.sh for LIBRARY and WORK.
set -eu

WORK=${WORK:-target/dedup}

cargo build --release -p ore-bench
mkdir -p "$WORK"
WORK=$WORK sh "$(dirname "$0")/each-disc.sh" formats/results/2026-11-11-gc-inventory/discs.txt \
    "$WORK/units.tsv" target/release/dedup units
target/release/dedup sum "$WORK/units.tsv" >"${1:-$WORK/totals.tsv}"
