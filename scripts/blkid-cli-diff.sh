#!/usr/bin/env bash
# Differential test: our `blkid` and `findfs` against util-linux 2.39.3's.
#
# `blkid-diff.sh` compares the library -- every value of every prober -- so
# this one compares the programs around it: options and their refusals,
# every output format, the low-level mode (`-p`, `-i`) with its filters,
# hints, offsets and sizes, the cache mode with the cache file it writes,
# tag lookup and evaluation, garbage collection, exit statuses, and
# `findfs`.
#
# Images come from util-linux's own test corpus (tests/ts/blkid), which
# util-linux-source.sh fetches, plus an ambivalent one made here (an ext3
# superblock with a swap signature over it). Nothing needs root: the
# devices probed are image files, and as an ordinary user the machine's
# real disks are refused with EACCES on both sides alike.
#
# Each side gets its own cache file (`BLKID_FILE`), removed before every
# case, and a configuration file that does not exist (`BLKID_CONF`), so
# neither sees what the other wrote nor this machine's. The cache file a
# case leaves is compared too, its TIME= stamps masked.
set -u

DIFF_PROG='blkid'
DIFF_PKG='blkid findfs'
DIFF_BINS='blkid findfs'
DIFF_NEED='timeout xz'
# shellcheck source=util-linux-source.sh
. "$(dirname "$0")/util-linux-source.sh"
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

if [ "$(id -u)" = "0" ]; then
  echo "blkid-cli-diff: refusing to run as root: the no-operand cases would probe" >&2
  echo "  and cache this machine's own disks." >&2
  exit 2
fi

tsdir=$UL_SRC/tests/ts/blkid
if ! [ -d "$tsdir/images-fs" ]; then
  echo "blkid-cli-diff.sh: util-linux 2.39.3's source is not available; skipped"
  exit 0
fi

pass=0; fail=0

fx=$DIFF_TMP/fx
mkdir -p "$fx"
cd "$fx" >/dev/null || exit 1
for n in ext2 ext3 ext4 vfat swap0 swap1 xfs btrfs iso luks2 lvm2 ntfs exfat \
         jbd hfsplus udf zfs squashfs4 f2fs; do
  f=$tsdir/images-fs/$n.img.xz
  [ -f "$f" ] && xz -dc "$f" > "$n.img"
done
for n in dos+bsd gpt sun sgi bsd atari-primary; do
  f=$tsdir/images-pt/$n.img.xz
  [ -f "$f" ] && xz -dc "$f" > "pt-$n.img"
done
# Ambivalent: ext3's superblock, and a swap signature over it (version 1
# at 1 KiB, where ext3 keeps its inode count; SWAPSPACE2 at 4086).
if [ -f ext3.img ]; then
  cp ext3.img amb.img
  printf '\001\000\000\000' | dd of=amb.img bs=1 seek=1024 conv=notrunc status=none
  printf 'SWAPSPACE2' | dd of=amb.img bs=1 seek=4086 conv=notrunc status=none
