#!/bin/bash
# Exercise create-ext4-rootfs.sh's CMake staging block against fake artifacts,
# without building an image.
#
# In scripts/ and not build/ because it is a STANDING CHECK and not a one-off
# measurement -- lane A's rule, learned when `build/survey_reach.py` was cited
# in known-issues.md as "the check" and had never been tracked by git, so it
# vanished at the next clean checkout while the sentence claiming it went on
# reading true. The tell is the tense: "x ESTABLISHED that" survives in prose,
# "THE CHECK IS x" does not.
#
# The branch that matters is the middle one: a binary present with no module
# tree must stage NEITHER, because a cmake without /share/cmake-4.4 fails at
# `--version` and half of this pair is worse than none of it.
set -uo pipefail

SRC="$(dirname "$0")/../scripts/create-ext4-rootfs.sh"
PASS=0
FAIL=0
ok() { PASS=$((PASS + 1)); }
bad() { FAIL=$((FAIL + 1)); echo "FAIL $1"; }

# Extract the block verbatim, so this tests the shipped text and not a copy.
BLOCK="$(awk '/^CMAKE_SLATE=/,/^fi$/' "$SRC")"
if [ -z "$BLOCK" ]; then
    echo "FAIL could not extract the CMAKE_SLATE block from $SRC"
    exit 1
fi

run_case() {
    local label="$1" make_bin="$2" make_data="$3"
    local T
    T="$(mktemp -d)"
    export ROOT_DIR="$T/repo" STAGE="$T/stage"
    mkdir -p "$ROOT_DIR/build/spike" "$STAGE/bin" "$ROOT_DIR/toolchain/sysroot/lib"
    : > "$ROOT_DIR/toolchain/sysroot/lib/libc.a"
    [ "$make_bin" = yes ] && printf 'ELF' > "$ROOT_DIR/build/spike/cmake-slateos.elf"
    if [ "$make_data" = yes ]; then
        mkdir -p "$ROOT_DIR/build/spike/cmake-data/share/cmake-4.4/Modules"
        : > "$ROOT_DIR/build/spike/cmake-data/share/cmake-4.4/Modules/CMake.cmake"
    fi
    OUT="$(eval "$BLOCK" 2>&1)"
    CM_STAGED=no; [ -e "$STAGE/bin/cmake" ] && CM_STAGED=yes
    DATA_STAGED=no; [ -d "$STAGE/share/cmake-4.4/Modules" ] && DATA_STAGED=yes
    echo "--- $label: binary=$CM_STAGED data=$DATA_STAGED"
    rm -rf "$T"
}

# 1. Both present: both staged.
run_case "both present" yes yes | grep -q "binary=yes data=yes" && ok || bad "both present should stage both"

# 2. Binary only: NEITHER staged. This is the whole point of the block.
out="$(run_case "binary only" yes no)"
echo "$out" | grep -q "binary=no data=no" && ok || bad "binary without data must stage neither, got: $out"

# 3. Neither: nothing staged, no error.
run_case "neither" no no | grep -q "binary=no data=no" && ok || bad "neither should stage nothing"

# 4. The half-staged case must SAY so, not fail silently.
T="$(mktemp -d)"
export ROOT_DIR="$T/repo" STAGE="$T/stage"
mkdir -p "$ROOT_DIR/build/spike" "$STAGE/bin" "$ROOT_DIR/toolchain/sysroot/lib"
: > "$ROOT_DIR/toolchain/sysroot/lib/libc.a"
printf 'ELF' > "$ROOT_DIR/build/spike/cmake-slateos.elf"
msg="$(eval "$BLOCK" 2>&1)"
case "$msg" in
    *"staging NEITHER"*) ok ;;
    *) bad "the half-staged case must explain itself, got: $msg" ;;
esac
case "$msg" in
    *"--version"*) ok ;;
    *) bad "the message should name the failure it prevents" ;;
esac
rm -rf "$T"

echo "test-rootfs-staging: $PASS/$((PASS + FAIL)) cases pass"
[ "$FAIL" -eq 0 ]
