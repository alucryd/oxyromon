#!/bin/sh
# Each CHD's files, as the ORE runs took them, against the DAT's SHA-1:
# a CD's BIN cut at each track's first index into the BINs its cue sheet
# names, and that cue sheet; a DVD's ISO.
#   sh dev/ore-bench/dat-verify.sh DAT WORK <CHD>...
set -u
dat=$1; w=$2; shift 2
sha1_of() {
    grep -F "name=\"$(printf '%s' "$1" | sed 's/&/\&amp;/g')\"" "$dat" |
        sed -n 's/.* sha1="\([0-9a-f]*\)".*/\1/p' | head -n 1
}
size_of() { stat -c %s -- "$1"; }
printf 'disc\tfiles\tmatches\tmismatched\n'
for chd in "$@"; do
    name=$(basename -- "$chd" .chd)
    x="$w/x"; rm -rf -- "$x"; mkdir -p "$x"
    cp -- "$chd" "$x/$name.chd"
    if ! target/release/chdrs -o "$x" "$x/$name.chd" >/dev/null 2>&1; then
        for p in "$(dirname -- "$chd")"/*.chd; do
            [ "$p" = "$chd" ] && continue
            target/release/chdrs -p "$p" -o "$x" "$x/$name.chd" >/dev/null 2>&1 && break
        done
    fi
    rm -f -- "$x/$name.chd"
    lib_cue="$(dirname -- "$chd")/$name.cue"
    if [ -f "$x/$name.cue" ] && [ -f "$lib_cue" ]; then
        if [ "$(grep -c '^FILE' "$x/$name.cue")" = 1 ] && [ "$(grep -c '^FILE' "$lib_cue")" -gt 1 ]; then
            single="$x/$(sed -n 's/^FILE "\(.*\)" BINARY.*/\1/p' "$x/$name.cue")"
            awk '/TRACK/ {t = 1} /INDEX/ && t {split($3, m, ":"); print (m[1] * 60 + m[2]) * 75 + m[3]; t = 0}' \
                "$x/$name.cue" > "$x/.starts"
            echo $(($(size_of "$single") / 2352)) >> "$x/.starts"
            sed -n 's/^FILE "\(.*\)" BINARY.*/\1/p' "$lib_cue" > "$x/.names"
            i=1
            while IFS= read -r file; do
                start=$(sed -n "${i}p" "$x/.starts"); end=$(sed -n "$((i + 1))p" "$x/.starts")
                dd if="$single" of="$x/$file" bs=2352 skip="$start" count=$((end - start)) status=none
                i=$((i + 1))
            done < "$x/.names"
            rm -f -- "$single" "$x/.starts" "$x/.names"
        fi
        cp -- "$lib_cue" "$x/$name.cue"
    fi
    # A DVD's ISO as the DAT names it
    for iso in "$x"/*.iso; do [ -f "$iso" ] && [ "$iso" != "$x/$name.iso" ] && mv -- "$iso" "$x/$name.iso"; done
    files=0; matches=0; bad=""
    for f in "$x"/*; do
        file=$(basename -- "$f"); files=$((files + 1))
        if [ "$(sha1sum -- "$f" | cut -d' ' -f1)" = "$(sha1_of "$file")" ]; then
            matches=$((matches + 1))
        else
            bad="$bad${bad:+, }$file"
        fi
    done
    printf '%s\t%s\t%s\t%s\n' "$name" "$files" "$matches" "$bad"
done
rm -rf -- "$w/x"
