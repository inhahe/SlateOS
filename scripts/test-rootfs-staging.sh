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
# the way the cmake one does, because this block closes several top-level ifs
# and would be cut short; it ends at the section that follows it instead --
# the fonts, which stage real files, fetched over the network, and have no
# place in a test of the staging logic. (Until 2026-09-28 it ended at the
# completeness check, which the fonts had come to stand in front of, so every
# case below that evaluated the block also fetched or copied 11.5 MB of
# fonts.)
SLATE_BLOCK="$(awk '/^SLATE_BIN_DIR=/{f=1} /^# --- fonts/{f=0} f' "$SRC")"
if [ -z "$SLATE_BLOCK" ]; then
    echo "FAIL could not extract the SLATE_BIN_DIR block from $SRC"
    exit 1
fi

# A file whose first four bytes are real ELF magic, which is what the block
# tests. Note that the cmake fixture above -- printf 'ELF' -- is NOT one: it is
# three bytes and lacks the leading 0x7f, so it would be refused here. That is
# the distinction the block relies on to tell a binary from a depfile.
mk_elf() { printf '\177ELF' > "$1"; }

# The manifest is what decides shipping now, so every case states one.
mk_manifest() {
    mkdir -p "$ROOT_DIR/scripts"
    printf '%s\n' "$@" > "$ROOT_DIR/scripts/rootfs-bin-manifest.txt"
}

slate_env() {
    T="$(mktemp -d)"
    export ROOT_DIR="$T/repo" STAGE="$T/stage"
    mkdir -p "$ROOT_DIR/target/x86_64-slateos/release" "$STAGE/bin" \
             "$ROOT_DIR/toolchain/sysroot/lib" "$ROOT_DIR/scripts"
    export SYSROOT_LIBC="$ROOT_DIR/toolchain/sysroot/lib/libc.a"
    : > "$SYSROOT_LIBC"
    # The block reports its share of the image and warns past a quarter of it,
    # so it reads IMG_SIZE. Under `set -u` an unset one is a crash, not a
    # skipped warning -- which is why this is exported rather than left to the
    # real script to define.
    export IMG_SIZE="384M"
}

# 5. A name in the manifest with a built ELF behind it reaches /bin, and the
# count is reported.
slate_env
mk_manifest ls cat
mk_elf "$ROOT_DIR/target/x86_64-slateos/release/ls"
mk_elf "$ROOT_DIR/target/x86_64-slateos/release/cat"
msg="$(eval "$SLATE_BLOCK" 2>&1)"
{ [ -e "$STAGE/bin/ls" ] && [ -e "$STAGE/bin/cat" ]; } && ok \
    || bad "two listed ELFs should both be staged, got: $msg"
case "$msg" in
    *"staged 2 SlateOS-native"*) ok ;;
    *) bad "the count should be reported, got: $msg" ;;
esac
rm -rf "$T"

# 6. THE COUPLING PROBE, and the reason the manifest exists at all. A binary
# that is BUILT but not listed must not ship. Before the manifest this block
# scanned the build directory, so a measurement build of every userspace crate
# put 204 MiB into /bin and mke2fs could not allocate a block. Building a crate
# to debug it must not change what the operating system contains.
slate_env
mk_manifest ls
mk_elf "$ROOT_DIR/target/x86_64-slateos/release/ls"
mk_elf "$ROOT_DIR/target/x86_64-slateos/release/tcpdump"
mk_elf "$ROOT_DIR/target/x86_64-slateos/release/nano"
msg="$(eval "$SLATE_BLOCK" 2>&1)"
{ [ -e "$STAGE/bin/ls" ] && [ ! -e "$STAGE/bin/tcpdump" ] \
  && [ ! -e "$STAGE/bin/nano" ]; } && ok \
    || bad "an unlisted binary must not ship, got: $msg"
case "$msg" in
    *"staged 1 SlateOS-native"*) ok ;;
    *) bad "only the listed one should be counted, got: $msg" ;;
esac
rm -rf "$T"

