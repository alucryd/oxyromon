#!/bin/sh
# B5 over the corpus's GameCube discs, by way of each-disc.sh.
#
#   sh dev/ore-bench/b5.sh [RESULTS]
#
# COUNT (default 20) is how many discs, spread across the corpus's sizes;
# extra ore-bench options go in BENCH_ARGS. See each-disc.sh for LIBRARY and
# WORK.
set -eu

WORK=${WORK:-target/b5}
COUNT=${COUNT:-20}

cargo build --release -p ore-bench
mkdir -p "$WORK"
grep -v '^#' formats/corpus/gc-wii.txt | grep 'Nintendo - GameCube/' | sort -k2 -n |
    awk -v n="$COUNT" '{ line[NR] = $0 } END { for (i = 0; i < n && i < NR; i++) print line[int(i * NR / n) + 1] }' \
        >"$WORK/discs.txt"
# shellcheck disable=SC2086
WORK=$WORK sh "$(dirname "$0")/each-disc.sh" "$WORK/discs.txt" "${1:-$WORK/results.tsv}" \
    target/release/ore-bench -o "$WORK" ${BENCH_ARGS:-}
