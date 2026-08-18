#!/usr/bin/env bash
# Differential test: our `coreutils::extfloat` against glibc's `strtold` and
# `printf`.
#
# ## Why this exists separately from `seq-diff.sh`
#
# `seq` is the first utility that needs an 80-bit float, and it would be
# possible to certify the float only through `seq`'s own output. That would be
# a worse test in both directions. A `seq` run reaches a narrow slice of the
# format -- no infinities, no subnormals, no hexadecimal input, one conversion
# out of eight -- so most of `extfloat` would go unmeasured; and when a `seq`
# case did fail, nothing would say whether the fault was in the option parsing,
# the sequence arithmetic, or the digits. Testing the float directly makes the
# next utility that needs one (`printf %f`, `sort -g`) inherit a certified
# component rather than a plausible one.
#
# ## Why the reference is glibc, and only glibc
#
# The same reason as every other harness here: the host's `/usr/bin` is MSYS2,
# a Cygwin derivative, and its `printf` is not glibc's. It matters more for this
# file than for most -- Cygwin's `long double` *is* x87 80-bit, so the two agree
# often enough to look like a passing test while disagreeing on exactly the
# cases that motivated the module. So the C side is compiled and run inside WSL,
# never on the host.
#
# There is no `OURS=` discrimination check here, because there is nothing on the
# host to point it at: the thing under test is a library, not a program with a
# system counterpart. What plays that role instead is `--flip`, which runs the
# C probe against itself with the two modes' outputs swapped, and must report
# differences. If it reports none, the comparison is not comparing.
#
# ## Why `LC_ALL=C`
#
# glibc's `printf` takes the decimal point and the thousands separator from the
# locale, so a `%f` under a comma-decimal locale prints `1,5`. `seq` sets
# `LC_NUMERIC` and then works in whatever it got; `extfloat` implements the C
# locale only, which is what the OS's own `seq` will run under. Pinning the
# reference to `C` measures that claim rather than the host's environment.
#
# Usage:
#   ./scripts/extfloat-diff.sh              # both modes, default case count
#   ./scripts/extfloat-diff.sh --cases 200  # a quicker pass
#   ./scripts/extfloat-diff.sh --flip       # prove the harness discriminates
#   ./scripts/extfloat-diff.sh --keep       # leave the case and output files

set -u

cd "$(dirname "$0")/.." || exit 1

CASES=4000
FLIP=0
KEEP=0
while [ $# -gt 0 ]; do
  case "$1" in
    --cases) CASES="$2"; shift 2 ;;
    --flip) FLIP=1; shift ;;
    --keep) KEEP=1; shift ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done

TARGET=x86_64-pc-windows-gnu
OURS=${OURS:-target/$TARGET/debug/examples/extfloat-probe.exe}

if [ ! -x "$OURS" ]; then
  echo "building the probe..."
  cargo build -p coreutils --example extfloat-probe --target "$TARGET" || exit 1
fi
if [ ! -x "$OURS" ]; then
  echo "no probe at $OURS" >&2
  exit 1
fi

WORK=$(mktemp -d)
cleanup() { [ "$KEEP" = 1 ] || rm -rf "$WORK"; }
trap cleanup EXIT
[ "$KEEP" = 1 ] && echo "working files in $WORK"

# The C probe is compiled inside WSL, from a copy under /tmp: a build in the
# Windows-mounted tree would write its output through the 9p mount, which is
# slow enough to dominate the run.
export MSYS2_ARG_CONV_EXCL='*'
wsl -e bash -c 'mkdir -p /tmp/extfloat && cat > /tmp/extfloat/probe.c' < scripts/extfloat-probe.c || exit 1
if ! wsl -e bash -c 'cd /tmp/extfloat && gcc -O2 -o probe probe.c 2>&1'; then
  echo "could not build the reference probe in WSL" >&2
  exit 1
fi

total=0

run_mode() {
  mode=$1
  echo
  echo "=== $mode ==="
  python scripts/extfloat-cases.py "$mode" "$CASES" > "$WORK/$mode.cases" || exit 1
  lines=$(wc -l < "$WORK/$mode.cases")
  echo "$lines cases"

  # Both sides read the identical byte stream. The case file is copied into WSL
  # rather than read across the mount for the same reason the source was.
  wsl -e bash -c "cat > /tmp/extfloat/$mode.cases" < "$WORK/$mode.cases" || exit 1
  wsl -e bash -c "cd /tmp/extfloat && LC_ALL=C ./probe $mode < $mode.cases" > "$WORK/$mode.theirs" || exit 1
  "$OURS" "$mode" < "$WORK/$mode.cases" > "$WORK/$mode.ours" || exit 1

  if [ "$FLIP" = 1 ]; then
    # Deliberately misalign the reference by one line. Every case must then
    # differ from its neighbour, or the cases are not discriminating.
    tail -n +2 "$WORK/$mode.theirs" > "$WORK/$mode.theirs.flip"
    mv "$WORK/$mode.theirs.flip" "$WORK/$mode.theirs"
  fi

  # The three files are read by line number rather than pasted into columns:
  # a read case may itself contain a tab (a leading tab is skipped by
  # `strtold`, so ` \t1` is a case worth having), which would shift the columns
  # of a `paste`-and-split comparison and report the whole file as different.
  # Reading them separately also lets a line-count mismatch be named as such
  # instead of appearing as thousands of differences.
  awk -v casefile="$WORK/$mode.cases" \
      -v oursfile="$WORK/$mode.ours" \
      -v theirsfile="$WORK/$mode.theirs" '
      BEGIN {
        n = 0
        while ((getline line < casefile)   > 0) c[++i] = line
        while ((getline line < oursfile)   > 0) o[++j] = line
        while ((getline line < theirsfile) > 0) t[++k] = line
        if (i != j || i != k)
          printf "  LINE COUNT MISMATCH: cases %d ours %d theirs %d\n", i, j, k
        m = (j < k ? j : k)
        for (x = 1; x <= m; x++) {
          # There is no skip list. An earlier version of this script excused
          # subnormal results, because this module rounded them correctly and
          # glibc does not; the module now reproduces the double rounding that
          # glibc does, precisely so that the hardest cases in the file are the
          # ones being measured rather than the ones being waved through.
          # (No apostrophes in here: the awk program is single-quoted.)
          if (o[x] == t[x])
            continue
          if (n < 25)
            printf "  case %-40s ours %-28s theirs %s\n", c[x], o[x], t[x]
          n++
        }
        if (n > 25) printf "  ... and %d more\n", n - 25
        print "COUNT " n
      }' > "$WORK/$mode.report"

  n=$(sed -n 's/^COUNT //p' "$WORK/$mode.report")
  grep -v '^COUNT ' "$WORK/$mode.report"
  echo "$n difference(s)"
  total=$((total + n))
}

run_mode read
run_mode write

echo
if [ "$FLIP" = 1 ]; then
  if [ "$total" = 0 ]; then
    echo "FLIP FAILED: a deliberately misaligned reference produced no differences,"
    echo "so this harness is not comparing anything."
    exit 1
  fi
  echo "flip check: $total difference(s) -- the harness discriminates"
  exit 0
fi

if [ "$total" = 0 ]; then
  echo "no differences"
else
  echo "TOTAL $total difference(s)"
fi
[ "$total" = 0 ]
