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
# ## Why the whole run is inside WSL
#
# `diff-wsl.sh` puts it there, and both sides with it. The subject used to be
# built for `x86_64-pc-windows-gnu` and run on the host while only the C probe
# lived in WSL, which meant six `wsl -e` round trips per run and a copy of every
# case file through the 9p mount -- but, much more to the point, it meant the
# subject was built by a path that has no staleness guard. `extfloat-probe` is
# an example in the `coreutils` package, so it links the same library that was
# three commits stale on 2026-08-24 while `cargo build` kept exiting 0. The
# preamble's `diff_assert_fresh` is the reason to be here; the round trips going
# away is a bonus.
#
# Nothing about the arithmetic changes with the triple -- `extfloat` builds its
# 80-bit format out of integers, and `f64` is SSE on both -- but the Linux build
# is the one closer to what `x86_64-slateos` will run.
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

DIFF_PROG=extfloat
# The subject is an example, not a utility: it exposes a library, and anything
# in `src/bin/` would be installed into the image. So there is no `--bin` to
# build, no reference of the same name on `PATH` (`DIFF_NO_REF`), and no pair of
# same-named binaries to put behind one `PATH` entry (`DIFF_NO_BINDIR`).
DIFF_EXAMPLES=extfloat-probe
DIFF_NO_REF=1
DIFF_NO_BINDIR=1
# `gcc` builds the reference; `python3` generates the cases. Without either, the
# run would be skipped rather than reported as agreement.
DIFF_NEED="gcc python3"
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

# Parsed *after* the preamble, which carries `$@` across the re-exec intact.
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

WORK=$DIFF_TMP
# `--keep` is honoured by replacing the preamble's cleanup rather than by
# cancelling its trap: a second `trap ... EXIT` would replace the first, and the
# next thing added to `diff_cleanup` would silently stop running here.
if [ "$KEEP" = 1 ]; then
  diff_cleanup() { echo "working files in $DIFF_TMP"; }
fi

# The reference is compiled into `$DIFF_TMP`, which is on WSL's own filesystem;
# the source is read once from the mounted tree, which is cheap, but writing a
# build's output through 9p is not.
if ! gcc -O2 -o "$WORK/probe" "$root/scripts/extfloat-probe.c"; then
  echo "could not build the reference probe" >&2
  exit 1
fi

total=0

run_mode() {
  mode=$1
  echo
  echo "=== $mode ==="
  python3 "$root/scripts/extfloat-cases.py" "$mode" "$CASES" > "$WORK/$mode.cases" || exit 1
  lines=$(wc -l < "$WORK/$mode.cases")
  echo "$lines cases"

  # Both sides read the identical byte stream, from the same file.
  #
  # `LC_ALL=C` rather than the preamble's `C.UTF-8`, and deliberately: glibc's
  # `printf` takes the decimal point from `LC_NUMERIC`, and the claim being
  # measured is that `extfloat` implements the C locale. The two agree on the
  # decimal point today; naming the one that is being claimed is what keeps the
  # test honest if that ever stops being true.
  LC_ALL=C "$WORK/probe" "$mode" < "$WORK/$mode.cases" > "$WORK/$mode.theirs" || exit 1
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
