#!/bin/sh
# B5 over the corpus's GameCube discs, one at a time: each is copied out of the
# library, checked against its corpus SHA-1, measured, then deleted, so the
# library is only ever read and at most one disc sits on the local disk.
#
#   sh dev/ore-bench/b5.sh [RESULTS]
#
# LIBRARY (default /mnt/server/Emulation) is the corpus's root, WORK (default
# target/b5) the local scratch directory, COUNT (default 20) how many discs,
# spread across the corpus's sizes. Extra ore-bench options go in BENCH_ARGS.
set -eu

LIBRARY=${LIBRARY:-/mnt/server/Emulation}
WORK=${WORK:-target/b5}
COUNT=${COUNT:-20}
RESULTS=${1:-$WORK/results.tsv}

cargo build --release -p ore-bench
mkdir -p "$WORK"
grep -v '^#' formats/corpus/gc-wii.txt | grep 'Nintendo - GameCube/' | sort -k2 -n |
    awk -v n="$COUNT" '{ line[NR] = $0 } END { for (i = 0; i < n && i < NR; i++) print line[int(i * NR / n) + 1] }' \
        >"$WORK/discs.txt"

header=true
while read -r sha1 size path; do
    disc="$WORK/$(basename "$path")"
    case "$(realpath -m "$disc")" in
    "$(realpath -m "$LIBRARY")"/*) echo "refusing to work inside the library: $disc" >&2 && exit 1 ;;
    esac
    echo "copying $path ($size bytes)" >&2
    cp -- "$LIBRARY/$path" "$disc"
    if ! echo "$sha1  $disc" | sha1sum -c --quiet -; then
        echo "skipping $path: copy does not match the corpus" >&2
        rm -f -- "$disc"
        continue
    fi
    # shellcheck disable=SC2086
    if $header; then
        target/release/ore-bench -o "$WORK" ${BENCH_ARGS:-} "$disc" >"$RESULTS"
        header=false
    else
        target/release/ore-bench -o "$WORK" ${BENCH_ARGS:-} "$disc" | tail -n +2 >>"$RESULTS"
    fi
    rm -f -- "$disc"
done <"$WORK/discs.txt"
