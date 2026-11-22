#!/bin/sh
# Build the ORE cores locally, as .github/workflows/ore_cores.yml does in CI,
# against this checkout's libsmelt:
#   ./ore_cores.sh [beetle-psx|parallel-ps2|pcee2|dolphin|mesen|mesen-s|sameboy|mgba ...]
# No argument builds them all. The cores are the checkouts next to oxyromon
# (CORES_DIR, default ..), built in place on whatever branch they are on.
set -eu

OXYROMON=$(cd "$(dirname "$0")" && pwd)
CORES_DIR=$(cd "${CORES_DIR:-$OXYROMON/..}" && pwd)
export SMELT_DIR="$OXYROMON/crates/smelt-rs"
JOBS=$(nproc)

beetle_psx() {
    core="$CORES_DIR/beetle-psx-libretro"
    sh "$core/tools/ore/run.sh"
    make -C "$core" -j"$JOBS" HAVE_ORE=1 SMELT_DIR="$SMELT_DIR"
    make -C "$core" clean HAVE_HW=1
    make -C "$core" -j"$JOBS" HAVE_ORE=1 HAVE_HW=1 SMELT_DIR="$SMELT_DIR"
}

parallel_ps2() {
    core="$CORES_DIR/parallel-ps2"
    SANITIZER=address,undefined sh "$core/tests/ore/build.sh"
    make -C "$core" -j"$JOBS" HAVE_ORE=1 SMELT_DIR="$SMELT_DIR"
}

pcee2() {
    core="$CORES_DIR/pcee2-libretro"
    # Static deps as in CI, built once into the core's ignored deps/
    [ -d "$core/deps" ] || PCEE2_SDL_STATIC=1 PCEE2_BUILD_JPEG=1 \
        bash "$core/pcee2-libretro/scripts/build-deps-linux.sh" "$core/deps"
    cmake -S "$core" -B "$core/build" -G Ninja \
        -DCMAKE_BUILD_TYPE=Release \
        -DCMAKE_C_COMPILER=clang -DCMAKE_CXX_COMPILER=clang++ \
        -DENABLE_QT_UI=OFF -DENABLE_TESTS=OFF -DENABLE_LIBRETRO=ON \
        -DCMAKE_PREFIX_PATH="$core/deps" \
        -DSHADERC_STATIC=ON \
        "-DSHADERC_LIBRARY=$core/deps/lib/libshaderc_combined.a" \
        -DDISABLE_ADVANCE_SIMD=ON \
        -DORE=ON -DSMELT_DIR="$SMELT_DIR"
    cmake --build "$core/build" --target pcee2_libretro
}

dolphin() {
    core="$CORES_DIR/dolphin"
    cmake -S "$core" -B "$core/build" -G Ninja \
        -DLIBRETRO=ON -DCMAKE_BUILD_TYPE=Release \
        -DORE=ON -DSMELT_DIR="$SMELT_DIR"
    cmake --build "$core/build"
}

# make does not track ORE=1: objects left by a build without it would be
# relinked as they are, without the reader, so these start clean
make_ore() {
    make -C "$1" clean
    make -C "$1" -j"$JOBS" ORE=1 SMELT_DIR="$SMELT_DIR"
}

mesen() {
    make_ore "$CORES_DIR/Mesen/Libretro"
}

mesen_s() {
    make_ore "$CORES_DIR/Mesen-S/Libretro"
}

sameboy() {
    make_ore "$CORES_DIR/SameBoy/libretro"
}

mgba() {
    core="$CORES_DIR/mgba"
    cmake -S "$core" -B "$core/build" \
        -DCMAKE_BUILD_TYPE=Release -DLIBMGBA_ONLY=ON -DBUILD_LIBRETRO=ON \
        -DORE=ON -DSMELT_DIR="$SMELT_DIR"
    cmake --build "$core/build" --target mgba_libretro -j"$JOBS"
}

cargo build --release --manifest-path "$SMELT_DIR/Cargo.toml"

[ $# -gt 0 ] || set -- beetle-psx parallel-ps2 pcee2 dolphin mesen mesen-s sameboy mgba
for c in "$@"; do
    case "$c" in
        beetle-psx | parallel-ps2 | pcee2 | dolphin | mesen | mesen-s | sameboy | mgba) echo "== $c"; "$(echo "$c" | tr - _)" ;;
        *) echo "unknown core: $c" >&2; exit 1 ;;
    esac
done
