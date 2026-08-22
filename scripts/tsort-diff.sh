#!/usr/bin/env bash
# Differential test: our tsort against GNU tsort.
#
# ## Why the reference is glibc, and only glibc
#
# The host's `tsort` is MSYS2's — a Cygwin derivative linking `msys-2.0.dll`
# rather than glibc, whose `getopt` words every option diagnostic differently
# (`unknown option -- x` against `invalid option -- 'x'`). A harness pointed at
# it would certify sentences no GNU/Linux system prints. See `known-issues.md`
# → `TD-COREUTILS-GETOPT-DIAGNOSTICS-USE-THE-WRONG-SHAPE`, and the identical
# note at the top of `comm-diff.sh`, `join-diff.sh`, `paste-diff.sh`,
# `fold-diff.sh`, `expand-diff.sh`, `head-diff.sh`, `wc-diff.sh`, `cut-diff.sh`,
# `uniq-diff.sh` and `nl-diff.sh`.
#
# Run `OURS=/usr/bin/tsort ./scripts/tsort-diff.sh` to confirm the harness still
# discriminates: it should report dozens of differences, not zero.
#
# ## Why the whole harness runs under `LC_ALL=C.UTF-8`
#
# Not for the reason `comm-diff.sh` and `join-diff.sh` keep a `C` section.
# `tsort` sorts its items with `strcmp`, never `strcoll`, so the locale changes
# nothing about what it computes and either setting would serve for the ordering
# cases. What decides it is the diagnostics: they pass a file name or an operand
# through gnulib's `quote()`, and since §351 ours prints U+2018/U+2019 in every
# locale — which is what GNU prints under a UTF-8 locale and not what it prints
# under `C`. This file used to run under `C` for the mirror-image of that reason,
# back when ours stayed ASCII (`open-questions.md` → B-Q2, since answered). The
# last section re-runs the ordering cases under `C` anyway, to record that the
# *output* really is locale-independent rather than merely assumed to be.
#
# ## Why `od -An -c`
#
# Item names are byte strings: `tsort` truncates a token at its first NUL, keeps
# `\r`, `\v` and `\f` inside a token, and prints whatever bytes it stored. A
# comparison that trimmed or collapsed whitespace would agree with an
# implementation that split on the wrong character set, which is exactly the bug
# the shipped `tsort` had.
#
# ## Why the order matters as much as the set
#
# Almost any topological sort of `a b / c d` is "correct"; only one of them is
# GNU's. Two rules decide it and both are observable, so most of the rows here
# are about order rather than about validity:
#
#   * ready items enter the queue in **sorted name order**, because upstream
#     walks a `strcmp`-keyed tree in order;
#   * an item's successors are walked **newest relation first**, because
#     `record_relation` prepends to a linked list.
#
# ## Cases that differ on purpose
#
# `--help` and `--version`, whose text is ours rather than the GNU project's,
# and a directory operand, which a Windows host refuses to open at all.
set -u

# Our tsort is a native Windows binary, so MSYS would rewrite an argument that
# looks like a path.
export MSYS2_ARG_CONV_EXCL='*'

OURS=${OURS:-"target/x86_64-pc-windows-gnu/debug/tsort.exe"}
export LC_ALL=${LC_ALL:-C.UTF-8}

pass=0; fail=0; xfail=0; xpass=0

