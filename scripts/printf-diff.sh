#!/usr/bin/env bash
# Differential test: our `printf` against GNU's.
#
# ## Why both sides run inside WSL
#
# `scripts/diff-wsl.sh` gives the general reasons; the one that decided it here
# is the reference. The host's `/usr/bin` is MSYS2, a Cygwin derivative. Its
# `printf` *is* GNU coreutils' printf, so pointing this harness at it is a
# useful positive control (`OURS=/usr/bin/printf ./scripts/printf-diff.sh`
# should report almost nothing) -- but it is not a reference, because the
# numbers printf prints come from the C library, and Cygwin's is not glibc's.
# The two agree on most of what printf prints, which is worse than disagreeing
# on all of it: it would look like a passing test while diverging on exactly the
# 80-bit corners that `coreutils::extfloat` exists for.
#
# The subject moved into WSL with it, which is what this file gained by being
# migrated. It used to be a `x86_64-pc-windows-gnu` build driven from the host
# while the reference ran under `wsl -e bash -c`, and that split cost three
# things: `MSYS2_ARG_CONV_EXCL='*'` had to be exported so MSYS would not rewrite
# the format `[%05.2f]` or the argument `/` into something with a drive letter
# in it; the case file had to be *copied* into WSL through `cat`, because a
# `wsl -e bash -c '...'` command line passes through two shells and a Win32
# command-line encoder that a format like `%\303\251` or an argument that is a
# single backslash does not survive -- printf needed that more than any other
# harness, since its first argument is a format string and the formats worth
# testing are precisely the ones made of awkward bytes; and the two printfs were
# linked against different C libraries, so any difference could be blamed on the
# host. Both sides are now the same kind of binary on the same kernel, and the
# case file is read twice from where it was written.
#
# ## Why the reference is reached through a directory and not by path
#
# Because `printf` is a shell builtin, and because GNU's diagnostics carry
# `argv[0]`. The preamble's `$bindir` threads that needle; the reasoning is in
# `scripts/printf-probe.sh`.
#
# ## Why `LC_ALL=C.UTF-8`
#
# Fixed by the preamble. Three of printf's answers are locale-dependent, and
# this file used to pin `C` because two of them were implemented for `C`. Both
# have since moved, so `C` is now the setting in which the *reference* would be
# wrong:
#
#   * gnulib's `quote()` wraps the offending argument in every numeric
#     diagnostic. It prints U+2018/U+2019 under a UTF-8 locale and ASCII
#     apostrophes under `C`; since §351 ours prints the curly pair in every
#     locale, which is the UTF-8 answer.
#   * `\uXXXX` is converted to the locale's charset, so `\u00e9` is two bytes
#     under a UTF-8 locale and comes back out as the literal text `\u00E9`
#     under `C`, where the charset is ASCII and the conversion fails. Ours
#     implemented the `C` branch until the same reasoning that produced §351
#     was applied to it: Q38 settled that SlateOS has no non-UTF-8 locale, so
#     an ASCII-charset branch implements a configuration that cannot occur.
#   * `%f` takes its decimal point from `LC_NUMERIC`, so under a comma-decimal
#     locale GNU prints `1,500000`. This one does *not* move: `C.UTF-8` has the
#     same `LC_NUMERIC` as `C`, so `extfloat`'s `.` is right in both and the
#     flip cannot have hidden a change here.
#
# Pinning both sides to `C.UTF-8` measures those claims instead of the
# development host's environment.
#
# ## `--help` and `--version` are not cases
#
# Both print text that names the implementation, so they can never match GNU
# byte for byte, and carrying them as permanent expected-differences would only
# train the reader to skim the report. What they actually have to satisfy --
# stdout, status 0 -- the unit tests in `printf.rs` check.
#
# Usage:
#   ./scripts/printf-diff.sh              # the full set
#   ./scripts/printf-diff.sh --cases 50   # fewer random cases, for a quick pass
#   ./scripts/printf-diff.sh --flip       # prove the harness discriminates
#   ./scripts/printf-diff.sh --keep       # leave the case and record files behind

set -u

# Into WSL, build ours for Linux, find glibc's, and put both behind the one name
# `printf` so `argv[0]` matches. See `scripts/diff-wsl.sh`.
#
# `DIFF_REF` is spelled out rather than left to the default `command -v printf`,
# which finds the shell builtin: the preamble is sourced by a shell, and the
# builtin is not the program under comparison. See `scripts/printf-probe.sh`.
DIFF_PROG=printf
DIFF_REF="/usr/bin/printf /bin/printf"
DIFF_NEED="timeout python3"
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

cd "$root" || exit 1

