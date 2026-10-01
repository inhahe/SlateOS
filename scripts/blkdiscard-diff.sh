#!/usr/bin/env bash
# Differential test: our `blkdiscard` against util-linux 2.39.3's.
#
# Discarding needs a block device opened read-write, which an ordinary user
# does not get; what this compares is everything before that -- options and
# their refusals, size arguments, operand counts, and each way the device
# is refused (missing, a directory, a regular file, a character device, a
# block device the user may not open). It also builds the port for Linux,
# the only place its block-device half compiles.
set -u

DIFF_PROG='blkdiscard'
DIFF_PKG='blkdiscard'
DIFF_NEED='timeout'
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

if [ "$(id -u)" = "0" ]; then
  echo "blkdiscard-diff: refusing to run as root: a block device operand would be discarded." >&2
  exit 2
fi

pass=0; fail=0
fx=$DIFF_TMP/fx
mkdir -p "$fx"
cd "$fx" >/dev/null || exit 1
head -c 65536 /dev/zero > file.img
mkdir -p adir
blockdev=$(for d in /dev/sda /dev/vda /dev/sdb /dev/loop0; do [ -b "$d" ] && { echo "$d"; break; }; done)

compare() {
  local o_out=$DIFF_TMP/o.out g_out=$DIFF_TMP/g.out o_err=$DIFF_TMP/o.err g_err=$DIFF_TMP/g.err
  local o_rc g_rc
  diff_run timeout -k 2 15 env LC_ALL=C.UTF-8 PATH="$bindir/ours" blkdiscard "$@" </dev/null >"$o_out" 2>"$o_err"; o_rc=$?
  diff_run timeout -k 2 15 env LC_ALL=C.UTF-8 PATH="$bindir/gnu" blkdiscard "$@" </dev/null >"$g_out" 2>"$g_err"; g_rc=$?
  if cmp -s "$o_out" "$g_out" && cmp -s "$o_err" "$g_err" && [ "$o_rc" = "$g_rc" ]; then
    pass=$((pass + 1))
    [ -n "${VERBOSE:-}" ] && printf 'OK   blkdiscard %s\n' "$*"
  else
    fail=$((fail + 1))
    printf 'DIFF blkdiscard %s\n  ours rc=%s, gnu rc=%s\n' "$*" "$o_rc" "$g_rc"
    diff -u "$g_out" "$o_out" | sed -n '3,10p' | sed 's/^/  out /'
    diff -u "$g_err" "$o_err" | sed -n '3,8p' | sed 's/^/  err /'
  fi
  return 0
}

compare -h
compare --help
compare -V
compare --version
compare
compare a b
compare --bogus
compare -x
compare --zero
compare -o
compare -o x file.img
compare -l 1Q file.img
compare -p -5 file.img
compare -o 99999999999999999999999 file.img
compare nonexistent
compare adir
compare file.img
compare -f file.img
compare -z -s -v file.img
compare /dev/null
compare -f /dev/null
[ -n "$blockdev" ] && compare "$blockdev"
[ -n "$blockdev" ] && compare -f "$blockdev"

echo "blkdiscard-diff: $pass agree, $fail differ"
[ "$fail" = 0 ]