# 7. THE REFUSAL PROBE. A manifest names what SHOULD ship; it cannot promise
# the file is a program. cargo owns that directory and fills it with depfiles.
slate_env
mk_manifest ls
printf 'ls: src/bin/ls.rs\n' > "$ROOT_DIR/target/x86_64-slateos/release/ls"
msg="$(eval "$SLATE_BLOCK" 2>&1)"
[ ! -e "$STAGE/bin/ls" ] && ok || bad "a non-ELF must not be staged, got: $msg"
case "$msg" in
    *"is not an ELF binary"*) ok ;;
    *) bad "refusing a non-ELF must say why, got: $msg" ;;
esac
rm -rf "$T"

# 8. A name an earlier block already staged is KEPT, not clobbered. lane A's
# boot test asserts on /bin/make, /bin/sh and /bin/tcc by name, and the 13
# fastpy commands are design-decisions.md §108's "no silent swap".
slate_env
mk_manifest make
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

# 9. A binary older than the libc it links against proves nothing about the
# current libc -- the same reasoning as the cmake staleness check above.
slate_env
mk_manifest ls
mk_elf "$ROOT_DIR/target/x86_64-slateos/release/ls"
touch -d "2020-01-01" "$ROOT_DIR/target/x86_64-slateos/release/ls"
touch "$SYSROOT_LIBC"
msg="$(eval "$SLATE_BLOCK" 2>&1)"
case "$msg" in
    *"OLDER than the sysroot libc.a"*) ok ;;
    *) bad "a binary older than libc.a must be reported, got: $msg" ;;
esac
# ...and REFUSED, not merely reported. It was a warning until 2026-09-14,
# because cargo could not see libc.a as an input and so no command made a
# stale binary fresh -- a gate nobody can satisfy is a gate that gets
# bypassed. userspace/sysroot-dep fixed that, so the refusal is now
# satisfiable and therefore right.
case "$msg" in
    *"refusing to build an image from stale binaries"*) ok ;;
    *) bad "a stale binary must STOP the image, got: $msg" ;;
esac
# ALLOW_STALE_FIXTURES=1 downgrades it, the same knob and meaning as the spike
# gates. Without this case the gate could be fatal-always, which would make an
# escape hatch that is documented and does not work.
# A FRESH STAGE FIRST. The block above already put /bin/ls on the previous
# stage, and a second run over it reports the collision instead of the
# staleness -- so without this reset the case passes or fails for the wrong
# reason. It failed that way when first written. (The first stage's tree goes
# first: `slate_env` makes a new one, and until 2026-10-01 every run of this
# file left the old one in /tmp.)
rm -rf "$T"
slate_env
mk_manifest ls
mk_elf "$ROOT_DIR/target/x86_64-slateos/release/ls"
touch -d "2020-01-01" "$ROOT_DIR/target/x86_64-slateos/release/ls"
touch "$SYSROOT_LIBC"
msg="$(ALLOW_STALE_FIXTURES=1 eval "$SLATE_BLOCK" 2>&1)"
case "$msg" in
    *"packing them anyway"*) ok ;;
    *) bad "ALLOW_STALE_FIXTURES=1 must downgrade the refusal, got: $msg" ;;
esac
rm -rf "$T"

# 10. A listed name with nothing built is NAMED, not silently dropped -- an
# unbuilt crate and a typo look identical from here, and both need saying.
slate_env
mk_manifest ls definitelynotbuilt
mk_elf "$ROOT_DIR/target/x86_64-slateos/release/ls"
msg="$(eval "$SLATE_BLOCK" 2>&1)"
rc=$?
[ "$rc" -eq 0 ] && ok || bad "an unbuilt name must not fail the build (exit $rc)"
case "$msg" in
    *"definitelynotbuilt"*) ok ;;
    *) bad "the missing name must be printed, got: $msg" ;;
esac
rm -rf "$T"

# 11. Nothing built at all -- the state every tree is in before its first
# slateos build -- is an ERROR naming the build commands, since 2026-09-16:
# a boot that ran against an image with an empty /bin cost hours in the ELF
# loader, and the NOTE this used to be was read as a pass. (This case went on
# asserting the NOTE until 2026-09-28, failing, in a test no gate runs.)
slate_env
mk_manifest ls cat
msg="$(eval "$SLATE_BLOCK" 2>&1)"
rc=$?
[ "$rc" -eq 1 ] && ok || bad "an empty build dir must fail the image build (exit $rc)"
case "$msg" in
    *"ERROR"*"cargo +nightly build --release"*) ok ;;
    *) bad "the ERROR must name the commands that fix it, got: $msg" ;;