RANDOM_CASES=400
SEED=20260817
FLIP=0
KEEP=0
while [ $# -gt 0 ]; do
  case "$1" in
    --cases) RANDOM_CASES="$2"; shift 2 ;;
    --seed) SEED="$2"; shift 2 ;;
    --flip) FLIP=1; shift ;;
    --keep) KEEP=1; shift ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done

# `--keep` works by replacing the cleanup function rather than by setting a
# second `EXIT` trap, which would replace the preamble's one instead of adding
# to it. The whole scratch directory then survives, symlinked binaries and all,
# which is what makes a kept case reproducible by hand.
WORK=$DIFF_TMP/work
mkdir -p "$WORK"
if [ "$KEEP" = 1 ]; then
  diff_cleanup() { echo "kept: $DIFF_TMP (cases and records in work/)"; }
fi

echo "ours: $OURS"
echo "gnu:  $gnu_real"

python3 scripts/printf-cases.py "$RANDOM_CASES" "$SEED" > "$WORK/cases" || exit 1
echo "$(wc -l < "$WORK/cases") cases"

echo "running ours..."
bash scripts/printf-probe.sh "$bindir/ours" "$WORK/cases" > "$WORK/ours" || exit 1

echo "running GNU's..."
bash scripts/printf-probe.sh "$bindir/gnu" "$WORK/cases" > "$WORK/theirs" || exit 1

if [ "$FLIP" = 1 ]; then
  # Drop the reference's first record, so every case is compared against its
  # neighbour's answer. Nearly all of them must then differ; if they do not,
  # the harness is not comparing anything.
  tail -n +5 "$WORK/theirs" > "$WORK/theirs.flip"
  mv "$WORK/theirs.flip" "$WORK/theirs"
fi

# The two record files are read by line number rather than pasted together:
# a record line can be long, and a line-count mismatch is worth naming as such
# instead of appearing as several hundred differences.
awk -v oursfile="$WORK/ours" \
    -v theirsfile="$WORK/theirs" \
    -v casefile="$WORK/cases" \
    -v flip="$FLIP" '
    BEGIN {
      us = sprintf("%c", 31)
      while ((getline line < oursfile)   > 0) o[++i] = line
      while ((getline line < theirsfile) > 0) t[++j] = line
      while ((getline line < casefile)   > 0) { gsub(us, " ", line); c[++k] = line }
      if (i % 4 != 0 || j % 4 != 0)
        printf "  RAGGED RECORDS: ours %d lines, theirs %d\n", i, j
      if (i != j && !flip)
        printf "  LINE COUNT MISMATCH: ours %d theirs %d\n", i, j
      nb = int((i < j ? i : j) / 4)

      pass = 0; fail = 0; xfail = 0; xpass = 0; shown = 0
      for (b = 1; b <= nb; b++) {
        base = (b - 1) * 4
        differs = 0
        for (r = 1; r <= 4; r++)
          if (o[base + r] != t[base + r]) differs = 1
        # A case name beginning with x is one where we differ on purpose --
        # see the generator. It has to keep differing: an expected difference
        # that has quietly gone away is a recorded reason that no longer
        # describes reality, and is reported rather than passed over.
        name = substr(o[base + 1], 6)
        expected = (substr(name, 1, 1) == "x")
        if (expected && differs) { xfail++; continue }
        if (expected && !differs) {
          xpass++
          printf "XPASS  printf %s\n    now agrees with GNU, so the recorded reason is stale\n", c[b]
          continue
        }
        if (!differs) { pass++; continue }
        fail++
        if (shown < 25) {
          shown++
          printf "DIFF   printf %s\n", c[b]
          for (r = 2; r <= 4; r++)
            if (o[base + r] != t[base + r]) {
              printf "    ours   %s\n", o[base + r]
              printf "    gnu    %s\n", t[base + r]
            }
        }
      }
      if (fail > shown) printf "  ... and %d more\n", fail - shown
      printf "SUMMARY %d %d %d %d\n", pass, fail, xfail, xpass
    }' > "$WORK/report"

grep -v '^SUMMARY ' "$WORK/report"
read -r _ pass fail xfail xpass < <(grep '^SUMMARY ' "$WORK/report")

echo
printf '%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
if [ "$xpass" -gt 0 ]; then
  printf ' (%d of which no longer do)' "$xpass"
fi
printf '\n'

if [ "$FLIP" = 1 ]; then
  if [ "$fail" -eq 0 ]; then
    echo "FLIP FAILED: a deliberately misaligned reference produced no"
    echo "differences, so this harness is not comparing anything."
    exit 1
  fi
  echo "flip check: $fail difference(s) -- the harness discriminates"
  exit 0
fi

# An xpass is not a failure in the sense that the output got worse -- agreeing
# with GNU is never worse -- but it does mean a written-down decision has gone
# stale, so it must not pass silently.
[ "$fail" -eq 0 ] && [ "$xpass" -eq 0 ]
