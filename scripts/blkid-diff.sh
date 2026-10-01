#!/usr/bin/env bash
# Differential test: `userspace/ulblkid`, the port of util-linux 2.39.3's
# libblkid probing, against the system's libblkid (Ubuntu's 2.39.3).
#
# Both sides answer the same questions about the same image files, and print
# the answers the same way: `scripts/blkid-probe.c` asks libblkid, the
# `blkid-probe` example asks the port. Per image, on a fresh probe each:
# `blkid_do_safeprobe` with every value asked for (and again accepting bad
# checksums), `blkid_do_fullprobe`, the `wipefs` walk (`blkid_do_probe` and a
# dry-run `blkid_do_wipe` until nothing is left), and the binary partition
# list with and without `BLKID_PARTS_FORCE_GPT`. Every value is compared byte
# for byte, its length included.
#
# The images:
#
#   * util-linux's own test corpus, `tests/ts/blkid/images-fs` (121
#     filesystems, RAID members and the rest) and `images-pt` (7 partition
#     tables), from the source tarball util-linux-source.sh fetches;
#   * images made here with whatever `mkfs.*`, `mkswap` and `sfdisk` WSL
#     has, where the corpus has no example (a DOS table with logical
#     partitions, a GPT with many entries) -- skipped, not failed, for a
#     tool that is missing;
#   * every corpus image again, truncated at a few sizes, so that the "short
#     read is nothing, not an error" paths are compared as well.
#
# Usage:
#   ./scripts/blkid-diff.sh            # everything
#   ./scripts/blkid-diff.sh --keep     # leave the images and outputs
#   VERBOSE=1 ./scripts/blkid-diff.sh  # the diff of every failing image
set -u

DIFF_PROG='blkid-probe'
DIFF_PKG='ulblkid'
# The subject is a test instrument, not a utility (see diff-wsl.sh): there is
# no reference of its name, and no pair of binaries to put behind one name.
DIFF_EXAMPLES=blkid-probe
DIFF_NO_REF=1
DIFF_NO_BINDIR=1
DIFF_NEED="gcc xz truncate"
# shellcheck source=util-linux-source.sh
. "$(dirname "$0")/util-linux-source.sh"
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

KEEP=0
while [ $# -gt 0 ]; do
  case "$1" in
    --keep) KEEP=1; shift ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done
if [ "$KEEP" = 1 ]; then
  diff_cleanup() { echo "working files in $DIFF_TMP"; }
fi

if ! [ -f /usr/include/blkid/blkid.h ]; then
  echo "blkid-diff.sh: libblkid-dev is not installed; skipped"
  exit 0
fi
ref=$DIFF_TMP/ref
if ! gcc -O2 -Wall -o "$ref" "$root/scripts/blkid-probe.c" -lblkid; then
  echo "could not build the reference probe" >&2
  exit 1
fi

tsdir=$UL_SRC/tests/ts/blkid
if ! [ -d "$tsdir/images-fs" ]; then
  echo "blkid-diff.sh: util-linux 2.39.3's source is not available; skipped"
  exit 0
fi

