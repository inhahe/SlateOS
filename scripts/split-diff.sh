#!/usr/bin/env bash
# Differential test: our split against GNU split.
#
# ## Why this harness compares files, and not just stdout
#
# `split` writes nothing to stdout in its ordinary use; its entire product is a
# set of files left in the working directory, and almost everything there is to
# get wrong is *which* bytes landed in *which* name. So, as in `csplit-diff.sh`,
# each case runs both implementations in a private directory and compares a
# manifest — every output file, in name order, with its contents through
# `od -An -c` so a stray CR or a missing final newline cannot hide.
#
# The two runs cannot share a directory. Both write `xaa`, and whichever ran
# second would win; the comparison would then be a file against itself.
#
# ## Why some cases compare names only
#
# The suffix auto-widening scheme only shows itself after 650 alphabetic names
# (or 90 numeric, or 240 hex), so exercising it needs a case that produces
# hundreds of files. Running `od` over 700 files twice per case is minutes of
# process spawning for a property that is entirely about the *names*; those
# cases use `names_case`, which compares the sorted file list and skips the
# contents. The contents of a one-byte piece are not in doubt.
#
# ## Why the reference is glibc, and only glibc
#
# The host's `split` is MSYS2's, a Cygwin derivative linking `msys-2.0.dll`
# rather than glibc, and its `getopt` words every option diagnostic
# differently. See `known-issues.md` →
# `TD-COREUTILS-GETOPT-DIAGNOSTICS-USE-THE-WRONG-SHAPE`, and the identical
# note at the top of `csplit-diff.sh`, `od-diff.sh` and `wc-diff.sh`.
#
# ## Why LC_ALL=C.UTF-8
#
# The diagnostics quote the offending argument back at the caller through gnulib's
# `quote()`, which picks its quote marks from the locale. Since §351 ours
# prints U+2018/U+2019 in every locale, and GNU prints those under a UTF-8
# locale and ASCII under `C` — so `C.UTF-8` is the setting the two agree in.
# This file ran under `C` for the mirror-image of that reason until B-Q2 was
# answered; nothing else here reads the locale.
set -u

# Our split is a native Windows binary, so MSYS would rewrite an argument that
# looks like a path — and `-n l/3`, `-n r/2/3` and `--filter='cat > $FILE'` all
# can.
export MSYS2_ARG_CONV_EXCL='*'

OURS=${OURS:-"target/x86_64-pc-windows-gnu/debug/split.exe"}
export LC_ALL=${LC_ALL:-C.UTF-8}

pass=0; fail=0; xfail=0; xpass=0

