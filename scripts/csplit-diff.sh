#!/usr/bin/env bash
# Differential test: our csplit against GNU csplit.
#
# ## Why this harness compares files, and not just stdout
#
# Every other coreutils harness here compares stdout, stderr and the exit
# status, because for those tools stdout *is* the output. `csplit`'s stdout is
# only a list of byte counts; its actual product is a set of files left behind
# in the working directory. A harness that compared stdout alone would pass a
# `csplit` that printed the right numbers and then wrote the wrong bytes into
# the wrong filenames — which is most of what there is to get wrong here. So
# each case runs both implementations in their own directory and compares a
# manifest: every file matching the output prefix, its name, and its contents
# byte for byte through `od -An -c`.
#
# The two runs cannot share a directory. Both write `xx00`, and whichever ran
# second would win; the comparison would then be a file against itself and
# would agree unconditionally.
#
# ## Why the reference is glibc, and only glibc
#
# The host's `csplit` is MSYS2's, a Cygwin derivative linking `msys-2.0.dll`
# rather than glibc, and its `getopt` words every option diagnostic
# differently. See `known-issues.md` →
# `TD-COREUTILS-GETOPT-DIAGNOSTICS-USE-THE-WRONG-SHAPE`, and the identical
# note at the top of `od-diff.sh`, `tr-diff.sh` and `wc-diff.sh`.
#
# ## Why LC_ALL=C.UTF-8
#
# The diagnostics quote the offending pattern back at the caller through gnulib's
# `quote()`, which picks its quote marks from the locale. Since §351 ours
# prints U+2018/U+2019 in every locale, and GNU prints those under a UTF-8
# locale and ASCII under `C` — so `C.UTF-8` is the setting the two agree in.
# This file ran under `C` for the mirror-image of that reason until B-Q2 was
# answered; nothing else here reads the locale.
set -u

# Our csplit is a native Windows binary, so MSYS would rewrite an argument that
# looks like a path — and `/REGEX/`, `%REGEX%` and `{*}` all can.
export MSYS2_ARG_CONV_EXCL='*'

OURS=${OURS:-"target/x86_64-pc-windows-gnu/debug/csplit.exe"}
GNU=${GNU:-"wsl -e env LC_ALL=C.UTF-8 csplit"}
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
if wsl -e env LC_ALL=C.UTF-8 csplit -s -f .p .probe 1 >/dev/null 2>&1 && [ -f .p00 ]; then
  HAVE_GNU=yes
else
  HAVE_GNU=no
  echo "csplit-diff: glibc csplit not reachable in this directory (tried: $GNU); skipping"
fi
rm -f .probe .p00 .p01

# --- fixtures -----------------------------------------------------------------

# 1..20, one per line. Short enough to reason about by hand, long enough that a
# repeat count has somewhere to go, and it crosses the 1->2 digit boundary at
# line 10 so a byte count is not simply the line count doubled.
seq 1 20 > seq20.txt

# Repeated matches for the regex forms, at known distances.
printf 'a\nMARK\nb\nc\nMARK\nd\n' > marks.txt

# No trailing newline on the final line: csplit must still account for it, and
# the byte counts differ from the newline-terminated case by exactly one.
printf 'x\ny\nz' > nonl.txt

# Empty input. Every pattern is out of range against it.
: > empty.txt

# --- machinery ----------------------------------------------------------------

run_ours() { ( cd "$1" && "$OURS_ABS" "${@:2}" ); }
run_gnu()  { ( cd "$1" && wsl -e env LC_ALL=C.UTF-8 csplit "${@:2}" ); }

# A manifest of everything the run left behind: each output file, in name
# order, with its contents. `od -An -c` so that a stray CR or a missing final
# newline shows up rather than being invisible in a diff of rendered text.
#
# Iterated with a glob rather than `for f in $(ls | sort)`, which was the
# original and is quietly wrong: an unquoted command substitution is
# word-split on IFS, so a name containing a space arrives in pieces and a name
# ending in one loses it entirely. The manifest then reads a file that does not
# exist, prints empty contents, and the case is scored as a divergence that
# belongs to the harness. `-b '%-5d'` -- whose names end in four spaces -- is
# what exposed it. A glob passes each name through whole and is already sorted.
manifest() {
  local dir=$1 f
  ( cd "$dir" || return 0
    for f in *; do
      # `*` with no matches expands to itself; and `in.txt` is the input we
      # copied in, not something the run produced.
      case $f in '*') [ -e "$f" ] || continue ;; in.txt) continue ;; esac
      printf '  %s: %s\n' "$f" "$(od -An -c < "$f" | tr -s ' \n' ' ')"
    done )
}