esac
rm -rf "$T"

# 11b. ...unless ALLOW_EMPTY_SLATE_BIN asks for a deliberately minimal image,
# when it is a NOTE and the build goes on.
slate_env
mk_manifest ls cat
msg="$(export ALLOW_EMPTY_SLATE_BIN=1; eval "$SLATE_BLOCK" 2>&1)"
rc=$?
[ "$rc" -eq 0 ] && ok || bad "ALLOW_EMPTY_SLATE_BIN must let an empty build dir through (exit $rc)"
case "$msg" in
    *"ALLOW_EMPTY_SLATE_BIN is set"*) ok ;;
    *) bad "the NOTE must say why it is not an error, got: $msg" ;;
esac
rm -rf "$T"

# 12. A MISSING MANIFEST IS AN ERROR, not a NOTE. It is tracked in git, so its
# absence is a broken checkout rather than a tree that has not built yet -- the
# distinction the fastpy block above had to learn the hard way.
slate_env
rm -f "$ROOT_DIR/scripts/rootfs-bin-manifest.txt"
mk_elf "$ROOT_DIR/target/x86_64-slateos/release/ls"
msg="$(eval "$SLATE_BLOCK" 2>&1)"
rc=$?
[ "$rc" -ne 0 ] && ok || bad "a missing manifest must fail, not stage nothing quietly"
case "$msg" in
    *"does not exist"*) ok ;;
    *) bad "the missing manifest must be named, got: $msg" ;;
esac
rm -rf "$T"

# 13. Comments, blank lines and trailing whitespace are not binary names. A
# stray CR would otherwise be reported as "not built" rather than as the typo
# it is.
slate_env
printf '# a comment\n\nls   \ncat\t\n' > "$ROOT_DIR/scripts/rootfs-bin-manifest.txt"
mk_elf "$ROOT_DIR/target/x86_64-slateos/release/ls"
mk_elf "$ROOT_DIR/target/x86_64-slateos/release/cat"
msg="$(eval "$SLATE_BLOCK" 2>&1)"
case "$msg" in
    *"staged 2 SlateOS-native"*) ok ;;
    *) bad "comments and padding must not become names, got: $msg" ;;
esac
case "$msg" in
    *"have no built binary"*) bad "a comment must not be reported as missing: $msg" ;;
    *) ok ;;
esac
rm -rf "$T"

# 14. THE SIZE TRIPWIRE FIRES. Nothing else in create-ext4-rootfs.sh accounts
# for free space, and on 2026-09-13 staging 204 MiB produced "mke2fs: Could not
# allocate block in ext2 filesystem" and no image at all. IMG_SIZE is shrunk
# rather than the fixture grown: a quarter of 4M is 1 MiB, so a 2 MiB binary
# crosses it without writing 96 MiB to disk.
slate_env
export IMG_SIZE="4M"
mk_manifest fat
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

# 15. ...AND STAYS QUIET UNDER BUDGET. A tripwire that always fires is not a
# tripwire; this is the half that makes case 14 mean something.
slate_env
mk_manifest small
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

# 16. A GIGABYTE-SUFFIXED IMG_SIZE MUST NOT BREAK THE BUILD. A regression pin,
# not a hypothetical: the first budget did `sed s/[Mm]$//` and left "1G"
# intact, so the arithmetic exited 1 with "value too great for base". The
# warning it guards says to RAISE IMG_SIZE, so the one documented way to act on
# the advice was the one way to break the script.
slate_env
export IMG_SIZE="1G"
mk_manifest x
mk_elf "$ROOT_DIR/target/x86_64-slateos/release/x"
msg="$(eval "$SLATE_BLOCK" 2>&1)"
rc=$?
[ "$rc" -eq 0 ] && ok || bad "IMG_SIZE=1G must not fail the image build (exit $rc)"
case "$msg" in
    *"value too great"*) bad "the G suffix must be parsed, not fed to arithmetic" ;;
    *) ok ;;
esac
rm -rf "$T"

