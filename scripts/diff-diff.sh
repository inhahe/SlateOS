#!/usr/bin/env bash
# Differential test: our `diff` against GNU diffutils.
#
# ## Why this harness exists
#
# `diff` is one of the sixteen duplicate pairs left under design decision §1005,
# and it is the one that most needs deciding by measurement rather than by
# counting. With `dup-bins-survey.py` reading `coreutils`' shared modules as well
# as its bin -- the fix that moved seventeen other pairs -- `diff` did not move
# at all: `userspace/coreutils/src/bin/diff.rs` is 414 lines and reaches no
# shared module, against the standalone crate's 1541.
#
# The option lists say the same thing more sharply. The `coreutils` half
# implements **three** options: `-q`, `--brief`, `-u`. The standalone mentions
# seventeen, including `-c`, `-r`, `-i`, `-w`, `-b`, `-B`, `-y`, `-N`, `--color`
# and `--width`. So this is the first pair where the standalone is plausibly the
# better program, and §1005 resolves that by PORTING the better half into
# `coreutils` rather than by deleting it -- which makes knowing which half is
# better the whole question.
#
# But "mentions seventeen options" is exactly the inference this tree has now
# been burned by five times. `dd` mentions `bs` and implements none of its
# suffixes. `join` mentions `-a` and rejects `-a1`. An option list is a claim,
# not a measurement, so the standalone gets measured before anything is moved.
#
# ## Why the reference is the installed binary
#
# Unlike every coreutils harness here, this one does not build its reference.
# `DIFF_GNU_SOURCE` fetches a *coreutils* tarball and `diff` is not in it -- it
# is GNU diffutils, a separate project. So the reference is `/usr/bin/diff`.
#
# The §726 caveat therefore applies, and applies less: this host's diffutils is
# `1:3.10-1ubuntu0.1` against its coreutils' `9.4-3ubuntu6.3`. Both are Ubuntu
# revisions and neither is pristine upstream, but one `ubuntu0.1` is a different
# order of divergence from `3ubuntu6.3`, which is what made the coreutils
# harnesses build their own. Recorded rather than waved past: a green run here
# certifies agreement with Ubuntu's diffutils 3.10.
#
# ## Why the fixtures have fixed timestamps
#
# `diff -u` and `diff -c` print each file's mtime in the header:
#
#     --- a.txt   2026-09-11 17:00:00.000000000 +0000
#
# Left alone that makes every run disagree with itself. Every fixture is
# therefore stamped with `touch -d` to one fixed instant, and `TZ` is pinned to
# UTC for both sides, so the header is deterministic and a difference in it is a
# real difference in formatting rather than noise. That is deliberately NOT done
# with `--label`: the standalone may not have it, and a harness that needs an
# option the subject might lack cannot test the subject that lacks it.
#
# ## Why the exit status is compared, not just the output
#
# `diff`'s exit status is part of its contract and is what scripts read: 0 the
# files are the same, 1 they differ, 2 trouble. A `diff` that printed the right
# text and answered 0 for "different" would break every `if diff ...; then` in
# existence while looking perfect in a side-by-side. Several cases here exist
# only to pin the status.
#
# ## Why `od -An -c`
#
# Its output is whitespace: leading `+`/`-`/space columns, tabs inside context
# lines, the exact blank line placement, and the `\ No newline at end of file`
# marker. Any comparison that trimmed or collapsed would agree with an
# implementation that got the column wrong, which is most of what there is to
# get wrong here.
set -u

DIFF_PROG='diff'
# Every invocation below is bounded with it, on both sides. `diff` on two large
# inputs is the classic quadratic-blowup program, so a subject that has stopped
# making progress and one that is merely working hard look identical from
# outside; 30 seconds separates them, since no fixture here is larger than a few
# hundred lines. The reference is wrapped too -- a harness that bounded only our
# side would hang on the day the reference was the buggy one.
DIFF_NEED=timeout
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; xfail=0; xpass=0

fixtures=$DIFF_TMP/fixtures
mkdir -p "$fixtures"
cd "$fixtures" >/dev/null || exit 1

# The one instant every fixture is stamped with. Any fixed value would do; this
# one is old enough never to collide with a file the harness makes later.
STAMP='2020-01-02 03:04:05'

stamp() { touch -d "$STAMP" "$@"; }

