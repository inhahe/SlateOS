#!/usr/bin/env bash
# Differential test: our `wipefs` against util-linux 2.39.3's.
#
# Both list signatures, and both erase them, so both halves are compared:
#
#   * listing, on util-linux's own test images (tests/ts/blkid) and on
#     images with more than one signature -- the default table, -p, -J, -i,
#     -O with every column, -t filters, several devices, and refusals;
#   * erasing, each side on its own copy of the image in its own directory
#     (so the name in every message is the same): -a, -n, -o, -t, -q, -f,
#     --backup (with HOME pointed into that directory) and --lock -- and
#     after each, the image's bytes and the backup files are compared too.
#
# Images are regular files, so a partition table on one is a "nested"
# table on a device that is not a whole disk, which wipefs leaves alone
# without --force: both halves of that are cases. Nothing needs root.
set -u

DIFF_PROG='wipefs'
DIFF_PKG='wipefs'
DIFF_NEED='timeout xz'
# shellcheck source=util-linux-source.sh
. "$(dirname "$0")/util-linux-source.sh"
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

tsdir=$UL_SRC/tests/ts/blkid
if ! [ -d "$tsdir/images-fs" ]; then
  echo "wipefs-diff.sh: util-linux 2.39.3's source is not available; skipped"
  exit 0
fi

pass=0; fail=0

fx=$DIFF_TMP/fx
mkdir -p "$fx"
for f in "$tsdir"/images-fs/*.img.xz; do
  xz -dc "$f" > "$fx/$(basename "$f" .img.xz).img"
done
for f in "$tsdir"/images-pt/*.img.xz; do
  xz -dc "$f" > "$fx/pt-$(basename "$f" .img.xz).img"
done
# More than one signature: an ext3 superblock with a swap signature over it,
# and a DOS table in front of a vfat boot sector's copy.
if [ -f "$fx/ext3.img" ]; then
  cp "$fx/ext3.img" "$fx/multi.img"
  printf '\001\000\000\000' | dd of="$fx/multi.img" bs=1 seek=1024 conv=notrunc status=none
  printf 'SWAPSPACE2' | dd of="$fx/multi.img" bs=1 seek=4086 conv=notrunc status=none
fi

# One side's run, in its own directory, on its own copy called `img` (and
# `img2`, for two-device cases).
run_side() {
  local side=$1 src=$2 src2=$3; shift 3
  local d=$DIFF_TMP/run-$side
  rm -rf "$d"; mkdir -p "$d/home"
  [ -n "$src" ] && cp "$fx/$src" "$d/img"
  [ -n "$src2" ] && cp "$fx/$src2" "$d/img2"
  (cd "$d" && diff_run timeout -k 2 30 env LC_ALL=C.UTF-8 HOME="$d/home" \
    ${LOCKENV:+LOCK_BLOCK_DEVICE=$LOCKENV} PATH="$bindir/$side" wipefs "$@")
}

# The state a run left: the images' bytes and every backup file.
state() {
  local d=$DIFF_TMP/run-$1
  for f in "$d/img" "$d/img2"; do
    [ -f "$f" ] && sha256sum < "$f"
  done
  for f in "$d"/home/*; do
    [ -f "$f" ] && { basename "$f"; od -An -tx1 "$f"; }
  done
}

compare() {
  local src=$1 src2=$2; shift 2
  local o_out=$DIFF_TMP/o.out g_out=$DIFF_TMP/g.out o_err=$DIFF_TMP/o.err g_err=$DIFF_TMP/g.err
  local o_rc g_rc
  run_side ours "$src" "$src2" "$@" </dev/null >"$o_out" 2>"$o_err"; o_rc=$?
  state ours >>"$o_out"
  run_side gnu "$src" "$src2" "$@" </dev/null >"$g_out" 2>"$g_err"; g_rc=$?
  state gnu >>"$g_out"
  if cmp -s "$o_out" "$g_out" && cmp -s "$o_err" "$g_err" && [ "$o_rc" = "$g_rc" ]; then
    pass=$((pass + 1))
    [ -n "${VERBOSE:-}" ] && printf 'OK   [%s %s] wipefs %s\n' "$src" "$src2" "$*"
  else
    fail=$((fail + 1))
    printf 'DIFF [%s %s] wipefs %s\n  ours rc=%s, gnu rc=%s\n' "$src" "$src2" "$*" "$o_rc" "$g_rc"
    diff -u "$g_out" "$o_out" | sed -n '3,14p' | sed 's/^/  out /'
    diff -u "$g_err" "$o_err" | sed -n '3,8p' | sed 's/^/  err /'
  fi
  return 0
}

# One image as `img`: w IMAGE ARGS... runs `wipefs ARGS... img`.
w() { local src=$1; shift; compare "$src" "" "$@" img; }
# Two images: w2 IMAGE1 IMAGE2 ARGS...
w2() { local a=$1 b=$2; shift 2; compare "$a" "$b" "$@" img img2; }
# No image at all.
w0() { compare "" "" "$@"; }

# --- options and refusals -------------------------------------------------------------
w0 -h
w0 --help
w0 -V
w0 --version
w0
w0 --bogus
w0 -x
w0 -O
w ext4.img -O bogus
w ext4.img -O TYPE,bogus,UUID
w ext4.img -a -O TYPE
w ext4.img -a -o 0x438
w ext4.img -o x
w ext4.img -o -1
w ext4.img -b
w ext4.img --lock=bogus -a
w0 nonexistent.img
w0 .

# --- listing ----------------------------------------------------------------------------
for img in $(cd "$fx" && ls ./*.img | sed 's|^\./||'); do
  w "$img"
done
for args in '-p' '-J' '-i' '-p -i' '-J -i' '-O UUID,LABEL,LENGTH,TYPE,OFFSET,USAGE,DEVICE' \
            '-O length,offset -J' '-t ext3' '-t noext3' '-t swap,ext3' '-t nosuch' '-q'; do
  # shellcheck disable=SC2086  # word-splitting is the point
  w multi.img $args
  # shellcheck disable=SC2086
  w pt-gpt.img $args
done
w2 ext4.img vfat.img
w2 ext4.img vfat.img -J
w2 ext4.img vfat.img -p

# --- erasing ----------------------------------------------------------------------------
for img in ext4.img vfat.img multi.img xfs.img btrfs.img swap1.img lvm2.img pt-gpt.img pt-dos+bsd.img iso.img; do
  [ -f "$fx/$img" ] || continue
  w "$img" -a
  w "$img" -a -n
  w "$img" -a -f
  w "$img" -a -q
  w "$img" -a -b
done
w ext4.img -o 0x438
w ext4.img -o 1080
w ext4.img -o 0x438 -o 0x10
w ext4.img -o 0x10 -q
w ext4.img -o 0x438 -b
w multi.img -a -t swap
w multi.img -a -t noswap
w multi.img -a -t nosuch
w multi.img -a -t swap -n
w pt-gpt.img -a -t gpt
w pt-gpt.img -a -t gpt -f
w pt-gpt.img -a -t PMBR -f
w2 ext4.img vfat.img -a
w2 ext4.img vfat.img -o 0x438
w ext4.img -a --lock
w ext4.img -a --lock=yes
w ext4.img -a --lock=nonblock
w ext4.img -a --lock=no
LOCKENV=yes w ext4.img -a
LOCKENV=bogus w ext4.img -a

echo "wipefs-diff: $pass agree, $fail differ"
[ "$fail" = 0 ]
