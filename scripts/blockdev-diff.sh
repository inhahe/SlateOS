#!/usr/bin/env bash
# Differential test: our `blockdev` against util-linux 2.39.3's.
#
# blockdev is `ioctl`s and little else, and an ordinary user has no block
# device to aim them at. So both sides run under scripts/blockdev-shim.c, an
# LD_PRELOAD interposer that answers the block-device requests for fixture
# files as a block device would -- including values at the edges of each
# request's type, requests that fail, and the fallbacks a failed size
# request takes -- and logs every request each side makes. The logs are
# compared along with the output, so the requests, their arguments (a
# `--setra` value arrives by value, a `--setbsz` one by pointer) and their
# order are measured, not only what is printed.
#
# `--report` also reads /proc/partitions and, for a partition's start,
# sysfs. Those are faked inside an unprivileged user and mount namespace
# (`unshare -rm`): a /proc/partitions of the harness's own, fixture files
# and character devices bind-mounted over /dev nodes, and a /sys/dev/block
# and /sys/block whose entries say which of those devices is a partition,
# where it starts, or that its start cannot be read. A character device is
# used where a partition is wanted because, unlike a file, it has a device
# number for sysfs to be asked about.
set -u

DIFF_PROG='blockdev'
DIFF_PKG='blockdev'
DIFF_NEED='timeout gcc unshare'
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

here=$(cd "$(dirname "$0")" && pwd)
pass=0; fail=0
fx=$DIFF_TMP/fx
mkdir -p "$fx"
cd "$fx" >/dev/null || exit 1

shim=$DIFF_TMP/blockdev-shim.so
if ! gcc -shared -fPIC -O1 -Wall -Werror -o "$shim" "$here/blockdev-shim.c" -ldl; then
  echo "blockdev-diff: the ioctl shim does not build" >&2
  exit 1
fi

# The namespace must be available, or the --report half measures nothing.
if ! unshare -rm true 2>/dev/null; then
  echo "blockdev-diff: no unprivileged user namespaces here; skipping"
  exit 0
fi

# --- fixtures ----------------------------------------------------------------------------
fixture() { # name, then "REQUEST VALUE" lines
  local name=$1; shift
  { echo '#blockdev-shim'; printf '%s\n' "$@"; } > "$name"
}
fixture blk-disk 'BLKROGET 0' 'BLKRAGET 256' 'BLKFRAGET 128' 'BLKSSZGET 512' 'BLKPBSZGET 4096' \
  'BLKIOMIN 4096' 'BLKIOOPT 0' 'BLKALIGNOFF 0' 'BLKSECTGET 1280' 'BLKBSZGET 4096' \
  'BLKGETSIZE 2097152' 'BLKGETSIZE64 1073741824' 'BLKGETDISKSEQ 7' 'BLKDISCARDZEROES 0'
# Every value at an edge of the type the kernel writes it as.
fixture blk-odd 'BLKROGET -5' 'BLKRAGET -1' 'BLKFRAGET 9223372036854775807' 'BLKSSZGET -1' \
  'BLKPBSZGET 4294967295' 'BLKIOMIN 0' 'BLKIOOPT 2147483648' 'BLKALIGNOFF -512' \
  'BLKSECTGET 65535' 'BLKBSZGET 2147483647' 'BLKGETSIZE 18446744073709551615' \
  'BLKGETSIZE64 18446744073709551615' 'BLKGETDISKSEQ 18446744073709551615' 'BLKDISCARDZEROES 1'
fixture blk-ro 'BLKROGET 1' 'BLKRAGET 0' 'BLKSSZGET 4096' 'BLKBSZGET 4096' 'BLKGETSIZE64 4096'
fixture blk-noread 'BLKROGET 0' 'BLKRAGET !13' 'BLKFRAGET !13' 'BLKSSZGET 512' 'BLKBSZGET 512' \
  'BLKGETSIZE64 512'
# The size falls back: BLKGETSIZE64, then BLKGETSIZE, then FDGETPRM, then
# the file's own length.
fixture blk-size32 'BLKROGET 0' 'BLKRAGET 8' 'BLKSSZGET 512' 'BLKBSZGET 1024' \
  'BLKGETSIZE64 !25' 'BLKGETSIZE 4194303'
fixture blk-nosize 'BLKROGET 0' 'BLKRAGET 8' 'BLKSSZGET 512' 'BLKBSZGET 1024' \
  'BLKGETSIZE64 !25' 'BLKGETSIZE !25'
fixture blk-setfail 'BLKROSET !1' 'BLKBSZSET !22' 'BLKRASET !13' 'BLKFRASET !95' \
  'BLKFLSBUF !1' 'BLKRRPART !16' 'BLKROGET 0'
head -c 3000 /dev/zero > plain
mkdir -p adir
mkfifo afifo