img=$DIFF_TMP/img
mkdir -p "$img"
for f in "$tsdir"/images-fs/*.img.xz "$tsdir"/images-pt/*.img.xz; do
  n=$(basename "$f" .img.xz)
  case "$f" in
    */images-pt/*) n="pt-$n" ;;
  esac
  xz -dc "$f" > "$img/$n.img" || { echo "cannot unpack $f" >&2; exit 1; }
done

# --- images made here -----------------------------------------------------------
made=0
mk() { # mk NAME SIZE_MIB COMMAND... : a zeroed image, then COMMAND on it
  local name=$1 size=$2
  shift 2
  truncate -s "${size}M" "$img/mk-$name.img" || return
  if "$@" "$img/mk-$name.img" > /dev/null 2>&1; then
    made=$((made + 1))
  else
    rm -f "$img/mk-$name.img"
  fi
}
if command -v sfdisk > /dev/null; then
  mk dos-logicals 64 sh -c 'printf "label: dos\nlabel-id: 0x1234abcd\n,8M,L,*\n,8M,S\n,,E\n,4M,L\n,4M,83\n,,b\n" | sfdisk -q "$0"'
  mk gpt-many 64 sh -c 'printf "label: gpt\nlabel-id: 01234567-89ab-cdef-0123-456789abcdef\n,4M,L,name=root\n,4M,S,name=swap\n,4M,U\n,4M,L,attrs=RequiredPartition\n,,L,name=\"with space\"\n" | sfdisk -q "$0"'
  mk gpt-4k-entries 64 sh -c 'printf "label: gpt\ntable-length: 4\n,,L\n" | sfdisk -q "$0"'
  mk sun 64 sh -c 'printf "label: sun\n,8M\n,8M\n" | sfdisk -q "$0"'
  mk sgi 64 sh -c 'printf "label: sgi\n,8M\n" | sfdisk -q "$0"'
fi
command -v mkswap > /dev/null && mk swap 8 mkswap -L swaplbl -U 11111111-2222-3333-4444-555555555555
command -v mkfs.vfat > /dev/null && mk vfat16 32 mkfs.vfat -F 16 -n MYVFAT -i 12345678
command -v mkfs.vfat > /dev/null && mk vfat32 64 mkfs.vfat -F 32 -n ""
command -v mkfs.ext4 > /dev/null && mk ext4 32 mkfs.ext4 -q -L extlabel -U 99999999-8888-7777-6666-555555555555
command -v mkfs.ext2 > /dev/null && mk ext2 8 mkfs.ext2 -q
command -v mkfs.xfs > /dev/null && mk xfs 320 mkfs.xfs -q -L xfslabel
command -v mkfs.btrfs > /dev/null && mk btrfs 128 mkfs.btrfs -q -L btr
command -v mkfs.ntfs > /dev/null && mk ntfs 16 mkfs.ntfs -q -F -L ntlabel
command -v mkfs.exfat > /dev/null && mk exfat 16 mkfs.exfat -n EXL
command -v mkfs.f2fs > /dev/null && mk f2fs 64 mkfs.f2fs -q -l f2l
command -v mkfs.minix > /dev/null && mk minix 8 mkfs.minix -3
command -v mkfs.cramfs > /dev/null && mk cramfs 1 sh -c 'mkdir -p "$0.d" && mkfs.cramfs "$0.d" "$0"'
echo "$made image(s) made here"

# --- truncated copies -------------------------------------------------------------
# A prober asking for more than the device holds gets "nothing", which is not
# the same answer as a failed read; cut images short to compare that. Half
# of each image too, but only of the small ones: the mkfs-made XFS and btrfs
# images are hundreds of MiB, and halving them tests nothing the corpus's
# own copies do not.
for f in "$img"/*.img; do
  size=$(stat -c %s "$f")
  cuts="1024 4096 65536"
  [ "$size" -le $((16 << 20)) ] && cuts="$cuts $((size / 2))"
  for cut in $cuts; do
    [ "$cut" -lt "$size" ] || continue
    head -c "$cut" "$f" > "$f.cut$cut"
  done
done

pass=0; fail=0
failed=''
out=$DIFF_TMP/out
mkdir -p "$out"
for f in "$img"/*.img "$img"/*.img.cut*; do
  [ -e "$f" ] || continue
  n=$(basename "$f")
  "$ref" "$f" > "$out/$n.ref" 2>&1
  "$OURS" "$f" > "$out/$n.ours" 2>&1
  if cmp -s "$out/$n.ref" "$out/$n.ours"; then
    pass=$((pass + 1))
  else
    fail=$((fail + 1))
    failed="$failed $n"
    if [ "${VERBOSE:-0}" = 1 ]; then
      echo "--- $n"
      diff -u "$out/$n.ref" "$out/$n.ours" | head -40
    fi
  fi
done

if [ -n "$failed" ]; then
  echo "differ:$failed"
fi
echo "blkid-diff: $pass image(s) agree, $fail differ"
[ "$fail" = 0 ]
