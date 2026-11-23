#!/bin/sh
# Build the ORE cores locally, as .github/workflows/ore_cores.yml does in CI,
# against this checkout's libsmelt:
#   ./ore_cores.sh [beetle-psx|parallel-ps2|pcee2|dolphin|mesen|mesen-s|sameboy|mgba|melonds-ds|azahar|parallel-n64|cemu|beetle-vb|pokemini|genesis-plus-gx|picodrive ...]
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
# relinked as they are, without the reader, so these start clean.
# make_ore <dir> [make flags...]
make_ore() {
    dir=$1
    shift
    make -C "$dir" clean "$@"
    make -C "$dir" -j"$JOBS" ORE=1 SMELT_DIR="$SMELT_DIR" "$@"
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

parallel_n64() {
    # As the core's own AGENTS.md builds it
    make_ore "$CORES_DIR/parallel-n64" platform=unix WITH_DYNAREC=x86_64 \
        HAVE_THR_AL=1 HAVE_PARALLEL=1 HAVE_PARALLEL_RSP=1
}

beetle_vb() {
    make_ore "$CORES_DIR/beetle-vb-libretro"
}

pokemini() {
    make_ore "$CORES_DIR/PokeMini"
}

genesis_plus_gx() {
    make_ore "$CORES_DIR/Genesis-Plus-GX" -f Makefile.libretro
}

picodrive() {
    core="$CORES_DIR/picodrive"
    # Its CPU cores, libchdr and the libretro glue are submodules
    git -C "$core" submodule update --init
    make_ore "$core" -f Makefile.libretro
}

mgba() {
    core="$CORES_DIR/mgba"
    cmake -S "$core" -B "$core/build" \
        -DCMAKE_BUILD_TYPE=Release -DLIBMGBA_ONLY=ON -DBUILD_LIBRETRO=ON \
        -DORE=ON -DSMELT_DIR="$SMELT_DIR"
    cmake --build "$core/build" --target mgba_libretro -j"$JOBS"
}

melonds_ds() {
    core="$CORES_DIR/melonds-ds"
    # Fetches melonDS and its other dependencies at the first configure
    cmake -S "$core" -B "$core/build" -G Ninja -Wno-deprecated \
        -DCMAKE_BUILD_TYPE=Release -DCMAKE_POLICY_VERSION_MINIMUM=3.5 \
        -DORE=ON -DSMELT_DIR="$SMELT_DIR"
    cmake --build "$core/build" --target melondsds_libretro
}

azahar() {
    core="$CORES_DIR/azahar"
    # The libretro build takes its dependencies from the submodules
    git -C "$core" submodule update --init --recursive
    cmake -S "$core" -B "$core/build" \
        -DCMAKE_BUILD_TYPE=Release -DENABLE_LIBRETRO=ON \
        -DORE=ON -DSMELT_DIR="$SMELT_DIR"
    cmake --build "$core/build" --target citra_libretro -j"$JOBS"
}

cemu() {
    core="$CORES_DIR/cemu-libretro"
    # vcpkg and ZArchive come from the submodules; a system zarchive, older
    # than the fork's, would be found first otherwise
    git -C "$core" submodule update --init --recursive
    cmake -S "$core" -B "$core/build" -G Ninja \
        -DCMAKE_BUILD_TYPE=Release -DENABLE_LIBRETRO=ON \
        -DENABLE_WXWIDGETS=OFF -DENABLE_DISCORD_RPC=OFF -DENABLE_CUBEB=OFF \
        -DENABLE_BLUEZ=OFF -DENABLE_FERAL_GAMEMODE=OFF \
        -DCMAKE_POSITION_INDEPENDENT_CODE=ON \
        -DCMAKE_DISABLE_FIND_PACKAGE_ZArchive=ON \
        -DORE=ON -DSMELT_DIR="$SMELT_DIR"
    cmake --build "$core/build" --target cemu_libretro
}

cargo build --release --manifest-path "$SMELT_DIR/Cargo.toml"

[ $# -gt 0 ] || set -- beetle-psx parallel-ps2 pcee2 dolphin mesen mesen-s sameboy mgba melonds-ds azahar parallel-n64 cemu beetle-vb pokemini genesis-plus-gx picodrive
for c in "$@"; do
    case "$c" in
        beetle-psx | parallel-ps2 | pcee2 | dolphin | mesen | mesen-s | sameboy | mgba | melonds-ds | azahar | parallel-n64 | cemu | beetle-vb | pokemini | genesis-plus-gx | picodrive) echo "== $c"; "$(echo "$c" | tr - _)" ;;
        *) echo "unknown core: $c" >&2; exit 1 ;;
    esac
done