# 17. A size with no unit this can read skips the COMPARISON and says so -- it
# does not skip the report, and it does not fail. The budget is advice.
slate_env
export IMG_SIZE="wat"
mk_manifest x
mk_elf "$ROOT_DIR/target/x86_64-slateos/release/x"
msg="$(eval "$SLATE_BLOCK" 2>&1)"
rc=$?
[ "$rc" -eq 0 ] && ok || bad "an unreadable IMG_SIZE must not fail the build (exit $rc)"
case "$msg" in
    *"budget was NOT checked"*) ok ;;
    *) bad "a skipped budget check must announce itself, got: $msg" ;;
esac
rm -rf "$T"

# 18. THE SHIPPED MANIFEST ITSELF. A corpus check: the real file must parse to
# a plausible number of names, and must not name any of the 13 fastpy commands
# -- listing one would be the silent swap §108 forbids, and the guard in the
# block would then be the only thing standing between us and it.
REAL_MANIFEST="$(dirname "$0")/rootfs-bin-manifest.txt"
if [ -f "$REAL_MANIFEST" ]; then
    real_n="$(grep -cvE '^[[:space:]]*(#|$)' "$REAL_MANIFEST")"
    [ "$real_n" -gt 20 ] && ok \
        || bad "the shipped manifest names only $real_n binaries -- has it lost its contents?"
    fastpy_hit=""
    for n in cat chmod chown grep head ls mkdir mv rm rmdir tail uniq wc sh; do
        if grep -qxE "[[:space:]]*${n}[[:space:]]*" "$REAL_MANIFEST"; then
            fastpy_hit="$fastpy_hit $n"
        fi
    done
    [ -z "$fastpy_hit" ] && ok \
        || bad "the manifest names a command an earlier block owns:$fastpy_hit (§108)"
else
    bad "scripts/rootfs-bin-manifest.txt is missing"
fi

# 19. A multi-call alias gets created. `ar` is `ranlib` and `strip` when
# invoked under those names, and that only works if something makes the name.
# multicall-aliases.py found 167 such names tree-wide that nothing produces.
slate_env
mk_manifest ar "ranlib = ar"
mk_elf "$ROOT_DIR/target/x86_64-slateos/release/ar"
msg="$(eval "$SLATE_BLOCK" 2>&1)"
[ -e "$STAGE/bin/ranlib" ] && ok || bad "the alias should exist, got: $msg"
case "$msg" in
    *"created 1 multi-call alias"*) ok ;;
    *) bad "the alias count should be reported, got: $msg" ;;
esac
rm -rf "$T"

# 20. THE REFUSAL HALF. An alias whose producer is not staged would be a link
# to nothing. It is NAMED, because a manifest that promises a name the image
# does not have is a manifest error and silence is how it stays one.
slate_env
mk_manifest ar "ranlib = nosuchtool"
mk_elf "$ROOT_DIR/target/x86_64-slateos/release/ar"
msg="$(eval "$SLATE_BLOCK" 2>&1)"
[ ! -e "$STAGE/bin/ranlib" ] && ok || bad "an alias with no producer must not be created"
case "$msg" in
    *"no staged producer"*) ok ;;
    *) bad "an orphaned alias must be named, got: $msg" ;;
esac
rm -rf "$T"

# 21. An alias that collides with a real binary does NOT overwrite it. Same
# reasoning as the collision guard above: two things want one name and the
# script is not the place to decide which wins.
slate_env
mk_manifest ar "ranlib = ar"
mk_elf "$ROOT_DIR/target/x86_64-slateos/release/ar"
printf 'a real ranlib' > "$STAGE/bin/ranlib"
msg="$(eval "$SLATE_BLOCK" 2>&1)"
if [ "$(cat "$STAGE/bin/ranlib")" = "a real ranlib" ]; then ok
else bad "an existing name must not be replaced by an alias"; fi
case "$msg" in
    *"already exists"*) ok ;;
    *) bad "the alias collision must be announced, got: $msg" ;;
esac
rm -rf "$T"