fixtures=$(mktemp -d)
trap 'rm -rf "$fixtures"' EXIT
OURS_ABS=$OURS
case $OURS in /*|[A-Za-z]:*) ;; *) OURS_ABS="$PWD/$OURS" ;; esac
cd "$fixtures" >/dev/null || exit 1

# WSL is invoked with the Windows cwd, which for an MSYS temp directory lands
# on the same bytes under `/mnt/c/...`. Verified rather than assumed: a
# reference that silently ran somewhere else would write its output files into
# a different directory, and every manifest comparison would then be "ours has
# files, GNU has none" — which looks like a total divergence rather than a
# broken harness.
printf '1\n2\n' > .probe
if wsl -e env LC_ALL=C.UTF-8 split -l 1 .probe .p >/dev/null 2>&1 && [ -f .paa ]; then
  HAVE_GNU=yes
else
  HAVE_GNU=no
  echo "split-diff: glibc split not reachable in this directory; skipping"
fi
rm -f .probe .paa .pab

# --- fixtures -----------------------------------------------------------------

# 1..20, one per line: 51 bytes, because the run crosses the 1->2 digit
# boundary at line 10. That matters more than it looks. `-n l/7` on 51 bytes
# puts the fifth boundary at floor(5*51/7) = 36, where the "truncated share"
# formula 5*(51/7) = 35 puts it one byte earlier — on the other side of a
# newline, so a whole line moves between pieces. A fixture whose length were a
# multiple of the line length could not tell the two formulas apart.
seq 1 20 > seq20.txt

# No trailing newline. The last record is still a record, so `-l` and `-n r/`
# must both emit it, and `-C` must count it.
printf 'x\ny\nz' > nonl.txt

# Empty. Every chunk mode must still create all N files from it.
: > empty.txt

# Records of deliberately mixed length for `-C`, including one longer than any
# limit the cases use, so the cutting path is reached rather than only the
# packing path.
printf 'aaaa\nbb\nccccccccccccc\nd\nee\n' > rec.txt

# Ten bytes, no separator anywhere. `-l` sees one record, `-C` sees one record
# too long to fit, and `-n l/N` finds no boundary to round forward to.
printf '0123456789' > tiny.txt

# Colon-separated, for --separator.
printf 'a:b:c:d:e:' > colons.txt

# 700 one-character records. 700 crosses the alphabetic widening point at 650,
# the numeric one at 90 and the hex one at 240, so one fixture drives all three
# name_case runs.
for i in $(seq 1 700); do printf 'z'; done > wide.txt

# --- machinery ----------------------------------------------------------------

run_ours() { ( cd "$1" && "$OURS_ABS" "${@:2}" ); }
run_gnu()  { ( cd "$1" && wsl -e env LC_ALL=C.UTF-8 split "${@:2}" ); }

# Every file the run left behind, in name order, with its contents.
#
# A glob, not `for f in $(ls | sort)`: an unquoted command substitution is
# word-split on IFS, so a name containing a space arrives in pieces and one
# ending in a space loses it. `split --additional-suffix=' x'` is enough to
# reach that, and the failure mode is silent -- the manifest reads a file that
# does not exist and prints nothing for it, scoring a harness bug as a
# divergence. Same fix, and same reason, as `csplit-diff.sh`, where a case
# actually landed on it.
manifest() {
  local dir=$1 f
  ( cd "$dir" || return 0
    for f in *; do
      # `*` with no matches expands to itself; `in.txt` is the input we copied
      # in, not something the run produced.
      case $f in '*') [ -e "$f" ] || continue ;; in.txt) continue ;; esac
      printf '  %s: %s\n' "$f" "$(od -An -c < "$f" | tr -s ' \n' ' ')"
    done )
}

# The names alone, for the cases that produce hundreds of files.
names() {
  ( cd "$1" || return 0; ls | grep -v '^in\.txt$' | sort | tr '\n' ' ' )
}

# $1 = fixture (or `-` for none), $2 = `full` or `names`, $3 = stdin redirect
# source (`-` for none), rest = the whole argv.
#
# The argv is passed whole rather than assembled here because a few cases have
# to put something *before* the file operand, and a few have no file operand at
# all (reading stdin).
compare_argv() {
  local fixture=$1 depth=$2 stdin=$3; shift 3
  local o_out g_out o_err g_err o_rc g_rc o_man g_man
  rm -rf o g; mkdir -p o g
  if [ "$fixture" != - ]; then
    cp "$fixture" o/in.txt; cp "$fixture" g/in.txt
  fi

  o_err=$(mktemp); g_err=$(mktemp)
  local o_bin g_bin; o_bin=$(mktemp); g_bin=$(mktemp)
  # stdout through a file, not a pipe: in `x=$(split | od)` the recorded status
  # is od's, and PIPESTATUS is set in the substitution's subshell where it
  # cannot be read. Same note as csplit-diff.sh and cat-diff.sh.
  if [ "$stdin" = - ]; then
    run_ours o "$@" >"$o_bin" 2>"$o_err" </dev/null; o_rc=$?
    run_gnu  g "$@" >"$g_bin" 2>"$g_err" </dev/null; g_rc=$?
  else
    run_ours o "$@" >"$o_bin" 2>"$o_err" <"$stdin"; o_rc=$?
    run_gnu  g "$@" >"$g_bin" 2>"$g_err" <"$stdin"; g_rc=$?
  fi

  o_out=$(od -An -c <"$o_bin"); g_out=$(od -An -c <"$g_bin")
  rm -f "$o_bin" "$g_bin"
  local o_msg g_msg
  o_msg=$(cat "$o_err"); g_msg=$(cat "$g_err")
  rm -f "$o_err" "$g_err"

  if [ "$depth" = names ]; then
    o_man=$(names o); g_man=$(names g)
  else
    o_man=$(manifest o); g_man=$(manifest g)
  fi

  if [ "$o_out" = "$g_out" ] && [ "$o_rc" = "$g_rc" ] \
     && [ "$o_msg" = "$g_msg" ] && [ "$o_man" = "$g_man" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  ours (rc=%s): %s  {%s}\n%s\n  gnu  (rc=%s): %s  {%s}\n%s' \
    "$o_rc" "$(printf '%s' "$o_out" | tr -s ' \n' ' ')" "$(printf '%s' "$o_msg" | tr '\n' '|')" "$o_man" \
    "$g_rc" "$(printf '%s' "$g_out" | tr -s ' \n' ' ')" "$(printf '%s' "$g_msg" | tr '\n' '|')" "$g_man")
  rm -rf o g
}

report() {
  local label="$1"
  if [ "$AGREED" = yes ]; then
    pass=$((pass+1))
    [ -n "${VERBOSE:-}" ] && printf 'OK   %s\n' "$label"
  else
    fail=$((fail+1))
    printf 'DIFF %s\n%s\n' "$label" "$REPORT"
  fi
  return 0
}

# The common shape: options, then the fixture as the input operand.
run_case() {
  [ "$HAVE_GNU" = yes ] || return 0
  local fixture=$1; shift
  compare_argv "$fixture" full - "$@" in.txt
  report "split $* $fixture"
}

# The same, comparing names only — for the hundreds-of-files cases.
names_case() {
  [ "$HAVE_GNU" = yes ] || return 0
  local fixture=$1; shift
  compare_argv "$fixture" names - "$@" in.txt
  report "split $* $fixture (names)"
}

# The uncommon shape: the whole argv, for cases needing something before the
# operand, or no operand at all.
raw_case() {
  [ "$HAVE_GNU" = yes ] || return 0
  local fixture=$1; shift
  compare_argv "$fixture" full - "$@"
  report "split $*"
}

# Reading the fixture from stdin rather than naming it.
stdin_case() {
  [ "$HAVE_GNU" = yes ] || return 0
  local fixture=$1; shift
  compare_argv "$fixture" full "$fixture" "$@"
  report "split $* < $fixture"
}

# A case we expect to differ, with the reason. Counted separately so that a
# case that starts agreeing is reported too — an xfail that silently becomes
# correct is a stale note in the harness.
xfail_case() {
  [ "$HAVE_GNU" = yes ] || return 0
  local why="$1" fixture=$2; shift 2
  compare_argv "$fixture" full - "$@" in.txt
  if [ "$AGREED" = yes ]; then
    xpass=$((xpass+1)); printf 'XPASS split %s  (expected to differ: %s)\n' "$*" "$why"
  else
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'XFAIL split %s  (%s)\n' "$*" "$why"
  fi
  return 0
}

# --- lines --------------------------------------------------------------------

run_case seq20.txt
run_case seq20.txt -l 3
run_case seq20.txt -l 1
run_case seq20.txt -l 20
run_case seq20.txt -l 21
run_case seq20.txt -l 1000
run_case seq20.txt --lines=7
run_case nonl.txt -l 2
run_case nonl.txt -l 1
run_case tiny.txt -l 1
run_case empty.txt -l 3
# The obsolete `-N` form, still accepted, still meaning `-l N`.
run_case seq20.txt -6
run_case seq20.txt -1

# --- bytes --------------------------------------------------------------------

run_case seq20.txt -b 10
run_case seq20.txt -b 1
run_case seq20.txt -b 51
run_case seq20.txt -b 52
run_case seq20.txt -b 1000
run_case seq20.txt --bytes=17
run_case nonl.txt -b 2
run_case tiny.txt -b 3
run_case empty.txt -b 5
# Multiplier suffixes: `b` is 512, a bare letter is the power, and the `B`/`iB`
# spellings pick the base. All larger than the fixture, so they agree on one
# piece — the case is that they are *accepted*, which `-l` refuses.
run_case seq20.txt -b 1b
run_case seq20.txt -b 1K
run_case seq20.txt -b 1KB
run_case seq20.txt -b 1KiB
run_case seq20.txt -b 1k
run_case seq20.txt -b 1M

# --- line-bytes ---------------------------------------------------------------

# `-C` packs whole records up to the limit and cuts a record that cannot fit
# into limit-sized bites, letting the tail begin the next piece. `rec.txt`'s
# 13-character record is longer than every limit here, so the cutting path runs
# in each of them.
run_case rec.txt -C 2
run_case rec.txt -C 3
run_case rec.txt -C 4
run_case rec.txt -C 5
run_case rec.txt -C 6
run_case rec.txt -C 8
run_case rec.txt -C 12
run_case rec.txt -C 13
run_case rec.txt -C 14
run_case rec.txt -C 15
run_case rec.txt -C 1
run_case rec.txt -C 100
run_case seq20.txt -C 10
run_case seq20.txt --line-bytes=7
run_case nonl.txt -C 2
run_case tiny.txt -C 3
run_case empty.txt -C 5
run_case rec.txt -C 1K

# --- chunks: -n N -------------------------------------------------------------

# The remainder goes to the *first* pieces: 51 bytes in 7 is 8,8,8,7,7,7,6, not
# 7,7,7,7,7,7,9.
run_case seq20.txt -n 3
run_case seq20.txt -n 7
run_case seq20.txt -n 1
run_case seq20.txt -n 51
# More pieces than bytes: the surplus files are created and are empty.
run_case seq20.txt -n 60
run_case tiny.txt -n 3
run_case tiny.txt -n 4
run_case nonl.txt -n 2
# Empty input still gets all N files.
run_case empty.txt -n 3
run_case seq20.txt --number=4

# --- chunks: -n l/N -----------------------------------------------------------

# Each boundary is the first separator at or after floor(i*size/N), computed
# exactly. `l/7` on 51 bytes is the case that separates that from the truncated
# share; see the seq20.txt fixture note.
run_case seq20.txt -n l/3
run_case seq20.txt -n l/7
run_case seq20.txt -n l/1
run_case seq20.txt -n l/20
# Fewer records than pieces: one record each until they run out, then empties.
run_case seq20.txt -n l/30
run_case nonl.txt -n l/2
run_case nonl.txt -n l/5
# No separator anywhere: everything lands in the first piece.
run_case tiny.txt -n l/3
run_case empty.txt -n l/3

# --- chunks: -n r/N -----------------------------------------------------------

run_case seq20.txt -n r/3
run_case seq20.txt -n r/7
run_case seq20.txt -n r/1
run_case seq20.txt -n r/25
run_case nonl.txt -n r/2
run_case tiny.txt -n r/3
run_case empty.txt -n r/3

# --- chunks: a single piece to stdout -----------------------------------------

run_case seq20.txt -n 2/3
run_case seq20.txt -n 1/3
run_case seq20.txt -n 3/3
run_case seq20.txt -n l/2/3
run_case seq20.txt -n l/1/7
run_case seq20.txt -n r/2/3
run_case seq20.txt -n r/1/3
run_case tiny.txt -n 2/4

# --- --elide-empty-files ------------------------------------------------------

# `-e` suppresses the empty pieces *and their names*, so the survivors stay
# consecutively named — the surplus is not merely emptied, it is skipped.
run_case seq20.txt -n 60 -e
run_case seq20.txt -n l/30 -e
run_case seq20.txt -n r/25 -e
run_case tiny.txt -n 4 -e
run_case empty.txt -n 3 -e
run_case seq20.txt -n 3 --elide-empty-files

# --- chunk grammar errors -----------------------------------------------------

# GNU parses `-n`'s argument with `strtoumax`'s end pointer rather than by
# splitting on `/`, which decides *which* text is quoted back. Each of these is
# a different branch of that decision.
run_case seq20.txt -n 0
run_case seq20.txt -n x/3
run_case seq20.txt -n /3
run_case seq20.txt -n 2/x
run_case seq20.txt -n 3/
run_case seq20.txt -n 2/3/4
run_case seq20.txt -n 0/3
run_case seq20.txt -n 4/3
run_case seq20.txt -n abc
run_case seq20.txt -n ''
run_case seq20.txt -n l/0
run_case seq20.txt -n r/0
run_case seq20.txt -n 1K
run_case seq20.txt -n ' 3'
run_case seq20.txt -n +3
# Whitespace is skipped before the `l/`/`r/` prefix is looked for, not only by
# the number scan underneath it.
run_case seq20.txt -n ' l/3'
run_case seq20.txt -n '  r/3'
run_case seq20.txt -n 'l /3'
run_case seq20.txt -n 'l/ 3'
run_case seq20.txt -n l
run_case seq20.txt -n l/
run_case seq20.txt -n r/
run_case seq20.txt -n /
run_case seq20.txt -n 2//3

# --- size errors --------------------------------------------------------------

run_case seq20.txt -l 0
run_case seq20.txt -b 0
run_case seq20.txt -C 0
run_case seq20.txt -l abc
run_case seq20.txt -b abc
run_case seq20.txt -C abc
# `-l` takes no multiplier suffix; `-b` and `-C` do. And `-C`'s bad-argument
# message says "lines" rather than "bytes" — an upstream inconsistency this
# reproduces rather than corrects.
run_case seq20.txt -l 1k
run_case seq20.txt -l 1K
run_case seq20.txt -b 1Z9
run_case seq20.txt -C 1q

# --- suffix length ------------------------------------------------------------

run_case seq20.txt -l 5 -a 1
run_case seq20.txt -l 5 -a 3
run_case seq20.txt -l 5 -a 5
run_case seq20.txt -l 5 --suffix-length=4
# An explicit -a turns widening off, so a run that outgrows the width is an
# error rather than a wider name.
run_case seq20.txt -b 2 -a 1
run_case seq20.txt -l 5 -a 0
run_case seq20.txt -l 5 -a -1
run_case seq20.txt -l 5 -a abc
run_case seq20.txt -l 5 -a 99999999999999999999

# --- suffix alphabets ---------------------------------------------------------

run_case seq20.txt -l 5 -d
run_case seq20.txt -l 5 --numeric-suffixes
run_case seq20.txt -l 5 --numeric-suffixes=0
run_case seq20.txt -l 5 --numeric-suffixes=7
run_case seq20.txt -l 5 -x
run_case seq20.txt -l 5 --hex-suffixes
run_case seq20.txt -l 5 --hex-suffixes=0
run_case seq20.txt -l 5 --hex-suffixes=10
run_case seq20.txt -l 5 --numeric-suffixes=abc
run_case seq20.txt -l 5 --numeric-suffixes=98
# The start value is checked character by character against the alphabet, so a
# sign is out and so is a digit the base does not have. An *empty* value
# passes that check vacuously and behaves like a bare -d with widening off.
run_case seq20.txt -l 5 --numeric-suffixes=-1
run_case seq20.txt -l 5 --numeric-suffixes=1a
run_case seq20.txt -l 5 --numeric-suffixes=
run_case seq20.txt -l 5 --hex-suffixes=
run_case seq20.txt -l 5 --hex-suffixes=1g
# Leading zeros are stripped before the value is measured, so this fits a
# width of one where the same number written plainly would not.
run_case seq20.txt -l 5 --numeric-suffixes=007 -a 1
run_case seq20.txt -l 5 --numeric-suffixes=07
run_case seq20.txt -n 3 --numeric-suffixes=0007
run_case seq20.txt -l 5 -d -a 1
run_case seq20.txt -l 5 --numeric-suffixes=8 -a 1
# `-n` pre-sizes the width to the digits the count needs, rather than widening
# as it goes.
run_case seq20.txt -n 3 -d
run_case seq20.txt -n 200 -d
run_case seq20.txt -n 200 -x
# A start value is folded into the width `-n` needs only when it is smaller
# than the count; a larger one is ignored, so this is an error rather than a
# silently wider field. The pair either side of the count is the whole test.
run_case seq20.txt -n 3 --numeric-suffixes=999
run_case seq20.txt -n 200 --numeric-suffixes=900
run_case seq20.txt -n 200 --numeric-suffixes=100
run_case seq20.txt -n 200 --numeric-suffixes=199
run_case seq20.txt -n 200 --numeric-suffixes=201
run_case seq20.txt -n 20 --numeric-suffixes=5
run_case seq20.txt -n 20 --numeric-suffixes=5 -a 4

# --- suffix widening ----------------------------------------------------------

# 700 pieces, past every widening point. Names only; a one-byte piece's
# contents are not in doubt.
names_case wide.txt -b 1
names_case wide.txt -b 1 -d
names_case wide.txt -b 1 -x
names_case wide.txt -b 1 -a 4
names_case wide.txt -b 1 --numeric-suffixes=0

# --- additional suffix --------------------------------------------------------

run_case seq20.txt -l 5 --additional-suffix=.txt
run_case seq20.txt -l 5 --additional-suffix=
run_case seq20.txt -l 5 --additional-suffix=a/b
run_case seq20.txt -n 3 --additional-suffix=.dat -d

# --- separator ----------------------------------------------------------------

run_case colons.txt -l 2 -t :
run_case colons.txt -C 4 -t :
run_case colons.txt -n r/2 -t :
run_case colons.txt -n l/2 -t :
run_case colons.txt --separator=: -l 3
run_case seq20.txt -l 3 -t ''
run_case seq20.txt -l 3 -t ab
# The same separator twice is allowed; two different ones are not.
run_case seq20.txt -l 3 -t : -t :
run_case seq20.txt -l 3 -t : -t ,

# --- prefix -------------------------------------------------------------------

run_case seq20.txt -l 5 part.
run_case seq20.txt -l 5 ''
run_case seq20.txt -n 3 out-

# --- verbose and unbuffered ---------------------------------------------------

run_case seq20.txt -l 5 --verbose
run_case seq20.txt -n 3 --verbose
run_case seq20.txt -n 3 -e --verbose
run_case seq20.txt -l 5 -u
run_case seq20.txt -l 5 --unbuffered

# --- stdin --------------------------------------------------------------------

stdin_case seq20.txt -l 5
stdin_case seq20.txt -b 20
stdin_case seq20.txt -C 10
stdin_case seq20.txt -l 5 -
raw_case - -l 5

# --- operand errors -----------------------------------------------------------

raw_case - -l 5 nosuch
run_case seq20.txt -l 5 pre extra
run_case seq20.txt -l 3 -b 3
run_case seq20.txt -l 3 -l 4
run_case seq20.txt -n 3 -b 3
run_case seq20.txt -C 3 -n 2
run_case seq20.txt -Z
run_case seq20.txt --nosuchopt
run_case seq20.txt --s
run_case seq20.txt -l
run_case seq20.txt --lines

# --- filter -------------------------------------------------------------------

run_case seq20.txt -l 5 --filter='cat > $FILE'
run_case seq20.txt -b 20 --filter='wc -c'
run_case seq20.txt -l 5 --filter='cat > $FILE.z'
run_case seq20.txt -n 3 --filter='cat > $FILE'
run_case seq20.txt -n 2/3 --filter=cat
run_case seq20.txt -l 5 --filter='exit 3'

# --- not implemented ----------------------------------------------------------

# The first two are about identifying ourselves rather than about behaviour,
# so neither will ever become a pass.
xfail_case 'our --help omits the GNU project ancillary block' seq20.txt --help
xfail_case 'our --version names SlateOS' seq20.txt --version

# `--hex-suffixes=FROM` where FROM contains a letter is a genuine
# out-of-bounds read in GNU 9.4: `next_file_name` builds the suffix index with
# `sufindex[i] = numeric_suffix_start[i] - '0'`, which is the *decimal* value
# of the character, so 'a'..'f' index 49..55 of a 16-character alphabet. The
# names it then produces are wrong and not even in order —
# `--hex-suffixes=ff` gives `xff xf3 xf4`, and `=a` gives `x0a x0e x10`. Ours
# counts in base 16 and gives `xff` (then exhausted) and `x0a x0b x0c`.
# Reproducing a buffer overrun is not compatibility, so these stay divergent.
# See known-issues.md -> TD-SPLIT-GNU-HEX-SUFFIX-START-READS-OUT-OF-BOUNDS.
xfail_case 'GNU reads out of bounds for a hex start value' seq20.txt -l 5 --hex-suffixes=ff
xfail_case 'GNU reads out of bounds for a hex start value' seq20.txt -l 5 --hex-suffixes=a
xfail_case 'GNU reads out of bounds for a hex start value' seq20.txt -n 3 --hex-suffixes=1f

if [ "$HAVE_GNU" = yes ]; then
  printf '\n%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
  [ "$xpass" -gt 0 ] && printf ', %d NO LONGER differ (update the harness)' "$xpass"
  printf '\n'
fi
[ "$fail" -eq 0 ] || exit 1
