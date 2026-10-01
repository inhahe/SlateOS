#!/usr/bin/env bash
# Differential test: `userspace/smartcols`, the port of libsmartcols, against
# util-linux 2.39.3's library itself.
#
# The programs built on the port each have a harness of their own, and those
# compare what the programs print; this one compares the library directly,
# on what no program shipped here yet exercises in bulk -- groups of lines
# (the chart `lsblk --merge` draws) and sorting (`lsblk --sort`, and sorting
# by tree) -- and on the tree and table printing around them.
#
# Both sides run the same scripts: `scripts/smartcols-cases.py` writes them
# (see it for the four families), `scripts/scols-probe.c` runs each through
# the installed libsmartcols.so.1, and `examples/scols-probe.rs` through the
# port. Each script's stdout, stderr (the answer of every call that can
# refuse) and exit status must agree -- in a UTF-8 locale and in C, which
# draw with different symbols. A table upstream aborts on (a group's lines
# met out of order) must abort here too.
#
# Where upstream never finishes, there is nothing to agree with, and the
# port finishes instead -- the cases its crate documentation lists: a
# terminal too narrow for the table's minimum widths, which upstream's
# reduction loops on (it is killed by the timeout, 124), and a line made its
# own group's child, which upstream's sort recurses through until the stack
# runs out (SIGSEGV, 139). Those are counted, and shown with VERBOSE=1, but
# they are not differences -- provided the port did finish, with status 0.
#
# The reference needs libsmartcols's header, which only the -dev package
# installs; it is taken from util-linux 2.39.3's source instead (the
# version the installed library is), so nothing has to be installed.
#
#   ./scripts/smartcols-diff.sh                 # 1500 scripts, seed 1
#   ./scripts/smartcols-diff.sh --cases 5000 --seed 7
#   ./scripts/smartcols-diff.sh --keep          # leave the scripts and outputs
set -u

DIFF_PROG='smartcols'
DIFF_PKG='smartcols'
# The subject is an example: it exposes a library to a C reference.
DIFF_EXAMPLES=scols-probe
DIFF_NO_REF=1
DIFF_NO_BINDIR=1
DIFF_NEED='gcc python3 timeout'
# shellcheck source=util-linux-source.sh
. "$(dirname "$0")/util-linux-source.sh"
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

CASES=1500
SEED=1
KEEP=0
while [ $# -gt 0 ]; do
  case "$1" in
    --cases) CASES="$2"; shift 2 ;;
    --seed) SEED="$2"; shift 2 ;;
    --keep) KEEP=1; shift ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done
if [ "$KEEP" = 1 ]; then
  diff_cleanup() { echo "working files in $DIFF_TMP"; }
fi

hdr=$UL_SRC/libsmartcols/src/libsmartcols.h.in
lib=$(ls /usr/lib/x86_64-linux-gnu/libsmartcols.so.1 2>/dev/null)
if ! [ -f "$hdr" ] || [ -z "$lib" ]; then
  echo "smartcols-diff.sh: libsmartcols.so.1 or util-linux 2.39.3's source is not available; skipped"
  exit 0
fi
mkdir -p "$DIFF_TMP/inc" "$DIFF_TMP/cases" "$DIFF_TMP/out"
sed 's/@LIBSMARTCOLS_VERSION@/2.39.3/' "$hdr" > "$DIFF_TMP/inc/libsmartcols.h"
if ! gcc -O2 -Wall -I"$DIFF_TMP/inc" -o "$DIFF_TMP/probe" "$root/scripts/scols-probe.c" "$lib"; then
  echo "smartcols-diff.sh: could not build the reference probe" >&2
  exit 1
fi
python3 "$root/scripts/smartcols-cases.py" "$DIFF_TMP/cases" "$CASES" "$SEED" || exit 1

# An abort must not leave a core file behind, on either side.
ulimit -c 0

pass=0; fail=0; aborts=0; charts=0; unfinished=0
o=$DIFF_TMP/out
for f in "$DIFF_TMP"/cases/*.scols; do
  for loc in C.UTF-8 C; do
    # In a subshell that waits (the `exit` keeps it from exec'ing `timeout`),
    # so that the shell's "Aborted" notice for a side that aborted goes
    # nowhere rather than into the report.
    (timeout -k 1 3 env LC_ALL=$loc "$DIFF_TMP/probe" < "$f" > "$o/g.out" 2> "$o/g.err"; exit $?) 2>/dev/null; g_rc=$?
    (timeout -k 1 3 env LC_ALL=$loc "$OURS" < "$f" > "$o/o.out" 2> "$o/o.err"; exit $?) 2>/dev/null; o_rc=$?
    if [ "$o_rc" = 124 ]; then
      # The port never fails to finish, whatever upstream does.
      fail=$((fail + 1))
      printf 'HANG %s (LC_ALL=%s): ours timed out, upstream rc=%s\n' "${f##*/}" "$loc" "$g_rc"
    elif { [ "$g_rc" = 124 ] || [ "$g_rc" = 139 ]; } && [ "$o_rc" = 0 ]; then
      unfinished=$((unfinished + 1))
      [ -n "${VERBOSE:-}" ] && printf 'UPSTREAM-UNFINISHED %s (LC_ALL=%s): rc=%s\n' "${f##*/}" "$loc" "$g_rc"
    elif cmp -s "$o/o.out" "$o/g.out" && cmp -s "$o/o.err" "$o/g.err" && [ "$o_rc" = "$g_rc" ]; then
      pass=$((pass + 1))
      [ "$g_rc" = 134 ] && aborts=$((aborts + 1))
      grep -q -e ',->' -e "$(printf '\342\224\206')" -e "$(printf '\342\226\266')" "$o/g.out" && charts=$((charts + 1))
    else
      fail=$((fail + 1))
      printf 'DIFF %s (LC_ALL=%s)\n  ours rc=%s, upstream rc=%s\n' "${f##*/}" "$loc" "$o_rc" "$g_rc"
      diff -u "$o/g.out" "$o/o.out" | sed -n '3,16p' | sed 's/^/  out /'
      diff -u "$o/g.err" "$o/o.err" | sed -n '3,8p' | sed 's/^/  err /'
      if [ "$KEEP" = 1 ]; then
        cp "$o/g.out" "$f.$loc.upstream.out"; cp "$o/o.out" "$f.$loc.ours.out"
      fi
    fi
  done
done

# The scripts must reach what they are for, or agreement proves nothing.
echo "smartcols-diff: $pass agree, $fail differ ($charts with a group chart, $aborts aborted on both sides; upstream never finished $unfinished)"
if [ "$charts" = 0 ]; then
  echo "smartcols-diff: no script drew a group chart; the cases are not discriminating" >&2
  exit 1
fi
[ "$fail" = 0 ]