fi
images=$(ls ./*.img 2>/dev/null | sed 's|^\./||' | sort)
# A cache file with a device that does not exist and one that does, for -g.
cat > seeded.tab <<EOF
<device DEVNO="0x0801" TIME="1700000000.1" TYPE="ext4">/nonexistent-blkid-cli/sda1</device>
<device DEVNO="0x0000" TIME="1700000000.2" LABEL="x" TYPE="ext2">$fx/ext2.img</device>
EOF
printf 'EVALUATE=scan\n' > scan.conf
uuid_link=$(ls /dev/disk/by-uuid 2>/dev/null | head -1)

run_side() {
  local side=$1 prog=$2; shift 2
  rm -f "$DIFF_TMP/cache-$side"
  [ -n "${SEED:-}" ] && cp "$SEED" "$DIFF_TMP/cache-$side"
  diff_run timeout -k 2 30 env LC_ALL=C.UTF-8 BLKID_FILE="$DIFF_TMP/cache-$side" \
    BLKID_CONF="${CONF:-/nonexistent-blkid-conf}" ${COLS:+COLUMNS=$COLS} \
    PATH="$bindir/$side" "$prog" "$@"
}

compare() {
  local prog=$1; shift
  local o_out g_out o_err g_err o_rc g_rc
  o_out=$DIFF_TMP/o.out; g_out=$DIFF_TMP/g.out
  o_err=$DIFF_TMP/o.err; g_err=$DIFF_TMP/g.err
  run_side ours "$prog" "$@" </dev/null >"$o_out" 2>"$o_err"; o_rc=$?
  { [ -f "$DIFF_TMP/cache-ours" ] && sed 's/TIME="[^"]*"/TIME="T"/' "$DIFF_TMP/cache-ours"; } >>"$o_out"
  run_side gnu "$prog" "$@" </dev/null >"$g_out" 2>"$g_err"; g_rc=$?
  { [ -f "$DIFF_TMP/cache-gnu" ] && sed 's/TIME="[^"]*"/TIME="T"/' "$DIFF_TMP/cache-gnu"; } >>"$g_out"
  if cmp -s "$o_out" "$g_out" && cmp -s "$o_err" "$g_err" && [ "$o_rc" = "$g_rc" ]; then
    pass=$((pass + 1))
    [ -n "${VERBOSE:-}" ] && printf 'OK   %s %s\n' "$prog" "$*"
  else
    fail=$((fail + 1))
    printf 'DIFF %s %s\n  ours rc=%s, gnu rc=%s\n' "$prog" "$*" "$o_rc" "$g_rc"
    diff -u "$g_out" "$o_out" | sed -n '3,14p' | sed 's/^/  out /'
    diff -u "$g_err" "$o_err" | sed -n '3,8p' | sed 's/^/  err /'
  fi
  return 0
}

b() { compare blkid "$@"; }
f() { compare findfs "$@"; }

# --- options and refusals -----------------------------------------------------------
b -h
b --help
b -V
b -v
b --version
b -k
b --list-filesystems
b -x
b --bogus
b --outp=value -k
b -o
b -o bogus ext2.img
b -O x -p ext2.img
b -O -1 -p ext2.img
b -S 99999999999999999999999 -p ext2.img
b -S 1Q -p ext2.img
b -t foo
b -t A=1 -t B=2
b -L x -t A=1
b -t 'A="open'
b -n vfat -u raid
b -u raid -n vfat
b -u bogus -p ext2.img
b -u no -p ext2.img
b -u filesystem,bogus,raid -p ext2.img
b -n no -p ext2.img
b -n '' -p ext2.img
b -p
b -i
b -p -o list ext2.img
b -l
b -w ignored -V
many=''
for _ in $(seq 1 127); do many="$many -s T"; done
# shellcheck disable=SC2086  # word-splitting is the point
b $many -p ext2.img
# shellcheck disable=SC2086
b $many -s T -p ext2.img

# --- low-level probing ----------------------------------------------------------------
for img in $images; do
  b -p "$img"
done
for fmt in value export udev device full list; do
  b -p -o "$fmt" ext4.img pt-gpt.img
done
b -p -o udev amb.img
b -p amb.img
b -p -o value amb.img ext2.img
b -p -s TYPE ext4.img
b -p -s TYPE -s LABEL -o value ext4.img vfat.img
b -p -s NOSUCH ext4.img
b -p -u filesystem ext4.img lvm2.img
b -p -u nofilesystem ext4.img lvm2.img
b -p -u raid,crypto luks2.img
b -p -n ext4 ext4.img ext3.img
b -p -n noext4 ext4.img ext3.img
b -p -n vfat,ext3, ext3.img
b -p -D pt-gpt.img
b -p -D pt-dos+bsd.img
b -p -d vfat.img
b -p -O 1024 -S 65536 ext2.img
b -p -O 1M ext2.img
b -p -S 512 ext2.img
b -p -O 999999999999 ext2.img
b -i ext4.img
b -i -o full ext4.img
b -i -o value ext4.img
b -p -i ext4.img
b -p -H session_offset=0 iso.img
b -p -H session_offset=abc iso.img
b -p -H session_offset= iso.img
b -p -H 'x="unclosed' iso.img
b -p -H 99 iso.img
b -p ext4.img nonexistent.img
b -p nonexistent.img
b -p /dev/null
b -p .
b -p ext2.img ext3.img ext4.img

# --- the cache ------------------------------------------------------------------------
b ext2.img
b ext4.img vfat.img pt-gpt.img
for fmt in value export udev device full; do
  b -o "$fmt" ext2.img ext4.img
done
b -o list ext2.img ext4.img
COLS=200 b -o list ext2.img ext4.img swap1.img
COLS=40 b -o list ext2.img
b -s TYPE ext2.img ext4.img
b -s TYPE -s UUID -o value ext2.img ext4.img
b -t TYPE=ext4 ext2.img ext4.img
b -t TYPE=nosuch ext2.img ext4.img
b -l -t TYPE=ext4 ext2.img ext4.img
b -l -t TYPE=nosuch ext2.img
b -l -o device -t LABEL=nosuch
b -c /dev/null ext2.img
b -d ext2.img
b nonexistent.img
b /dev/null
SEED=$fx/seeded.tab b -g
SEED=$fx/seeded.tab b
SEED=$fx/seeded.tab b -t LABEL=x
SEED=$fx/seeded.tab b -o list
b

# --- evaluation -----------------------------------------------------------------------
b -L nosuchlabel
b -U nosuchuuid
CONF=$fx/scan.conf b -L nosuchlabel
[ -n "$uuid_link" ] && b -U "$uuid_link"
[ -n "$uuid_link" ] && b -l -o device -t UUID="$uuid_link"

# --- findfs ---------------------------------------------------------------------------
f
f a b
f -h
f --help
f -V
f --version
f -x
f --bogus
f LABEL=nosuch
f UUID=nosuch
f /dev/null
f --
f 'LABEL="open'
f LABEL=
[ -n "$uuid_link" ] && f UUID="$uuid_link"

echo "blkid-cli-diff: $pass agree, $fail differ"
[ "$fail" = 0 ]