# 22. The alias is a LINK to the producer, not an empty file -- a caller
# invoking it must get the same program.
slate_env
mk_manifest ar "ranlib = ar"
mk_elf "$ROOT_DIR/target/x86_64-slateos/release/ar"
printf 'XX' >> "$ROOT_DIR/target/x86_64-slateos/release/ar"
eval "$SLATE_BLOCK" >/dev/null 2>&1
if [ "$(wc -c < "$STAGE/bin/ranlib")" = "$(wc -c < "$STAGE/bin/ar")" ]; then ok
else bad "the alias must carry the producer's bytes"; fi
rm -rf "$T"

# 23. MORE THAN ONE ALIAS. The regression pin for the bug the unit tests could
# not see: the accumulator joined specs with `$(printf ...)`, and command
# substitution strips trailing newlines, so the separator was the empty string
# and all three aliases arrived as one unparseable line. Every case above used
# a single alias, so all of them passed while the image build printed
# "ranlib(->arstrip = arkillall = kill)". One alias is not a test of a list.
slate_env
mk_manifest ar kill "ranlib = ar" "strip = ar" "killall = kill"
mk_elf "$ROOT_DIR/target/x86_64-slateos/release/ar"
mk_elf "$ROOT_DIR/target/x86_64-slateos/release/kill"
msg="$(eval "$SLATE_BLOCK" 2>&1)"
{ [ -e "$STAGE/bin/ranlib" ] && [ -e "$STAGE/bin/strip" ]   && [ -e "$STAGE/bin/killall" ]; } && ok     || bad "all three aliases should exist, got: $msg"
case "$msg" in
    *"created 3 multi-call alias"*) ok ;;
    *) bad "three aliases should be counted, got: $msg" ;;
esac
case "$msg" in
    *"no staged producer"*) bad "no alias should be orphaned here, got: $msg" ;;
    *) ok ;;
esac
rm -rf "$T"


# 24. A NAME TWO PACKAGES BUILD ships as the userspace/<name> crate's build or
# not at all. Cargo keeps whichever of the pair it linked last, and until the
# build was split in two the image got coreutils' `kill`, which has no
# `killall`. Which copy `release/kill` is, is read off the deps/ file it is a
# copy of, and that file's dep-info -- not off `release/kill.d`.
slate_env
D="$ROOT_DIR/target/x86_64-slateos/release/deps"
mkdir -p "$ROOT_DIR/userspace/coreutils/src/bin" "$ROOT_DIR/userspace/kill" "$D"
: > "$ROOT_DIR/userspace/coreutils/src/bin/kill.rs"
: > "$ROOT_DIR/userspace/kill/Cargo.toml"
mk_manifest kill
printf '\177ELF coreutils' > "$D/kill-c0c0"
printf 'x: userspace\\coreutils\\src\\bin\\kill.rs\n' > "$D/kill-c0c0.d"
printf '\177ELF standalone' > "$D/kill-5a5a"
printf 'x: userspace\\kill\\src\\main.rs\n' > "$D/kill-5a5a.d"
cp "$D/kill-c0c0" "$ROOT_DIR/target/x86_64-slateos/release/kill"
# The dep-info beside the binary says "standalone", as it did on 2026-09-28,
# and is not believed.
cp "$D/kill-5a5a.d" "$ROOT_DIR/target/x86_64-slateos/release/kill.d"
msg="$(eval "$SLATE_BLOCK" 2>&1)"
rc=$?
[ ! -e "$STAGE/bin/kill" ] && ok || bad "coreutils' copy of a doubly-built name must not ship"
case "$rc:$msg" in
    1:*"crate's build: kill"*"-p ar -p kill -p logger -p logrotate -p powerctl"*) ok ;;
    *) bad "the wrong copy must be fatal, named, with the commands, got rc=$rc: $msg" ;;
esac
# ...and the standalone copy, linked last, ships.
cp "$D/kill-5a5a" "$ROOT_DIR/target/x86_64-slateos/release/kill"
msg="$(eval "$SLATE_BLOCK" 2>&1)"
rc=$?
{ [ "$rc" -eq 0 ] && cmp -s "$STAGE/bin/kill" "$D/kill-5a5a"; } && ok \
    || bad "the standalone copy should ship, got rc=$rc: $msg"
rm -rf "$T"