# $1 = fixture file (or `-` for none), rest = the whole argv, `in.txt` included.
#
# The argv is passed whole rather than assembled here because a few cases have
# to put something *before* the file operand — `csplit nosuch 2` names a file
# that does not exist, which no amount of options after `in.txt` can express.
compare_argv() {
  local fixture=$1; shift
  local o_out g_out o_err g_err o_rc g_rc o_man g_man
  rm -rf o g; mkdir -p o g
  if [ "$fixture" != - ]; then
    cp "$fixture" o/in.txt; cp "$fixture" g/in.txt
  fi

  o_err=$(mktemp); g_err=$(mktemp)
  local o_bin g_bin; o_bin=$(mktemp); g_bin=$(mktemp)
  # stdout through a file, not a pipe: in `x=$(csplit | od)` the recorded
  # status is od's, and PIPESTATUS is set in the substitution's subshell where
  # it cannot be read. Same note as cat-diff.sh and comm-diff.sh.
  run_ours o "$@" >"$o_bin" 2>"$o_err"; o_rc=$?
  run_gnu  g "$@" >"$g_bin" 2>"$g_err"; g_rc=$?

  o_out=$(od -An -c <"$o_bin"); g_out=$(od -An -c <"$g_bin")
  rm -f "$o_bin" "$g_bin"
  local o_msg g_msg
  o_msg=$(cat "$o_err"); g_msg=$(cat "$g_err")
  rm -f "$o_err" "$g_err"

  o_man=$(manifest o); g_man=$(manifest g)

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

# The common shape: the fixture is the input file, and the arguments follow it.
run_case() {
  [ "$HAVE_GNU" = yes ] || return 0
  local fixture=$1; shift
  compare_argv "$fixture" in.txt "$@"
  report "csplit $fixture $*"
}

# The uncommon shape: the whole argv, for the cases that need something before
# the file operand or no file operand at all.
raw_case() {
  [ "$HAVE_GNU" = yes ] || return 0
  local fixture=$1; shift
  compare_argv "$fixture" "$@"
  report "csplit $*"
}

# A case we expect to differ, with the reason. Counted separately so that a
# case that starts agreeing is reported too — an xfail that silently becomes
# correct is a stale note in the harness.
xfail_case() {
  [ "$HAVE_GNU" = yes ] || return 0
  local why="$1" fixture=$2; shift 2
  compare_argv "$fixture" in.txt "$@"
  if [ "$AGREED" = yes ]; then
    xpass=$((xpass+1)); printf 'XPASS csplit %s %s  (expected to differ: %s)\n' "$fixture" "$*" "$why"
  else
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'XFAIL csplit %s %s  (%s)\n' "$fixture" "$*" "$why"
  fi
  return 0
}

# --- line-number patterns -----------------------------------------------------

run_case seq20.txt 4
run_case seq20.txt 1
run_case seq20.txt 20
run_case seq20.txt 2 4 8
# Splitting at the last line, and one past it. `21` is the case GNU's own
# source singles out — "ensure that the line number specified is not 1 greater
# than the number of lines in the file". Every line is written and counted
# first, and *then* it is an error, because the section the split was supposed
# to begin has no lines to begin with. A boundary check that rejected 21 before
# writing would get the exit status right and the byte counts wrong.
run_case seq20.txt 21
run_case seq20.txt 20 21

# A repeated line number advances by its own literal N, so `4 {1}` splits at 4
# and 8 — not at 4 and then 4 again (already passed), and not at the delta from
# the preceding pattern either, which `2 4 {1}` distinguishes: it splits at 8,
# not at 6.
run_case seq20.txt 4 '{1}'
run_case seq20.txt 4 '{2}'
run_case seq20.txt 2 4 '{1}'

# `{*}` on a line number runs out of range and is an *error*, unlike `{*}` on a
# regex, which stops at end of input silently. Both are exercised so that a
# change collapsing the two shows up. `3 {*}` divides 20 lines into seven
# pieces exactly, so the failing repetition is the one whose piece was written
# in full — it names repetition 6, not 7, which is the off-by-one that a
# "detect the empty section on the next pass" implementation gets wrong.
run_case seq20.txt 3 '{*}'
run_case seq20.txt 5 '{*}'

# --- regex patterns -----------------------------------------------------------

run_case seq20.txt '/5/'
run_case marks.txt '/MARK/'
run_case marks.txt '/MARK/' '{*}'
run_case seq20.txt '/5/' '{*}'
run_case marks.txt '/MARK/+1'
run_case marks.txt '/MARK/-1'
# `/^5$/` and `/5/` must not be the same pattern: the first matches only the
# line that is exactly "5", the second also matches 15. This is the case that
# fails outright if the regex is implemented as a substring search.
run_case seq20.txt '/^5$/'
run_case seq20.txt '/1[0-9]/'
# An offset landing exactly at end of input, and a line-number pattern behind
# it with nothing left to read: the "input disappeared" exit, which is the one
# fatal path that leaves its output file behind even without -k.
run_case seq20.txt '/20/+1'
run_case seq20.txt '/20/+1' 1
# Two patterns where the second matches a line the first already passed. The
# search cursor sits one line *after* the match, not at the start of the new
# section, so `2 /2/` finds nothing rather than matching "2" again.
run_case seq20.txt 2 '/2/'

# --- skip patterns ------------------------------------------------------------

# `%REGEX%` discards the lines before the match and *keeps* the matching line
# as the first line of the next section. Discarding the match too is the
# single-line error this case exists to catch.
run_case seq20.txt '%5%'
run_case marks.txt '%MARK%'
run_case marks.txt '%MARK%' '/MARK/'
# A skip pattern with `{*}` that runs out of matches ends the whole run from
# inside the loop, with status 0 and *no* trailing section — so this leaves no
# output file at all, where `/MARK/ {*}` leaves several.
run_case marks.txt '%MARK%' '{*}'
run_case seq20.txt '%nomatch%'

# --- errors -------------------------------------------------------------------

run_case seq20.txt '/nomatch/'
run_case seq20.txt 99
run_case seq20.txt 4 2
run_case seq20.txt 4 4
run_case empty.txt 1
run_case seq20.txt '{3}'
run_case seq20.txt 'notapattern'
run_case seq20.txt 0

# Malformed patterns. Each of these is a different branch of the argument
# parser and each has its own wording; GNU quotes some of them and not others,
# and `{x}` is quoted after the brace has already been consumed, so the message
# reads `'{x'}` — an artefact worth reproducing exactly, because getting it
# "right" would be a divergence.
run_case seq20.txt '/5/xyz'
run_case seq20.txt '/5'
run_case seq20.txt '%5'
run_case seq20.txt '{x}'
run_case seq20.txt 4 '{1'
run_case seq20.txt '/5/+x'

# Option arguments that are not numbers, and a suffix format that is not a
# single integer conversion.
run_case seq20.txt -n abc 4
run_case seq20.txt -b '%s' 4
run_case seq20.txt -b 'nopercent' 4
run_case seq20.txt -b '%d%d' 4

# The diagnostic for a bad conversion specifier has two branches -- name the
# character, or print one octal escape -- and `%s` above only ever reaches the
# first. These reach the second, which is the half where a byte-wise rule and a
# character-wise one give different answers.
#
# This is the one diagnostic left in the tree that is deliberately *byte*-wise
# after the §357 rewrite, and it is byte-wise because GNU is: `csplit.c` tests
# `isprint` on the single byte `*p`, so under `C.UTF-8` a `%é` suffix reports
# `\303` -- its lead byte -- and stops there. Our awk went the other way for
# its syntax errors, because it counts characters everywhere else and would
# otherwise contradict itself; csplit has no such obligation, so matching GNU
# is worth more. Measuring both branches is what keeps that a decision rather
# than an accident.
run_case seq20.txt -b '%é' 4
run_case seq20.txt -b "%$(printf '\001')" 4
# A space is printable to both rules and takes the character branch: the
# boundary case showing the test is `isprint` and not `isgraph`. It is also
# how the flag-set bug below was found, since the space has to reach the
# conversion slot at all before it can be printed from there.
run_case seq20.txt -b '% ' 4

# The flag set, which is *not* printf's. GNU takes `-`, `0`, `#` and `'` and
# stops at anything else — so `+` and a space are read as the conversion
# specifier and rejected. We accepted both and happily wrote files named `+0`
# and `+1`. Every one of these was measured against GNU 9.4 before being
# written down; see `known-issues.md`
# → BUG-CSPLIT-ACCEPTS-TWO-SUFFIX-FLAGS-GNU-REJECTS.
run_case seq20.txt -b '%+d' 4
run_case seq20.txt -b '% 5d' 4
# The flags GNU does take, so the fix is pinned from both sides: a narrower
# parser that also dropped these would pass the two cases above and still be
# wrong.
run_case seq20.txt -b '%05d' 4
run_case seq20.txt -b '%#o' 4
run_case seq20.txt -b "%'d" 4
#
# `-b '%-5d'` is deliberately absent, and this is a property of the *host*
# rather than of csplit. Left-justifying in width 5 names the files `xx0` and
# `xx1` followed by four spaces, and **Win32 silently strips trailing spaces
# from every path component**. Our csplit is a native Windows binary, so it
# asks for `xx0    ` and the filesystem gives it `xx0`; GNU's runs under WSL,
# whose Linux VFS keeps the spaces. The case therefore reports a divergence
# that neither implementation has. Measured rather than reasoned: a plain
# `python -c "open('zz1    ','w')"` in the same directory also produces `zz1`.
#
# `%-5d` is not left untested — `csplit.rs`'s
# `the_suffix_flag_set_is_gnus_and_not_printfs` asserts in-process that it
# renders `xx3    `, spaces included, which is the half a harness on this host
# cannot ask. `%05d` above keeps a *width* in the harness, since zero padding
# is leading and so survives the trip.
#
# Note this is a Windows-host artifact only: SlateOS allows every byte but `/`
# and NUL in a name, so a trailing space is an ordinary character there and
# the case would be honest if the harness were ever run natively.
#
# There is deliberately no `%<0xFF>` case, and it is worth saying why so that
# nobody adds one: a byte that is not valid UTF-8 cannot survive the trip. An
# argument reaches both sides through a Windows command line, which is UTF-16,
# so MSYS and WSL each widen `0xFF` to U+00FF and hand it over as `ÿ` — the two
# bytes `\303\277`. Measured: `-b "%$(printf '\xff')"` makes GNU report `\303`,
# exactly what `%é` reports, because that is literally what it received. The
# case would pass while testing something other than its name, which is worse
# than not having it. A control byte is unaffected (`%\001` arrives as itself,
# also measured), and the undecodable-byte question is covered where it can be
# asked honestly — in-process, by `quote.rs`'s own tests.

# An input file that is not there. This one needs the whole argv, because the
# point is the operand *before* where the options go.
raw_case - nosuch 2

# Errors with -k, which keeps the files that were already written rather than
# unlinking them. The manifest is the whole point of the case: the byte counts
# on stdout are identical with and without -k.
run_case seq20.txt -k '/nomatch/'
run_case seq20.txt -k 99

# --- naming options -----------------------------------------------------------

run_case seq20.txt -f part 4
run_case seq20.txt -n 4 4
run_case seq20.txt -f part -n 1 4
run_case seq20.txt -s 4
run_case seq20.txt -q 4
run_case seq20.txt --prefix=part 4
run_case seq20.txt --digits=4 4
run_case seq20.txt --quiet 4
run_case seq20.txt --silent 4
run_case seq20.txt --keep-files '/nomatch/'
run_case seq20.txt -b '%03d' 4
run_case seq20.txt -b 'q%dz' 4
run_case seq20.txt -b '%x' 4 8 12
run_case seq20.txt --suffix-format='%02u' 4
# `-b` overrides `-n` entirely rather than combining with it.
run_case seq20.txt -n 6 -b '%d' 4

# --- elision and suppression --------------------------------------------------

# `-z` on a split that produces no empty piece proves nothing; `-z 1` is the
# case that does, because splitting at line 1 makes the first section empty.
# The index is *not* consumed by an elided file, so what is left is a single
# `xx00` holding everything.
run_case seq20.txt -z 1
run_case seq20.txt -z 4
run_case seq20.txt -z 1 '{*}'
run_case marks.txt -z '/MARK/'

# `--suppress-matched` drops the line the split landed on. Measured to apply to
# line numbers too, not only to the pattern kind whose name it carries.
run_case marks.txt '/MARK/' --suppress-matched
run_case seq20.txt --suppress-matched 4
run_case marks.txt --suppress-matched '/MARK/' '{*}'

# --- edge cases in the input --------------------------------------------------

run_case nonl.txt 2
run_case nonl.txt '/y/'
run_case nonl.txt 3
run_case nonl.txt '/z/'
run_case empty.txt '/x/'
run_case empty.txt '%x%'

# --- not implemented ----------------------------------------------------------

# The two remaining deliberate divergences. Both are about identifying
# ourselves rather than about behaviour, so neither will ever become a pass.
xfail_case 'our --help omits the GNU project ancillary block' seq20.txt --help
xfail_case 'our --version names SlateOS' seq20.txt --version

if [ "$HAVE_GNU" = yes ]; then
  printf '\n%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
  [ "$xpass" -gt 0 ] && printf ', %d NO LONGER differ (update the harness)' "$xpass"
  printf '\n'
fi
[ "$fail" -eq 0 ] || exit 1
