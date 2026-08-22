#!/usr/bin/env bash
# Differential test: our `seq` against GNU's.
#
# ## Why the reference is inside WSL
#
# The host's `/usr/bin` is MSYS2, a Cygwin derivative. Its `seq` *is* GNU
# coreutils' seq, so pointing this harness at it is a useful positive control
# (`OURS=/usr/bin/seq ./scripts/seq-diff.sh` should report almost nothing) --
# but it is not a reference, because seq's answers are printed by the C
# library's `printf`, and Cygwin's is not glibc's. The two agree on most of
# what seq prints, which is worse than disagreeing on all of it: it would look
# like a passing test while diverging on exactly the 80-bit corners that
# `coreutils::extfloat` exists for. So the reference is run in WSL.
#
# ## Why the cases are a file and not a list of shell lines
#
# Both sides have to receive byte-identical argv, and they run under different
# operating systems -- ours natively on Windows, GNU's under WSL. Anything
# quoted into a `wsl -e bash -c '...'` command line passes through two shells
# and a Win32 command-line encoder, and an argument like `%\303\251` or a
# separator that is a single newline does not survive that intact. Writing the
# cases to a file, copying the file, and having an identical probe script read
# it on both sides removes every layer that could rewrite an argument.
#
# ## Why `LC_ALL=C.UTF-8`
#
# Two of seq's answers could move with the locale, and only one of them
# actually does.
#
# The decimal point is the one that does not. GNU seq takes it from
# `LC_NUMERIC`, so under a comma-decimal locale it prints `1,5` -- but
# `C.UTF-8` has the same `LC_NUMERIC` as `C`, so `extfloat`'s `.` is the right
# answer in both and this file's choice between them cannot move a number.
#
# The quote marks are the one that does. Every diagnostic seq prints about a
# bad format or a bad operand wraps the offending text in gnulib's `quote()`,
# which is U+2018/U+2019 under a UTF-8 locale and ASCII apostrophes under `C`.
# Since §351 ours prints the curly pair in every locale, so `C` is now the
# setting in which the *reference* would be wrong. This file pinned `C` for the
# mirror-image of that reason, back when ours stayed ASCII
# (`open-questions.md` -> B-Q2, since answered).
#
# seq reads the locale nowhere else. Unlike printf it expands no escapes, so it
# has no `\uXXXX` whose conversion to the locale's charset could differ.
#
# ## `--help` and `--version` are not cases
#
# Both print text that names the implementation, so they can never match GNU
# byte for byte, and carrying them as permanent expected-differences would only
# train the reader to skim the report. What they actually have to satisfy --
# stdout, status 0 -- the unit tests in `seq.rs` check.
#
# Usage:
#   ./scripts/seq-diff.sh              # the full set
#   ./scripts/seq-diff.sh --cases 50   # fewer random cases, for a quick pass
#   ./scripts/seq-diff.sh --flip       # prove the harness discriminates
#   ./scripts/seq-diff.sh --keep       # leave the case and record files behind

set -u

cd "$(dirname "$0")/.." || exit 1

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

TARGET=x86_64-pc-windows-gnu
# Whether `OURS` was chosen by the caller, which decides whether it is ours to
# build. Checked before the default is applied, because after that they look
# alike.
OURS_IS_DEFAULT=${OURS+no}
OURS=${OURS:-target/$TARGET/debug/seq.exe}
GNU=${GNU:-seq}

# Our seq is a native Windows binary, so MSYS would helpfully rewrite any
# argument that looks like a path -- turning the format `[%05.2f]` or the
# operand `-1` into something with a drive letter in it.
export MSYS2_ARG_CONV_EXCL='*'
export LC_ALL=C.UTF-8

# Built every run, not just when the binary is missing. `cargo build` is a
# no-op on an unchanged tree, so the only thing the "is it there?" version
# saved was correctness: `cargo test` and `cargo clippy` do not refresh
# `seq.exe`, so a fix verified by a unit test and then measured here would be
# measured against the *previous* binary. That is not hypothetical -- it
# happened in the printf harness, which was a copy of this one.
if [ "${OURS_IS_DEFAULT:-yes}" = yes ]; then
  cargo build -p coreutils --bin seq --target "$TARGET" || exit 1
fi
if [ ! -x "$OURS" ]; then
  echo "no seq at $OURS" >&2
  exit 1
fi

WORK=$(mktemp -d)
cleanup() { [ "$KEEP" = 1 ] || rm -rf "$WORK"; }
trap cleanup EXIT
[ "$KEEP" = 1 ] && echo "working files in $WORK"

python scripts/seq-cases.py "$RANDOM_CASES" "$SEED" > "$WORK/cases" || exit 1
echo "$(wc -l < "$WORK/cases") cases"

echo "running ours..."
bash scripts/seq-probe.sh "$OURS" "$WORK/cases" > "$WORK/ours" || exit 1

echo "running GNU's, in WSL..."
wsl -e bash -c 'mkdir -p /tmp/seqdiff && cat > /tmp/seqdiff/probe.sh' \
  < scripts/seq-probe.sh || exit 1
wsl -e bash -c 'cat > /tmp/seqdiff/cases' < "$WORK/cases" || exit 1
wsl -e bash -c "cd /tmp/seqdiff && LC_ALL=C.UTF-8 bash probe.sh '$GNU' cases" \
  > "$WORK/theirs" || exit 1

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
          printf "XPASS  seq %s\n    now agrees with GNU, so the recorded reason is stale\n", c[b]
          continue
        }
        if (!differs) { pass++; continue }
        fail++
        if (shown < 25) {
          shown++
          printf "DIFF   seq %s\n", c[b]
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