# --- fixtures ----------------------------------------------------------------
# The base file everything else is a variation of.
printf 'alpha\nbravo\ncharlie\ndelta\n'          > base.txt
# Identical content in another file: the exit-0, no-output case.
printf 'alpha\nbravo\ncharlie\ndelta\n'          > same.txt
# One line changed in the middle.
printf 'alpha\nbravo\nCHANGED\ndelta\n'          > mid.txt
# One line changed at the very top and at the very bottom, which is where an
# off-by-one in the context window shows.
printf 'CHANGED\nbravo\ncharlie\ndelta\n'        > first.txt
printf 'alpha\nbravo\ncharlie\nCHANGED\n'        > last.txt
# A line added, and a line removed, rather than changed.
printf 'alpha\nbravo\nextra\ncharlie\ndelta\n'   > added.txt
printf 'alpha\ncharlie\ndelta\n'                 > removed.txt
# Every line different, so there is no common subsequence to anchor on.
printf 'one\ntwo\nthree\nfour\n'                 > allnew.txt
# Empty, and a single empty line, which are not the same file.
printf ''                                        > empty.txt
printf '\n'                                      > blankline.txt
# No trailing newline: the `\ No newline at end of file` marker, on one side
# and then on both.
printf 'alpha\nbravo\ncharlie\ndelta'            > nonl.txt
printf 'alpha\nbravo\nCHANGED\ndelta'            > nonl2.txt
# Trailing whitespace and interior whitespace, for -b and -w.
printf 'alpha \nbravo\ncharlie\ndelta\n'         > trailws.txt
printf 'al pha\nbravo\ncharlie\ndelta\n'         > interws.txt
printf 'alpha\t\nbravo\ncharlie\ndelta\n'        > tabws.txt
# Case, for -i.
printf 'ALPHA\nbravo\ncharlie\ndelta\n'          > upper.txt
# Blank lines, for -B.
printf 'alpha\n\nbravo\ncharlie\ndelta\n'        > blanks.txt
# A tab inside a line, which the normal format prints raw and which must not be
# expanded.
printf 'alpha\tX\nbravo\ncharlie\ndelta\n'       > tabbed.txt
# Bytes that are not text. `diff` calls these binary and says so rather than
# printing them, and the sentence it says is part of the contract.
printf 'alpha\n\x80\xff\nbravo\n'                > bytes.txt
printf 'alpha\n\x80\xfe\nbravo\n'                > bytes2.txt
# A NUL, which is what actually triggers the binary heuristic.
printf 'alpha\n\x00nul\nbravo\n'                 > nul.txt
printf 'alpha\n\x00NUL\nbravo\n'                 > nul2.txt
# Long enough that the default three lines of context do not cover the whole
# file, so the hunk headers have to be right.
{ for i in $(seq 1 40); do printf 'line %02d\n' "$i"; done; }            > long.txt
{ for i in $(seq 1 40); do
    if [ "$i" = 20 ]; then printf 'LINE TWENTY\n'; else printf 'line %02d\n' "$i"; fi
  done; }                                                               > long2.txt
# Two changes far apart, so there are two hunks rather than one.
{ for i in $(seq 1 40); do
    case $i in 5|35) printf 'CHANGED %02d\n' "$i" ;; *) printf 'line %02d\n' "$i" ;; esac
  done; }                                                               > long3.txt
# Directories, for -r and for the "is a directory" diagnostics.
mkdir -p da db
printf 'alpha\n'                                 > da/one.txt
printf 'ALPHA\n'                                 > db/one.txt
printf 'only in a\n'                             > da/onlya.txt
printf 'only in b\n'                             > db/onlyb.txt
mkdir -p da/sub db/sub
printf 'nested\n'                                > da/sub/deep.txt
printf 'NESTED\n'                                > db/sub/deep.txt

