#!/bin/sh
# Run a command on each disc of a corpus list, one at a time: each is copied
# out of the library, checked against its corpus SHA-1, given to the command,
# then deleted, so the library is only ever read and at most one disc sits on
# the local disk.
#
#   sh dev/ore-bench/each-disc.sh LIST RESULTS COMMAND [ARGS]...
#
# LIST holds corpus lines (`<sha1>  <size>  <path>`). COMMAND runs as
# `COMMAND [ARGS]... DISC` and prints a header line, then rows; RESULTS gets
# the header once and every row. LIBRARY (default /mnt/server/Emulation) is
# the corpus's root, WORK (default target/b5) the local scratch directory.
set -eu

LIBRARY=${LIBRARY:-/mnt/server/Emulation}
WORK=${WORK:-target/b5}
list=$1
results=$2
shift 2

# Every command runs under a memory limit where systemd can set one, and
# without swap, so that a run that outgrows it is killed rather than taking
# the machine down: MEMORY_MAX (default 12G).
cap=""
if command -v systemd-run >/dev/null 2>&1; then
    cap="systemd-run --user --scope --quiet -p MemoryMax=${MEMORY_MAX:-12G} -p MemorySwapMax=0"
fi

mkdir -p "$WORK"
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
    if $header; then
        $cap "$@" "$disc" >"$results"
        header=false
    else
        $cap "$@" "$disc" | tail -n +2 >>"$results"
    fi
    rm -f -- "$disc"
done <"$list"