# sysfs as --report reads it: which device numbers are partitions (of a
# disk /sys/block knows), and where each starts. The character devices
# stand in for block devices: 1:5 (/dev/zero) starts at 2048, 1:7
# (/dev/full) has no start to read, 1:8 (/dev/random) starts past what
# fifteen columns hold, 1:9 (/dev/urandom) has a negative start, which
# `%lu` wraps; 1:3 (/dev/null) is a whole disk.
sy=$DIFF_TMP/sys
mkdir -p "$sy/devblock" "$sy/block" "$sy/devices/fakedisk" "$sy/devices/null"
echo '8:0' > "$sy/devices/fakedisk/dev"
echo '1:3' > "$sy/devices/null/dev"
part() { # p, devno, start (or "-" for none)
  mkdir -p "$sy/devices/fakedisk/$1"
  echo 1 > "$sy/devices/fakedisk/$1/partition"
  echo "$2" > "$sy/devices/fakedisk/$1/dev"
  [ "$3" = - ] || echo "$3" > "$sy/devices/fakedisk/$1/start"
  ln -s "$sy/devices/fakedisk/$1" "$sy/devblock/$2"
}
part p1 1:5 2048
part p2 1:7 -
part p3 1:8 1234567890123456789
part p4 1:9 -5
ln -s "$sy/devices/null" "$sy/devblock/1:3"
ln -s "$sy/devices/fakedisk" "$sy/block/fakedisk"
devmap="1:5=$fx/blk-disk,1:7=$fx/blk-odd,1:8=$fx/blk-ro,1:9=$fx/blk-size32,1:3=$fx/blk-disk"

# /proc/partitions: names under /dev, reached through bind mounts made in
# the namespace. Lines that are not four fields are skipped.
cat > parts.mixed <<'EOF'
major minor  #blocks  name

   7        0    1048576 loop0
   7        1    1048576 loop1
   7        2    1048576 loop2
   7        3    1048576 loop3
   7        4    1048576 loop4
   7        5    1048576 loop5
   1        0       4096 ram0
   8        0     397940 nosuchdevice
garbage line
   8   x   1 loop6
  +8  -1  +1 loop7
EOF
printf 'major minor  #blocks  name\n\n' > parts.header
: > parts.empty
printf '   1   1   1 ram1\tsuffix\n   1   2   1 ram2 trailing\n' > parts.odd
# A line longer than fgets' 199 bytes, split in two.
{ printf '   7   0   1 %s\n' "$(printf 'x%.0s' $(seq 1 190))"; echo '   7   1   1 loop1'; } > parts.long

nsrun=$DIFF_TMP/nsrun.sh
cat > "$nsrun" <<'EOF'
# nsrun DEST=SRC... -- CMD...: each SRC bind-mounted over DEST, then CMD.
set -e
while [ "$1" != "--" ]; do
  mount --bind "${1#*=}" "${1%%=*}"
  shift
done
shift
exec "$@"
EOF

# The mounts every namespaced case gets.
mounts_std=(
  "/sys/dev/block=$sy/devblock" "/sys/block=$sy/block"
  "/dev/loop0=$fx/blk-disk" "/dev/loop1=$fx/blk-odd" "/dev/loop2=$fx/blk-noread"
  "/dev/loop3=/dev/zero" "/dev/loop4=/dev/full" "/dev/loop5=/dev/random"
  "/dev/loop6=/dev/urandom" "/dev/loop7=/dev/null" "/dev/ram1=$fx/blk-ro"
  "/dev/ram2=$fx/blk-size32"
)

# --- one case ----------------------------------------------------------------------------
# PARTS: a /proc/partitions for the namespace (unset: no namespace at all).
run_side() {
  local side=$1; shift
  local log=$DIFF_TMP/$side.log
  : > "$log"
  if [ -n "${PARTS:-}" ]; then
    diff_run timeout -k 2 20 unshare -rm sh "$nsrun" "/proc/partitions=$PARTS" "${mounts_std[@]}" -- \
      env LC_ALL=C.UTF-8 LD_PRELOAD="$shim" SHIM_LOG="$log" SHIM_DEVMAP="$devmap" \
      PATH="$bindir/$side" blockdev "$@"
  else
    diff_run timeout -k 2 20 env LC_ALL=C.UTF-8 LD_PRELOAD="$shim" SHIM_LOG="$log" \
      SHIM_DEVMAP="$devmap" PATH="$bindir/$side" blockdev "$@"
  fi
}

compare() {
  local o_out=$DIFF_TMP/o.out g_out=$DIFF_TMP/g.out o_err=$DIFF_TMP/o.err g_err=$DIFF_TMP/g.err
  local o_rc g_rc
  run_side ours "$@" </dev/null >"$o_out" 2>"$o_err"; o_rc=$?
  run_side gnu "$@" </dev/null >"$g_out" 2>"$g_err"; g_rc=$?
  if cmp -s "$o_out" "$g_out" && cmp -s "$o_err" "$g_err" && [ "$o_rc" = "$g_rc" ] \
     && cmp -s "$DIFF_TMP/ours.log" "$DIFF_TMP/gnu.log"; then
    pass=$((pass + 1))
    [ -n "${VERBOSE:-}" ] && printf 'OK   blockdev %s\n' "$*"
  else
    fail=$((fail + 1))
    printf 'DIFF blockdev %s  [PARTS=%s]\n  ours rc=%s, gnu rc=%s\n' "$*" "${PARTS:-}" "$o_rc" "$g_rc"
    diff -u "$g_out" "$o_out" | sed -n '3,12p' | sed 's/^/  out /'
    diff -u "$g_err" "$o_err" | sed -n '3,8p' | sed 's/^/  err /'
    diff -u "$DIFF_TMP/gnu.log" "$DIFF_TMP/ours.log" | sed -n '3,12p' | sed 's/^/  log /'
  fi
  return 0
}