# 25. A name only one package builds is not checked: no deps/ file, no
# dep-info, and it still ships.
slate_env
mkdir -p "$ROOT_DIR/userspace/coreutils/src/bin"
: > "$ROOT_DIR/userspace/coreutils/src/bin/date.rs"
mk_manifest date
mk_elf "$ROOT_DIR/target/x86_64-slateos/release/date"
msg="$(eval "$SLATE_BLOCK" 2>&1)"
[ -e "$STAGE/bin/date" ] && ok || bad "a singly-built name must ship unchecked, got: $msg"
rm -rf "$T"

# 26. THE THEMES. Every folder under gui/appearance/themes lands under
# /usr/share/slateos/themes byte for byte, and as data: 0644 files in 0755
# directories, whatever mode the tree was read with -- WSL reads the NTFS one
# as 0777 throughout.
THEMES_BLOCK="$(awk '/^THEMES_SRC=/{f=1} /^# --- notices/{f=0} f' "$SRC")"
if [ -n "$THEMES_BLOCK" ]; then ok; else bad "could not extract the THEMES_SRC block from $SRC"; fi
T="$(mktemp -d)"
export ROOT_DIR="$T/repo" STAGE="$T/stage"
TH="$ROOT_DIR/gui/appearance/themes"
mkdir -p "$TH/aero/icons" "$TH/night" "$STAGE"
printf 'name: Aero\n' > "$TH/aero/theme.yaml"
printf '<svg/>' > "$TH/aero/icons/a.svg"
printf 'name: Night\n' > "$TH/night/theme.yaml"
chmod -R 0777 "$ROOT_DIR/gui"
msg="$(eval "$THEMES_BLOCK" 2>&1)"
D="$STAGE/usr/share/slateos/themes"
{ cmp -s "$D/aero/theme.yaml" "$TH/aero/theme.yaml" \
    && cmp -s "$D/aero/icons/a.svg" "$TH/aero/icons/a.svg" \
    && cmp -s "$D/night/theme.yaml" "$TH/night/theme.yaml"; } && ok \
    || bad "every theme should be staged whole, got: $msg"
{ [ "$(stat -c %a "$D/aero/theme.yaml")" = 644 ] && [ "$(stat -c %a "$D/aero/icons")" = 755 ] \
    && [ "$(stat -c %a "$D/aero/icons/a.svg")" = 644 ]; } && ok \
    || bad "themes are data: 0644 files in 0755 directories"
case "$msg" in
    *"staged 2 colour theme(s)"*"(3 files)"*) ok ;;
    *) bad "the count should be reported, got: $msg" ;;
esac
rm -rf "$T"

# 27. A tree without the built-in theme is a broken checkout, and says so
# rather than shipping an image with no themes.
T="$(mktemp -d)"
export ROOT_DIR="$T/repo" STAGE="$T/stage"
mkdir -p "$ROOT_DIR" "$STAGE"
msg="$(eval "$THEMES_BLOCK" 2>&1)"
rc=$?
case "$rc:$msg" in
    1:*"checkout is broken"*) ok ;;
    *) bad "a missing theme tree must be fatal, got rc=$rc: $msg" ;;
esac
rm -rf "$T"

# 28. THE NOTICES. scripts/gather-notices.py writes the third-party notices
# into /usr/share/licenses; here a stand-in for it records what it was given
# -- its arguments, and the CARGO_HOME it ran with -- and writes a bundle of
# two components with modes a umask of 077 would give.
NOTICES_BLOCK="$(awk '/^# --- notices:/{f=1} /^# --- Completeness/{f=0} f' "$SRC")"
if [ -n "$NOTICES_BLOCK" ]; then ok; else bad "could not extract the notices block from $SRC"; fi
case "$THEMES_BLOCK" in
    *gather-notices*) bad "the themes block should end where the notices begin" ;;
    *) ok ;;
