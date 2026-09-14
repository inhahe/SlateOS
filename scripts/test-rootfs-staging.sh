#!/bin/bash
# Exercise create-ext4-rootfs.sh's staging blocks against fake artifacts,
# without building an image: the CMake pair, and the SlateOS-native
# utilities built out of userspace/.
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
    # Output discarded on purpose, and now says so. `run_case` prints one line that
    # every caller greps, so anything the block writes to stdout would corrupt the
    # assertion. This used to capture into an OUT that nothing ever read, which is the
    # same effect written in a way that looks like a forgotten variable -- and SC2034
    # was right to flag it.
    eval "$BLOCK" >/dev/null 2>&1 || true
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

# --- the SlateOS-native userspace block ---------------------------------------
#
# Extracted the same way, and for the same reason: this tests the shipped text
# rather than a copy of it that can drift. The awk range cannot end at /^fi$/
# the way the cmake one does, because this block closes TWO top-level ifs and
# would be cut in half; it ends at the comment that follows it instead.
SLATE_BLOCK="$(awk '/^SLATE_BIN_DIR=/{f=1} /^# --- Completeness/{f=0} f' "$SRC")"
if [ -z "$SLATE_BLOCK" ]; then
    echo "FAIL could not extract the SLATE_BIN_DIR block from $SRC"
    exit 1
fi

# A file whose first four bytes are real ELF magic, which is what the block
# tests. Note that the cmake fixture above -- printf 'ELF' -- is NOT one: it is
# three bytes and lacks the leading 0x7f, so it would be skipped here. That is
# the distinction this block relies on to tell a binary from a depfile.
mk_elf() { printf '\177ELF' > "$1"; }

slate_env() {
    T="$(mktemp -d)"
    export ROOT_DIR="$T/repo" STAGE="$T/stage"
    mkdir -p "$ROOT_DIR/target/x86_64-slateos/release" "$STAGE/bin" \
             "$ROOT_DIR/toolchain/sysroot/lib"
    export SYSROOT_LIBC="$ROOT_DIR/toolchain/sysroot/lib/libc.a"
    # The block reports its share of the image and warns past a quarter of
    # it, so it reads IMG_SIZE. Under `set -u` an unset one is a crash, not
    # a skipped warning -- which is why this is exported rather than left
    # to the real script to define.
    export IMG_SIZE="384M"
    : > "$SYSROOT_LIBC"
}

# 5. Real ELFs reach /bin, and the count is reported.
slate_env
mk_elf "$ROOT_DIR/target/x86_64-slateos/release/ls"
mk_elf "$ROOT_DIR/target/x86_64-slateos/release/cat"
msg="$(eval "$SLATE_BLOCK" 2>&1)"
{ [ -e "$STAGE/bin/ls" ] && [ -e "$STAGE/bin/cat" ]; } && ok \
    || bad "two ELFs should both be staged, got: $msg"
case "$msg" in
    *"staged 2 SlateOS-native"*) ok ;;
    *) bad "the count should be reported, got: $msg" ;;
esac
rm -rf "$T"

# 6. THE REFUSAL PROBE. cargo owns that directory and fills it with depfiles,
# incremental state and bookkeeping. Anything that is not an ELF must not
# reach /bin -- a gate that only ever says yes has not been shown to work.
slate_env
printf 'ls: src/bin/ls.rs\n' > "$ROOT_DIR/target/x86_64-slateos/release/ls.d"
printf 'not an elf\n' > "$ROOT_DIR/target/x86_64-slateos/release/README"
mkdir -p "$ROOT_DIR/target/x86_64-slateos/release/incremental"
msg="$(eval "$SLATE_BLOCK" 2>&1)"
{ [ ! -e "$STAGE/bin/ls.d" ] && [ ! -e "$STAGE/bin/README" ] \
  && [ ! -e "$STAGE/bin/incremental" ]; } && ok \
    || bad "non-ELF entries must not be staged, got: $msg"
