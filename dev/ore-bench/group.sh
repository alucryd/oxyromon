#!/bin/sh
# Run a command on a group of discs of a corpus list, all at once: each is
# copied out of the library and checked against its corpus SHA-1, the command
# runs on them all, then they are deleted. The library is only ever read.
#
#   sh dev/ore-bench/group.sh LIST RESULTS COMMAND [ARGS]...
#
# LIST holds corpus lines (`<sha1>  <size>  <path>`), in the order the
# command gets them. See each-disc.sh for LIBRARY and WORK.
set -eu

LIBRARY=${LIBRARY:-/mnt/server/Emulation}
WORK=${WORK:-target/b5}
list=$1
results=$2
shift 2

mkdir -p "$WORK"
set -- "$@" --
while read -r sha1 size disc_path; do
    disc="$WORK/$(basename "$disc_path")"
    case "$(realpath -m "$disc")" in
    "$(realpath -m "$LIBRARY")"/*) echo "refusing to work inside the library: $disc" >&2 && exit 1 ;;
    esac
    echo "copying $disc_path ($size bytes)" >&2
    cp -- "$LIBRARY/$disc_path" "$disc"
    if ! echo "$sha1  $disc" | sha1sum -c --quiet -; then
        echo "copy of $disc_path does not match the corpus" >&2
        rm -f -- "$disc"
        exit 1
    fi
    set -- "$@" "$disc"
done <"$list"
# The command, then its discs after the --
command=""
for arg in "$@"; do
    shift
    if [ "$arg" = "--" ]; then
        break
    fi
    command="$command $(printf '%s' "$arg" | sed "s/'/'\\\\''/g; s/^/'/; s/\$/'/")"
done
status=0
eval "$command \"\$@\"" >"$results" || status=$?
for disc in "$@"; do
    rm -f -- "$disc"
done
exit $status