esac
TEST_PY="$(command -v python3 || command -v python)"
# Is the path the stand-in recorded the one meant? Under Git Bash the Python
# is a Windows one, and MSYS hands it `/tmp/x` as `C:/Users/.../Temp/x`, in
# its arguments and its environment alike.
same_path() {
    [ "$1" = "$2" ] && return 0
    command -v cygpath >/dev/null 2>&1 && [ "$1" = "$(cygpath -m "$2")" ]
}
notices_env() {
    T="$(mktemp -d)"
    export ROOT_DIR="$T/repo" STAGE="$T/stage" NOTICES_TEST_RECORD="$T/record"
    SYSROOT_PY="$TEST_PY"
    mkdir -p "$ROOT_DIR/scripts" "$STAGE"
    cat > "$ROOT_DIR/scripts/gather-notices.py" <<'PY'
import os
import sys
from pathlib import Path

Path(os.environ["NOTICES_TEST_RECORD"]).write_text(
    " ".join(sys.argv[1:]) + "\n" + os.environ.get("CARGO_HOME", "<unset>") + "\n")
if os.environ.get("NOTICES_TEST_FAIL"):
    sys.exit(int(os.environ["NOTICES_TEST_FAIL"]))
out = Path(sys.argv[2])
for component, text in (("comp-a", "LICENSE"), ("comp-b", "COPYING")):
    (out / component).mkdir(parents=True)
    (out / component / text).write_text("text\n")
    os.chmod(out / component / text, 0o600)
    os.chmod(out / component, 0o700)
(out / "index.yaml").write_text("comp-a: {}\ncomp-b: {}\n")
(out / "NOTICES.txt").write_text("both\n")
PY
}
notices_env
export SLATEOS_CARGO_HOME="$T/cache"
msg="$(eval "$NOTICES_BLOCK" 2>&1)"
rc=$?
unset SLATEOS_CARGO_HOME
D="$STAGE/usr/share/licenses"
{ [ "$rc" -eq 0 ] && [ -f "$D/index.yaml" ] && [ -f "$D/NOTICES.txt" ] \
    && [ -f "$D/comp-a/LICENSE" ] && [ -f "$D/comp-b/COPYING" ]; } && ok \
    || bad "the bundle should be staged under /usr/share/licenses, got rc=$rc: $msg"
{ [ "$(head -1 "$T/record" | cut -d' ' -f1)" = "--out" ] \
    && same_path "$(head -1 "$T/record" | cut -d' ' -f2-)" "$D"; } && ok \
    || bad "the gatherer should be asked for --out \$STAGE/usr/share/licenses, got: $(head -1 "$T/record")"
same_path "$(sed -n 2p "$T/record")" "$T/cache" && ok \
    || bad "SLATEOS_CARGO_HOME should be the gatherer's CARGO_HOME, got: $(sed -n 2p "$T/record")"
{ [ "$(stat -c %a "$D/comp-a")" = 755 ] && [ "$(stat -c %a "$D/comp-a/LICENSE")" = 644 ] \
    && [ "$(stat -c %a "$D/index.yaml")" = 644 ]; } && ok \
    || bad "notices are data: 0644 files in 0755 directories"
case "$msg" in
    *"staged the notices of 2 component(s) under /usr/share/licenses (4 files)"*) ok ;;
    *) bad "the count should be reported, got: $msg" ;;
esac
rm -rf "$T"