case "$msg" in
    *"no binaries found"*) ok ;;
    *) bad "a directory holding no ELF must say so, got: $msg" ;;
esac
rm -rf "$T"

# 7. A name an earlier block already staged is KEPT, not clobbered. lane A's
# boot test asserts on /bin/make, /bin/sh and /bin/tcc by name.
slate_env
mk_elf "$ROOT_DIR/target/x86_64-slateos/release/make"
printf 'the host glibc make' > "$STAGE/bin/make"
msg="$(eval "$SLATE_BLOCK" 2>&1)"
if [ "$(cat "$STAGE/bin/make")" = "the host glibc make" ]; then ok
else bad "an already-staged name must not be overwritten"; fi
case "$msg" in
    *"already staged by an earlier block"*) ok ;;
    *) bad "the collision must be announced, got: $msg" ;;
esac
rm -rf "$T"

# 8. A binary older than the libc it links against proves nothing about the
# current libc -- the same reasoning as the cmake staleness check above.
slate_env
mk_elf "$ROOT_DIR/target/x86_64-slateos/release/ls"
touch -d "2020-01-01" "$ROOT_DIR/target/x86_64-slateos/release/ls"
touch "$SYSROOT_LIBC"
msg="$(eval "$SLATE_BLOCK" 2>&1)"
case "$msg" in
    *"OLDER than the sysroot libc.a"*) ok ;;
    *) bad "a binary older than libc.a must be reported, got: $msg" ;;
esac
rm -rf "$T"

# 9. No target directory at all -- the state every tree is in before its first
# slateos build -- is a NOTE naming the build command, and must NOT fail the
# image build. See the block's own comment for why this is not an exit 1 yet.
slate_env
rm -rf "$ROOT_DIR/target"
msg="$(eval "$SLATE_BLOCK" 2>&1)"
rc=$?
[ "$rc" -eq 0 ] && ok || bad "an absent target dir must not fail the image build"
case "$msg" in
    *"cargo +nightly build --release"*) ok ;;
    *) bad "the NOTE must name the command that fixes it, got: $msg" ;;
esac
rm -rf "$T"

# 10. THE SIZE TRIPWIRE FIRES. Nothing else in create-ext4-rootfs.sh accounts
# for free space, and this block is the largest single consumer of image bytes,
# so an overflow would surface as mke2fs failing partway with a broken image.
# IMG_SIZE is shrunk rather than the fixture grown: a quarter of 4M is 1 MiB,
# so a 2 MiB binary crosses it without writing 96 MiB to disk.
slate_env
export IMG_SIZE="4M"
mk_elf "$ROOT_DIR/target/x86_64-slateos/release/fat"
head -c 2097152 /dev/zero >> "$ROOT_DIR/target/x86_64-slateos/release/fat"
msg="$(eval "$SLATE_BLOCK" 2>&1)"
case "$msg" in
    *"more than a quarter of the 4M image"*) ok ;;
    *) bad "an oversized staging set must warn, got: $msg" ;;
esac
case "$msg" in
    *"mke2fs -d fails PARTWAY"*) ok ;;
    *) bad "the warning must name the failure it prevents, got: $msg" ;;
esac
rm -rf "$T"

# 11. ...AND STAYS QUIET UNDER BUDGET. A tripwire that always fires is not a
# tripwire; this is the half that makes case 10 mean something.
slate_env
export IMG_SIZE="384M"
mk_elf "$ROOT_DIR/target/x86_64-slateos/release/small"
msg="$(eval "$SLATE_BLOCK" 2>&1)"
case "$msg" in
    *"more than a quarter"*) bad "a 4-byte binary must not trip the budget, got: $msg" ;;
    *) ok ;;
esac
case "$msg" in
    *"(0 MiB)"*) ok ;;
    *) bad "the staged size should be reported either way, got: $msg" ;;
esac
rm -rf "$T"

echo "test-rootfs-staging: $PASS/$((PASS + FAIL)) cases pass"
[ "$FAIL" -eq 0 ]