fixtures=$(mktemp -d)
trap 'rm -rf "$fixtures"' EXIT
cd "$fixtures" >/dev/null || exit 1
OURS_ABS=$OURS
case $OURS in /*|[A-Za-z]:*) ;; *) OURS_ABS="$OLDPWD/$OURS" ;; esac

# Every invocation is bounded, on both sides. The loop-breaking pass is the
# specific reason: it repeats a whole tree walk until the backward chain closes,
# and an implementation that failed to clear a link would repeat it forever.
# The *reference* is wrapped too, so a harness that only bounded our side would
# hang on the day the reference was the buggy one.
run_ours() { timeout -k 2 30 "$OURS_ABS" "$@"; }
run_gnu()  { local loc=$1; shift; timeout -k 2 30 wsl -e env "LC_ALL=$loc" tsort "$@"; }

# WSL is invoked with the Windows cwd, which for an MSYS temp directory lands on
# the same bytes under `/mnt/c/...`. Verified rather than assumed, because a
# reference that silently ran somewhere else would report every file operand as
# missing and still "agree" on the ones fed through stdin.
printf 'a b\n' > .probe
if [ "$(run_gnu C .probe 2>/dev/null | tr '\n' '|')" = "a|b|" ]; then
  HAVE_GNU=yes
else
  HAVE_GNU=no
  echo "tsort-diff: glibc tsort not reachable in this directory; skipping"
fi
rm -f .probe

# --- fixtures ----------------------------------------------------------------
# One relation, the smallest thing that is not empty.
printf 'a b\n'                      > one.txt
# Empty, which is not an error: zero items sort to nothing.
printf ''                           > empty.txt
# Whitespace only, which is also zero items — the tokeniser must not invent an
# empty token out of the trailing delimiters.
printf '  \t\n \n'                  > blank.txt
# A chain, whose answer is forced.
printf 'a b\nb c\nc d\nd e\n'       > chain.txt
# A diamond: two ready items at once, so the name order decides.
printf 'a b\na c\nb d\nc d\n'       > diamond.txt
# The successor-order fixture. `a` precedes three items and none of them
# precedes another, so the only thing that can order `b`, `c` and `d` is the
# order the relations were read — reversed.
printf 'a c\na b\na d\n'            > succ.txt
# The same three relations in a different order, so a harness that matched
# `succ.txt` by accident does not match this too.
printf 'a d\na c\na b\n'            > succ2.txt
# A relation from an item to itself, which is dropped rather than made a cycle.
printf 'x x\n'                      > self.txt
# A self relation mixed in with real ones.
printf 'a b\nb b\nb c\n'            > self2.txt
# The same relation twice: the in-degree is 2 and there are two successor
# entries, so an implementation that deduplicated one but not the other leaves
# an item unprintable.
printf 'a b\na b\n'                 > dup.txt
printf 'a b\na b\nb c\nb c\nb c\n'  > dup2.txt
# Disconnected components, which interleave by name rather than concatenating.
printf 'a b\nc d\n'                 > split.txt
printf 'p q\na b\nz y\n'            > split2.txt
# A two-item cycle: the smallest loop.
printf 'a b\nb a\n'                 > cyc2.txt
# A three-item cycle beside an acyclic part, so the acyclic part is printed
# before the loop is even noticed.
printf 'a b\nb c\nc a\nx y\n'       > cyc3.txt
# A cycle written backwards, so the backward walk names it in an order that is
# neither the input's nor sorted.
printf 'b a\nc b\na c\nd e\n'       > cycrev.txt
# Two independent cycles: the outer loop runs twice and reports twice.
printf 'a b\nb a\nc d\nd c\n'       > cyc2x.txt
# A cycle with a tail hanging off it, so breaking the loop releases more items.
printf 'a b\nb c\nc a\nc d\nd e\n'  > cyctail.txt
# A self-referential pair sharing an item with a larger cycle.
printf 'a b\nb c\nc a\nb d\nd b\n'  > cycshare.txt
# An odd token count: the last token has nothing to precede.
printf 'a b c\n'                    > odd.txt
printf 'solo'                       > solo.txt
# No trailing newline at all, so the last token ends at EOF rather than at a
# delimiter.
printf 'a b\nc d'                   > noeol.txt
# `\r` is *not* a delimiter, so this is two tokens — `a\rb` and `x` — not three.
# A line-based reader turns it into three and then complains about an odd count.
printf 'a\rb x\n'                   > cr.txt
printf 'a\rb\nc\rd\n'               > crlf.txt
# Vertical tab and form feed, likewise ordinary bytes inside a token.
printf 'a\vb a\fc\n'                > vtff.txt
# A NUL inside a token, which `xstrdup` truncates: this is `a` before `c`.
printf 'a\0b c\n'                   > nul.txt
# A token that is only a NUL, giving an item with an empty name that still
# sorts first and still gets printed as an empty line.
printf '\0 x\ny \0\n'               > nulonly.txt
# Bytes that are not text, to confirm the sort is over `unsigned char`.
printf '\xff q\n\x01 q\n\x80 q\n'   > high.txt
# Case, which a byte sort orders capitals first and a collating sort does not.
printf 'B q\na q\nA q\nb q\n'       > case.txt
# Names that are prefixes of one another, the tiebreak a comparison written as a
# bare memcmp over the shorter length would get wrong.
printf 'ab z\nabc z\na z\n'         > prefix.txt
# Tabs and runs of blanks as separators rather than single spaces.
printf 'a\tb\n\n\n   c   \t d  \n'  > seps.txt
# Big enough that the queue is refilled many times and the name sort is doing
# real work. A chain plus a fan, so both rules are exercised at size.
{ for i in $(seq 1 400); do printf 'n%03d n%03d\n' "$i" "$((i+1))"; done; } > long.txt
{ for i in $(seq 1 400); do printf 'root k%03d\n' "$i"; done; }             > fan.txt
mkdir subdir

compare() {
  local o_out g_out o_err g_err o_rc g_rc stdin=$1 loc=$2; shift 2
  o_err=$(mktemp); g_err=$(mktemp)
  # stdout through a file, not a pipe: in `x=$(tsort | od)` the recorded status
  # is od's, and `PIPESTATUS` is set in the substitution's subshell where it
  # cannot be read. See the same note in cat-diff.sh.
  local o_bin g_bin; o_bin=$(mktemp); g_bin=$(mktemp)
  if [ "$stdin" = "-" ]; then
    run_ours "$@" </dev/null >"$o_bin" 2>"$o_err"; o_rc=$?
    run_gnu "$loc" "$@" </dev/null >"$g_bin" 2>"$g_err"; g_rc=$?
  else
    printf '%b' "$stdin" | run_ours "$@" >"$o_bin" 2>"$o_err"; o_rc=$?
    printf '%b' "$stdin" | run_gnu "$loc" "$@" >"$g_bin" 2>"$g_err"; g_rc=$?
  fi
  o_out=$(od -An -c <"$o_bin"); g_out=$(od -An -c <"$g_bin")
  rm -f "$o_bin" "$g_bin"

  local o_msg g_msg
  o_msg=$(cat "$o_err"); g_msg=$(cat "$g_err")

  # stderr is compared in full, not merely for emptiness: the whole point of the
  # getopt module is that the sentences match, so a harness that only asked "did
  # it complain?" would pass on every wording this exists to fix. It matters
  # doubly here, because a cycle's *members* are printed on stderr — the list
  # and its order are output, not decoration.
  if [ "$o_out" = "$g_out" ] && [ "$o_rc" = "$g_rc" ] && [ "$o_msg" = "$g_msg" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  ours (rc=%s): %s  {%s}\n  gnu  (rc=%s): %s  {%s}' \
    "$o_rc" "$(printf '%s' "$o_out" | tr -s ' \n' ' ')" "$(printf '%s' "$o_msg" | tr '\n' '|')" \
    "$g_rc" "$(printf '%s' "$g_out" | tr -s ' \n' ' ')" "$(printf '%s' "$g_msg" | tr '\n' '|')")
  rm -f "$o_err" "$g_err"
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

run_case()  { [ "$HAVE_GNU" = yes ] || return 0; compare - C.UTF-8 "$@"; report "tsort $*"; }
# The same case under `C`. Unlike `comm`'s and `join`'s second locale, this is
# not measuring a divergence: it is the evidence that there is none to measure,
# because `tsort` never calls `strcoll`. Only the ordering cases belong here —
# a diagnostic run under `C` would report GNU's ASCII marks against our curly
# ones and fail for a reason that has nothing to do with ordering.
run_c()     { [ "$HAVE_GNU" = yes ] || return 0; compare - C "$@"; report "tsort $* [C]"; }
run_stdin() {
  [ "$HAVE_GNU" = yes ] || return 0
  local input="$1"; shift
  compare "$input" C.UTF-8 "$@"
  report "printf '$input' | tsort $*"
}
# A case we expect to differ, with the reason. Counted separately so that a case
# that starts agreeing is reported too — an xfail that silently becomes correct
# is a stale note in the harness.
xfail_case() {
  [ "$HAVE_GNU" = yes ] || return 0
  local why="$1"; shift
  compare - C.UTF-8 "$@"
  if [ "$AGREED" = yes ]; then
    xpass=$((xpass+1)); printf 'XPASS tsort %s  (expected to differ: %s)\n' "$*" "$why"
  else
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'XFAIL tsort %s  (%s)\n' "$*" "$why"
  fi
  return 0
}

# --- the ordinary answers -----------------------------------------------------
run_case one.txt
run_case empty.txt
run_case blank.txt
run_case chain.txt
run_case diamond.txt
run_case split.txt
run_case split2.txt
run_case self.txt
run_case self2.txt
run_case dup.txt
run_case dup2.txt
run_case prefix.txt
run_case long.txt
run_case fan.txt

# --- the two ordering rules ---------------------------------------------------
# Successors are walked newest-relation-first. The shipped tsort walked them
# oldest-first, so these two are the rows that fail against it while every
# "is it a valid topological order?" check would have passed.
run_case succ.txt
run_case succ2.txt
# Ready items enter the queue in sorted name order, bytewise.
run_case case.txt
run_case high.txt

# --- the delimiters -----------------------------------------------------------
# Space, tab and newline, and nothing else. `\r`, `\v` and `\f` are content.
run_case seps.txt
run_case cr.txt
run_case crlf.txt
run_case vtff.txt
run_case noeol.txt
# A token stops at its first NUL, and a token that is only a NUL is the empty
# name.
run_case nul.txt
run_case nulonly.txt

# --- cycles -------------------------------------------------------------------
# Both the standard output — which still lists every item — and the standard
# error list, whose order is the backward walk's rather than the input's.
run_case cyc2.txt
run_case cyc3.txt
run_case cycrev.txt
run_case cyc2x.txt
run_case cyctail.txt
run_case cycshare.txt

# --- odd token counts ---------------------------------------------------------
run_case odd.txt
run_case solo.txt

# --- standard input -----------------------------------------------------------
# The default operand, the explicit `-`, and the fact that both are named `-` in
# a diagnostic rather than `standard input`.
run_stdin 'a b\n'
run_stdin 'a b\n' -
run_stdin ''
run_stdin '' -
run_stdin 'a c\na b\na d\n'
run_stdin 'a b\nb a\n'
run_stdin 'a b c\n'
run_stdin 'a b c\n' -
run_stdin 'solo'
run_stdin '   \t\n\n  '
run_stdin 'a\rb x\n'
run_stdin 'a\0b c\n'
run_stdin 'a b\nb c\nc a\nx y\n'

# --- operands -----------------------------------------------------------------
run_case one.txt one.txt
run_case one.txt one.txt one.txt
run_case chain.txt diamond.txt
run_case one.txt -
run_case - one.txt
run_case - -
# `--` ends the options, and everything after it is an operand — including a
# spelling that would otherwise be an option.
run_case -- one.txt
run_case --
run_case -- --help
run_case -- -
run_case -- -x
run_case one.txt -- one.txt
run_case -- one.txt -x
run_case one.txt -- --version
# An operand that will not open, and one that opens and will not read.
run_case nosuch.txt
run_case ''
xfail_case 'a directory operand cannot be opened on a Windows host' subdir

# --- getopt -------------------------------------------------------------------
# There are no short options at all, so every short spelling is an error —
# including `-h`, which is the one a reader would guess exists.
run_case -h
run_case -x
run_case -0
run_case -qz
run_case -h one.txt
run_case one.txt -h
run_case --nope
run_case --nope one.txt
run_case --helpp
run_case --=x
run_case --help=x
run_case --hel=x
run_case --version=x
run_case --ver=x
# Options permute past an operand, because gnulib passes an option string with
# no leading `+`.
run_case one.txt --nope
# There is exactly one `getopt_long` call, so whichever option comes first is
# the only one ever looked at. `--version -x` prints the version and `-x
# --version` refuses; an implementation that scanned the whole line would report
# the bad option in both.
run_case -x --nope
run_case -x --version
run_case --nope --help

# --- the locale changes nothing -----------------------------------------------
# `tsort` sorts with `strcmp`, so these must agree byte for byte with their
# `C.UTF-8` counterparts above. A row that differed here would mean GNU had
# grown a collation this file does not model. Only ordering cases: a diagnostic
# would differ under `C` for a reason that is about quote marks, not order.
run_c case.txt
run_c high.txt
run_c prefix.txt
run_c diamond.txt
run_c succ.txt
run_c long.txt

# --- differ on purpose --------------------------------------------------------
xfail_case 'our --help omits the GNU project ancillary block' --help
xfail_case 'our --version names SlateOS' --version
# The abbreviated spellings are here too, and as xfails rather than as ordinary
# cases: they reach the same two texts, so a `run_case` row for them would
# report the *body* difference we already accept and hide the thing they are
# actually for — that `--h` and `--v` resolve at all rather than being rejected
# as unknown. An XPASS on either would mean the resolution broke in the
# direction that makes them print something else entirely.
xfail_case 'an abbreviation of --help reaches our help text' --h
xfail_case 'an abbreviation of --version reaches our version text' --v
xfail_case 'an option still permutes past an operand' one.txt --help
# The single-call rule again, on the two rows whose *body* we already accept as
# different: only the first option is looked at, so `--help --version` reaches
# help and `--version --help` reaches the version. An XPASS on either would mean
# the wrong one won, since the two texts do not otherwise resemble each other.
xfail_case 'the first option wins, and it is --help' --help --version
xfail_case 'the first option wins, and it is --version' --version --help
xfail_case 'a bad option after --version is never looked at' --version -x

if [ "$HAVE_GNU" = yes ]; then
  printf '\n%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
  [ "$xpass" -gt 0 ] && printf ', %d NO LONGER differ (update the harness)' "$xpass"
  printf '\n'
fi
[ "$fail" -eq 0 ] || exit 1