# Everything gets the same mtime, including the directories, so no header and no
# `-r` listing can differ for a reason that is not the program's.
stamp ./*.txt da/*.txt db/*.txt da/sub/*.txt db/sub/*.txt da/sub db/sub da db .

compare() {
  local o_out g_out o_err g_err o_rc g_rc stdin=$1; shift
  o_err=$(mktemp); g_err=$(mktemp)
  # stdout through a file rather than a pipe: in `x=$(diff | od)` the status
  # recorded is od's, and `diff`'s status is the greater half of its answer.
  local o_bin g_bin; o_bin=$(mktemp); g_bin=$(mktemp)
  if [ "$stdin" = "-" ]; then
    run_side ours "$@" </dev/null >"$o_bin" 2>"$o_err"; o_rc=$?
    run_side gnu  "$@" </dev/null >"$g_bin" 2>"$g_err"; g_rc=$?
  else
    printf '%b' "$stdin" | run_side ours "$@" >"$o_bin" 2>"$o_err"; o_rc=$?
    printf '%b' "$stdin" | run_side gnu  "$@" >"$g_bin" 2>"$g_err"; g_rc=$?
  fi
  o_out=$(od -An -c <"$o_bin"); g_out=$(od -An -c <"$g_bin")
  rm -f "$o_bin" "$g_bin"

  local o_msg g_msg
  o_msg=$(cat "$o_err"); g_msg=$(cat "$g_err")
  rm -f "$o_err" "$g_err"

  if [ "$o_out" = "$g_out" ] && [ "$o_rc" = "$g_rc" ] && [ "$o_msg" = "$g_msg" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  ours (rc=%s): %s  {%s}\n  gnu  (rc=%s): %s  {%s}' \
    "$o_rc" "$(printf '%s' "$o_out" | tr -s ' \n' ' ')" "$(printf '%s' "$o_msg" | tr '\n' '|')" \
    "$g_rc" "$(printf '%s' "$g_out" | tr -s ' \n' ' ')" "$(printf '%s' "$g_msg" | tr '\n' '|')")
}

# One invocation of one side. Each is reached through a symlink named `diff` in
# a directory that is the whole of `PATH` for that invocation, so `argv[0]` is
# the bare word and the `diff: ` prefix on every diagnostic matches. `TZ` is
# pinned so the `-u` and `-c` headers format identically on both sides.
run_side() {
  local side=$1; shift
  diff_run timeout -k 2 30 env TZ=UTC LC_ALL=C.UTF-8 PATH="$bindir/$side" diff "$@"
}

report() {
  local label="$1"; shift
  if [ "$AGREED" = yes ]; then
    pass=$((pass+1))
    [ -n "${VERBOSE:-}" ] && printf 'OK   %s\n' "$label"
  else
    fail=$((fail+1))
    printf 'DIFF %s\n%s\n' "$label" "$REPORT"
  fi
  return 0
}

run_case()  { compare - "$@"; report "diff $*"; }
run_stdin() { local i="$1"; shift; compare "$i" "$@"; report "printf '$i' | diff $*"; }

# A case expected to differ, with the reason. Counted apart so that one which
# starts agreeing is reported too: an xfail that silently becomes correct is a
# stale note in the harness rather than a success.
xfail_case() {
  local why="$1"; shift
  compare - "$@"
  if [ "$AGREED" = yes ]; then
    xpass=$((xpass+1))
    printf 'XPASS %s -- expected to differ (%s) and did not\n' "diff $*" "$why"
  else
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'xfail %s (%s)\n' "diff $*" "$why"
  fi
  return 0
}

# --- the default output format -----------------------------------------------
# Normal format is what `diff` prints with no options at all, and it is the one
# an implementation is most likely to skip in favour of unified.
run_case base.txt same.txt
run_case base.txt mid.txt
run_case base.txt first.txt
run_case base.txt last.txt
run_case base.txt added.txt
run_case base.txt removed.txt
run_case base.txt allnew.txt
run_case base.txt empty.txt
run_case empty.txt base.txt
run_case empty.txt empty.txt
run_case empty.txt blankline.txt
run_case base.txt long.txt

# --- the exit status, which is half the contract -----------------------------
run_case -q base.txt same.txt
run_case -q base.txt mid.txt
run_case --brief base.txt mid.txt
run_case -s base.txt same.txt
run_case --report-identical-files base.txt same.txt
run_case -q -s base.txt same.txt

# --- unified ------------------------------------------------------------------
run_case -u base.txt same.txt
run_case -u base.txt mid.txt
run_case -u base.txt first.txt
run_case -u base.txt last.txt
run_case -u base.txt added.txt
run_case -u base.txt removed.txt
run_case -u base.txt allnew.txt
run_case -u empty.txt base.txt
run_case -u long.txt long2.txt
run_case -u long.txt long3.txt
run_case -u0 long.txt long2.txt
run_case -u1 long.txt long2.txt
run_case -U 5 long.txt long2.txt
run_case -U 0 long.txt long3.txt
run_case --unified long.txt long2.txt
run_case --unified=1 long.txt long2.txt

# --- context ------------------------------------------------------------------
run_case -c base.txt mid.txt
run_case -c base.txt added.txt
run_case -c long.txt long3.txt
run_case -C 1 long.txt long2.txt
run_case --context long.txt long2.txt

# --- the no-newline marker ----------------------------------------------------
# `\ No newline at end of file` is the single most commonly omitted piece of
# diff output, and it changes what `patch` does with the result.
run_case base.txt nonl.txt
run_case nonl.txt base.txt
run_case -u base.txt nonl.txt
run_case -u nonl.txt base.txt
run_case -u nonl.txt nonl2.txt
run_case -c base.txt nonl.txt

# --- whitespace and case ------------------------------------------------------
run_case base.txt trailws.txt
run_case -b base.txt trailws.txt
run_case -w base.txt trailws.txt
run_case -w base.txt interws.txt
run_case -b base.txt interws.txt
run_case -b base.txt tabws.txt
run_case -Z base.txt trailws.txt
run_case --ignore-trailing-space base.txt trailws.txt
run_case --ignore-space-change base.txt interws.txt
run_case --ignore-all-space base.txt interws.txt
run_case base.txt upper.txt
run_case -i base.txt upper.txt
run_case --ignore-case base.txt upper.txt
run_case base.txt blanks.txt
run_case -B base.txt blanks.txt
run_case --ignore-blank-lines base.txt blanks.txt
run_case -u -i base.txt upper.txt
run_case -u -w base.txt interws.txt

# --- tabs, which must survive untouched ---------------------------------------
run_case base.txt tabbed.txt
run_case -u base.txt tabbed.txt
run_case -t base.txt tabbed.txt
run_case -T base.txt tabbed.txt

# --- bytes that are not text --------------------------------------------------
run_case bytes.txt bytes2.txt
run_case -q bytes.txt bytes2.txt
run_case nul.txt nul2.txt
run_case -q nul.txt nul2.txt
run_case -a nul.txt nul2.txt
run_case --text nul.txt nul2.txt
run_case base.txt bytes.txt
run_case -u bytes.txt bytes2.txt

# --- stdin --------------------------------------------------------------------
run_stdin 'alpha\nbravo\ncharlie\ndelta\n' base.txt -
run_stdin 'alpha\nbravo\nCHANGED\ndelta\n' base.txt -
run_stdin 'alpha\nbravo\ncharlie\ndelta\n' - base.txt
run_stdin 'alpha\nbravo\nCHANGED\ndelta\n' -u - base.txt
run_stdin '' - empty.txt

# --- directories ---------------------------------------------------------------
run_case da db
run_case -r da db
run_case --recursive da db
run_case -q -r da db
run_case -N da db
run_case --new-file da db
run_case -r -N da db
run_case da/one.txt db
run_case da db/one.txt
run_case -u da/one.txt db/one.txt

# --- side by side ---------------------------------------------------------------
run_case -y base.txt mid.txt
run_case --side-by-side base.txt mid.txt
run_case -y -W 40 base.txt mid.txt
run_case --width=40 -y base.txt mid.txt
run_case -y --suppress-common-lines base.txt mid.txt

# --- refusals and operand errors ------------------------------------------------
run_case
run_case base.txt
run_case nosuch.txt base.txt
run_case base.txt nosuch.txt
run_case nosuch.txt nosuch2.txt
run_case base.txt same.txt extra.txt
run_case -Q base.txt same.txt
run_case --nosuchoption base.txt same.txt
run_case -U notanumber long.txt long2.txt
run_case -U -1 long.txt long2.txt
run_case . base.txt
run_case base.txt .

# --- the two whose text is ours -------------------------------------------------
xfail_case "our help text, not the GNU project's" --help
xfail_case "our version string, not the GNU project's" --version

printf '\n%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
if [ "$xpass" -gt 0 ]; then
  printf ', %d NO LONGER differ (update the harness)' "$xpass"
fi
printf '\n'
[ -n "${DIFF_SKIPPED:-}" ] && printf 'skipped:%s\n' "$DIFF_SKIPPED"
[ "$fail" = 0 ] && [ "$xpass" = 0 ]
