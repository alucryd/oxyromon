#!/bin/sh
# Dictionaries over a corpus list, by way of each-disc.sh: every disc's
# samples, then the dictionaries trained on them, then every disc's chunks
# compressed with each, then the totals per variant.
#
#   sh dev/ore-bench/dict.sh LIST [TOTALS]
#
# See each-disc.sh for LIBRARY, WORK and MEMORY_MAX.
set -eu

WORK=${WORK:-target/dict}
list=$1

cargo build --release -p ore-bench
mkdir -p "$WORK"
WORK=$WORK sh "$(dirname "$0")/each-disc.sh" "$list" "$WORK/samples.tsv" \
    target/release/dict sample "$WORK/samples"
target/release/dict train "$WORK/samples" "$WORK/dictionaries"
WORK=$WORK sh "$(dirname "$0")/each-disc.sh" "$list" "$WORK/units.tsv" \
    target/release/dict units "$WORK/dictionaries"
target/release/dedup sum "$WORK/units.tsv" >"${2:-$WORK/totals.tsv}"