# 29. Without SLATEOS_CARGO_HOME, the registry is Windows cargo's when the
# recipe runs under WSL -- cmd.exe and wslpath both there -- and otherwise
# cargo's own, whatever CARGO_HOME the recipe was given.
notices_env
export CARGO_HOME="$T/env-cache"
msg="$(eval "$NOTICES_BLOCK" 2>&1)"
rc=$?
unset CARGO_HOME
seen="$(sed -n 2p "$T/record")"
if command -v cmd.exe >/dev/null 2>&1 && command -v wslpath >/dev/null 2>&1; then
    case "$rc:$seen" in
        0:/mnt/?/*) ok ;;
        *) bad "under WSL the registry should be Windows cargo's, got rc=$rc, CARGO_HOME=$seen: $msg" ;;
    esac
else
    { [ "$rc" -eq 0 ] && same_path "$seen" "$T/env-cache"; } && ok \
        || bad "off WSL the registry should be cargo's own, got rc=$rc, CARGO_HOME=$seen: $msg"
fi
rm -rf "$T"

# 30. A gatherer that fails fails the image, with its exit status named:
# an image missing a notice is what this exists to prevent.
notices_env
export NOTICES_TEST_FAIL=3
msg="$(eval "$NOTICES_BLOCK" 2>&1)"
rc=$?
unset NOTICES_TEST_FAIL
case "$rc:$msg" in
    1:*"gather-notices.py failed (exit 3"*) ok ;;
    *) bad "a failed gather must be fatal, got rc=$rc: $msg" ;;
esac
rm -rf "$T"

# 31. ...as does having no Python to gather with.
notices_env
# shellcheck disable=SC2034  # read by the block `eval` runs
SYSROOT_PY=""
msg="$(eval "$NOTICES_BLOCK" 2>&1)"
rc=$?
case "$rc:$msg" in
    1:*"no python3/python"*) ok ;;
    *) bad "no python must be fatal, got rc=$rc: $msg" ;;
esac
[ ! -e "$T/record" ] && ok || bad "with no python, nothing should have been run"
rm -rf "$T"

# 32. THE SYSROOT CHECK WITHOUT PYTHON. libc.a is behind its inputs when one
# of them is newer -- posix's own sources, and its path dependencies, read out
# of posix/Cargo.toml: a sibling crate (`../tzrules`) and one inside posix/
# (`vendor/libm`). Run with no python on PATH, in a tree whose path has a space
# in it, as every real one does ("visual studio projects"). Until 2026-10-01
# the dependencies were a word list that the space split apart, and the one
# inside posix/ was skipped, so neither case below was ever caught.
SYSROOT_BLOCK="$(awk '/^LIBC_A=/{f=1} f{print} f && /^fi$/{exit}' "$SRC")"
case "$SYSROOT_BLOCK" in
    *"_dep_roots"*"no python3/python"*) ok ;;
    *) bad "could not extract the sysroot check from $SRC" ;;
esac
# A PATH with sed and find on it and no python: wrappers that run the real
# tools by their full paths, which works on Linux and under MSYS alike. Each
# case runs in the subshell `$(...)` makes, so it removes its own tree.
sysroot_case() {  # sysroot_case <file made newer than libc.a, or "">
    T="$(mktemp -d)"
    trap 'rm -rf "$T"' EXIT
    local bin="$T/bin" tool
    mkdir -p "$bin"
    for tool in sed find; do
        printf '#!/bin/sh\nexec "%s" "$@"\n' "$(command -v "$tool")" > "$bin/$tool"
        chmod +x "$bin/$tool"
    done
    export ROOT_DIR="$T/visual studio projects/os"
    mkdir -p "$ROOT_DIR/toolchain/sysroot/lib" "$ROOT_DIR/posix/src" \
             "$ROOT_DIR/tzrules/src" "$ROOT_DIR/posix/vendor/libm/src"
    printf '[dependencies]\ntzrules = { path = "../tzrules" }\nlibm = { path = "vendor/libm", features = ["x"] }\n' \
        > "$ROOT_DIR/posix/Cargo.toml"
    : > "$ROOT_DIR/posix/src/lib.rs"
    : > "$ROOT_DIR/tzrules/src/lib.rs"
    : > "$ROOT_DIR/posix/vendor/libm/src/lib.rs"
    : > "$ROOT_DIR/toolchain/sysroot/lib/libc.a"
    find "$ROOT_DIR" -type f -exec touch -d @1600000000 {} +
    touch -d @1650000000 "$ROOT_DIR/toolchain/sysroot/lib/libc.a"
    [ -n "$1" ] && touch -d @1700000000 "$ROOT_DIR/$1"
    # The block's own `command -v` decides there is no python, from this PATH.
    PATH="$bin" eval "$SYSROOT_BLOCK" > "$T/out" 2>&1
    echo "STALE=[$SYSROOT_STALE] PY=[$SYSROOT_PY]"
}
for newer in "tzrules/src/lib.rs" "posix/vendor/libm/src/lib.rs" "posix/src/lib.rs"; do
    got="$(sysroot_case "$newer")"
    [ "$got" = "STALE=[$newer] PY=[]" ] && ok \
        || bad "without python, $newer newer than libc.a should make it stale, got: $got"
done
got="$(sysroot_case "")"
[ "$got" = "STALE=[] PY=[]" ] && ok || bad "without python, a current libc.a is not stale, got: $got"

echo "test-rootfs-staging: $PASS/$((PASS + FAIL)) cases pass"
[ "$FAIL" -eq 0 ]
