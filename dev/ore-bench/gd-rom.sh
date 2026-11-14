#!/bin/sh
# GD-ROMs through ore-rs end to end. Each CHD is copied out of the library
# and extracted by chdrs, a BIN a track, the library's cue sheet beside them;
# every file is checked against the DAT's SHA-1; the disc is compressed by
# orers in each profile and into one store that every disc before it went
# into; then the first ORE is decompressed and compared file by file.
#
#   sh dev/ore-bench/gd-rom.sh DAT WORK <CHD>...
#
# A row a disc on stdout, sizes in bytes; `store` is the store and its thin
# OREs after the disc. The library is only ever read: everything written
# goes to WORK, and the disc's files are removed once measured.
set -eu

dat=$1
work=$2
shift 2
bin=target/release
# orers makes the store on its first disc
mkdir -p "$work/store" "$work/thin"

# The SHA-1 the DAT gives a file of that name
sha1_of() {
    grep -F "name=\"$(printf '%s' "$1" | sed 's/&/\&amp;/g')\"" "$dat" |
        sed -n 's/.* sha1="\([0-9a-f]*\)".*/\1/p' | head -n 1
}

size_of() { stat -c %s -- "$1"; }

printf 'disc\tchd\ttracks\tbytes\tdat_matches\tdat_files\tz1\tz4\tl4\tstore\tround_trip\n'
for chd in "$@"; do
    name=$(basename -- "$chd" .chd)
    x="$work/x"
    rm -rf -- "$x" "$work/o" "$work/b"
    mkdir -p "$x" "$work/o" "$work/b" "$work/first"
    echo "$name" >&2
    cp -- "$chd" "$x/$name.chd"
    # A clone names its parent only by SHA-1: try its siblings, read in place
    if ! "$bin/chdrs" -o "$x" "$x/$name.chd" >/dev/null 2>&1; then
        for parent in "$(dirname -- "$chd")"/*.chd; do
            [ "$parent" = "$chd" ] && continue
            "$bin/chdrs" -p "$parent" -o "$x" "$x/$name.chd" >/dev/null 2>&1 && break
        done
    fi
    rm -f -- "$x/$name.chd"
    [ -n "$(ls -- "$x")" ] || { echo "$name: no parent found" >&2; continue; }
    cp -- "$(dirname -- "$chd")/$name.cue" "$x/$name.cue"

    files=0
    matches=0
    bytes=0
    tracks=0
    for f in "$x"/*; do
        file=$(basename -- "$f")
        files=$((files + 1))
        case $file in *.bin) tracks=$((tracks + 1)) bytes=$((bytes + $(size_of "$f"))) ;; esac
        [ "$(sha1sum -- "$f" | cut -d' ' -f1)" = "$(sha1_of "$file")" ] && matches=$((matches + 1))
    done

    sizes=""
    for options in "-p stream -c zstd" "-p archive -c zstd" "-p archive -c lzma2"; do
        rm -f -- "$work/o/$name.ore"
        # shellcheck disable=SC2086
        "$bin/orers" $options -o "$work/o" "$x/$name.cue" >/dev/null
        sizes="$sizes	$(size_of "$work/o/$name.ore")"
        # The first kept under its name, which its cue sheet is written under
        [ "$options" = "-p stream -c zstd" ] && mv -- "$work/o/$name.ore" "$work/first/$name.ore"
    done

    "$bin/orers" --store "$work/store/.ore-store" -o "$work/thin" "$x/$name.cue" >/dev/null
    store=$(($(du -sb -- "$work/store" | cut -f1) + $(du -sb -- "$work/thin" | cut -f1)))

    "$bin/orers" -o "$work/b" "$work/first/$name.ore" >/dev/null
    round_trip=true
    for f in "$x"/*; do
        cmp -s -- "$f" "$work/b/$(basename -- "$f")" || round_trip=false
    done
    rm -f -- "$work/first/$name.ore"

    printf '%s\t%s\t%s\t%s\t%s\t%s%s\t%s\t%s\n' "$name" "$(size_of "$chd")" "$tracks" "$bytes" \
        "$matches" "$files" "$sizes" "$store" "$round_trip"
done
rm -rf -- "$work/x" "$work/o" "$work/b" "$work/first"