# Standard output that cannot be written.
compare_full() {
  local o_rc g_rc
  run_side ours "$@" </dev/null >/dev/full 2>"$DIFF_TMP/o.err"; o_rc=$?
  run_side gnu "$@" </dev/null >/dev/full 2>"$DIFF_TMP/g.err"; g_rc=$?
  if cmp -s "$DIFF_TMP/o.err" "$DIFF_TMP/g.err" && [ "$o_rc" = "$g_rc" ]; then
    pass=$((pass + 1))
  else
    fail=$((fail + 1))
    printf 'DIFF blockdev %s >/dev/full\n  ours rc=%s, gnu rc=%s\n' "$*" "$o_rc" "$g_rc"
    diff -u "$DIFF_TMP/g.err" "$DIFF_TMP/o.err" | sed -n '3,8p' | sed 's/^/  err /'
  fi
}

# --- the command line --------------------------------------------------------------------
compare
for a in -h --help -V --version; do compare $a; compare $a blk-disk; done
compare -v -V blk-disk
compare --bogus blk-disk
compare -x blk-disk
compare - blk-disk
compare --
compare -- blk-disk
compare --getro -- blk-disk
compare --getro
compare --getsz
compare --setbsz
compare --setbsz blk-disk
compare --setbsz 512
compare --setbsz x blk-disk
compare --setbsz '' blk-disk
compare --setbsz 99999999999 blk-disk
compare --setbsz -2147483649 blk-disk
compare --setra 0x10 blk-disk
compare --setra ' 12' blk-disk
compare --setra 12x blk-disk
compare -v --report blk-disk
compare --getro --report blk-disk
compare blk-disk
compare ''
compare --getro ''
compare --getro nonexistent
compare --getro nonexistent blk-disk
compare --getro blk-disk nonexistent
compare --getro adir
compare --getsz adir
compare --getro plain
compare -v --getro plain
compare --getsz plain
compare --getsz /dev/null
compare --getsz /dev/zero
compare --getro /dev/null
compare -v --flushbufs plain

# --- every command, on each fixture ----------------------------------------------------------
cmds='--setro --setrw --getro --getdiscardzeroes --getss --getpbsz --getiomin --getioopt
  --getalignoff --getmaxsect --getbsz --getsize --getsize64 --getra --getfra --getdiskseq
  --flushbufs --rereadpt --getsz'
for f in blk-disk blk-odd blk-ro blk-noread blk-size32 blk-nosize blk-setfail; do
  for c in $cmds; do
    compare "$c" "$f"
    compare -v "$c" "$f"
  done
  for v in 0 512 4096 -1 2147483647 -2147483648; do
    compare --setbsz "$v" "$f"
    compare -v --setra "$v" "$f"
    compare -v --setfra "$v" "$f"
  done
done
# Several commands, several devices, verbosity switched on and off -- and
# starting off again for each device.
compare --getro --getss -v --getbsz --getsize64 -q --getra blk-disk blk-odd blk-ro
compare -v --getsz --setra 64 -q --getfra --setbsz 1024 -v --flushbufs blk-disk blk-ro
compare -v --getro --getra blk-disk blk-noread blk-ro
compare --getsz --getsz blk-disk blk-size32 blk-nosize plain
compare --getro blk-disk /dev/zero

# --- --report ----------------------------------------------------------------------------
compare --report
compare --report blk-disk
compare --report blk-disk blk-odd blk-ro blk-noread blk-size32 blk-nosize blk-setfail
compare --report plain adir afifo /dev/null nonexistent ''
compare --report -v
compare --report --getro blk-disk
for p in "$fx/parts.mixed" "$fx/parts.header" "$fx/parts.empty" "$fx/parts.odd" "$fx/parts.long"; do
  PARTS=$p compare --report
done
PARTS=$fx/parts.mixed compare --report /dev/loop0 /dev/loop3 /dev/loop4 /dev/loop5 /dev/loop6 /dev/loop7 /dev/ram1
PARTS=$fx/parts.mixed compare --getro --getsz /dev/loop3 /dev/loop4
PARTS=$fx/parts.mixed compare --report /dev/loop2 /dev/ram2

# --- standard output that fails ---------------------------------------------------------------
compare_full --getro blk-disk
compare_full --help
compare_full --version
compare_full --report blk-disk
compare_full -v --setro blk-setfail

echo "blockdev-diff: $pass agree, $fail differ"
[ "$fail" = 0 ]
