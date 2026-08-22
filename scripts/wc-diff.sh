#!/usr/bin/env bash
# Differential test: our wc against GNU wc.
#
# `wc`'s output is five numbers whose *alignment* is part of the answer, so
# stdout is compared byte for byte — the column width is measured behaviour
# (see `number_width` in wc.rs), and a comparison that trimmed whitespace would
# be blind to precisely the thing that was hardest to get right.
#
# ## Why the reference is glibc, and only glibc
#
# The other `*-diff.sh` harnesses default to the host's `wc`, which on this
# machine is MSYS2's. That is a Cygwin derivative: it links `msys-2.0.dll`
# rather than glibc, so its `getopt`, its `iswprint` and its `wcwidth` are all
# its own. For `cat` that only mattered in the option-diagnostics section and
# the fix was to reference glibc there (see `cat-diff.sh`). For `wc` it matters
# *everywhere*: `-m` asks which bytes decode, `-w` asks which characters are
# blank and `-L` asks how many columns each one occupies, and all three are
# locale-and-libc questions. Referencing MSYS2 here would certify answers no
# GNU/Linux system gives, so this harness reaches for glibc through WSL for
# every case and skips loudly if it is not there.
#
# The locale is `C.UTF-8`, which is settled policy rather than a convenience:
# the SlateOS target has no non-UTF-8 locale (design-decisions.md, "osh's
# string layer is UTF-8, full stop"), so `C` would be measuring a
# configuration this OS cannot be in.
set -u

# Our wc is a native Windows binary, so MSYS would rewrite an argument that
# looks like a path.
export MSYS2_ARG_CONV_EXCL='*'

OURS=${OURS:-"target/x86_64-pc-windows-gnu/debug/wc.exe"}
GNU=${GNU:-"wsl -e env LC_ALL=C.UTF-8 wc"}
export LC_ALL=${LC_ALL:-C.UTF-8}

pass=0; fail=0; xfail=0; xpass=0

fixtures=$(mktemp -d)
trap 'rm -rf "$fixtures"' EXIT
cd "$fixtures" >/dev/null || exit 1
OURS_ABS=$OURS
case $OURS in /*|[A-Za-z]:*) ;; *) OURS_ABS="$OLDPWD/$OURS" ;; esac

# WSL is invoked with the Windows cwd, which for an MSYS temp directory lands
# on the same bytes under `/mnt/c/...`. Verified rather than assumed, because a
# reference that silently ran somewhere else would report every file operand as
# missing and still "agree" on the ones fed through stdin.
if [ "$($GNU </dev/null 2>/dev/null | tr -d ' ')" = "000" ] && \
   printf 'probe\n' > .probe && \
   [ "$($GNU .probe 2>/dev/null | awk '{print $1, $2, $3}')" = "1 1 6" ]; then
  HAVE_GNU=yes
else
  HAVE_GNU=no
  echo "wc-diff: glibc wc not reachable in this directory (tried: $GNU); skipping"
fi
rm -f .probe

printf 'alpha beta\ngamma\n'                    > plain.txt      # 2 12 17
printf 'one two three'                          > unterminated.txt
printf ''                                       > empty.txt
printf '\n\n\n'                                 > blanks.txt
printf 'a\tb\tc\n'                              > tabs.txt
printf 'ab\rcdefg\nabcd\x0cef\nab\x0bcd\n'      > controls.txt
printf 'caf\xc3\xa9 na\xc3\xafve\n'             > utf8.txt
printf '\xe4\xb8\xad\xe6\x96\x87 text\n'        > wide.txt
printf 'a\xffb\n'                               > badbyte.txt
printf 'a\xc2\xa0b\n'                           > nbsp.txt
printf 'x\xe2\x80\xa8y\n'                       > lsep.txt
printf 'a\xc2\xadb\n'                           > softhyphen.txt
printf 'a\xe2\x80\x8bb\n'                       > zwsp.txt
# Long enough that the column width is 2 rather than 1 — the sum of the operand
# sizes is what sets it, so this file exists to make that observable.
printf '%0.sz' $(seq 1 40) > forty.txt
printf 'q\n'                                    > w1.txt

compare() {
  local o_out g_out o_err g_err o_rc g_rc stdin=$1; shift
  o_err=$(mktemp); g_err=$(mktemp)
  # stdout through a file, not a pipe: in `x=$(wc | od)` the recorded status is
  # od's, and `PIPESTATUS` is set in the substitution's subshell where it cannot
  # be read. See the same note in cat-diff.sh.
  local o_bin g_bin; o_bin=$(mktemp); g_bin=$(mktemp)
  if [ "$stdin" = "-" ]; then
    "$OURS_ABS" "$@" </dev/null >"$o_bin" 2>"$o_err"; o_rc=$?
    $GNU "$@" </dev/null >"$g_bin" 2>"$g_err"; g_rc=$?
  else
    printf '%b' "$stdin" | "$OURS_ABS" "$@" >"$o_bin" 2>"$o_err"; o_rc=$?
    printf '%b' "$stdin" | $GNU "$@" >"$g_bin" 2>"$g_err"; g_rc=$?
  fi
  o_out=$(od -An -c <"$o_bin"); g_out=$(od -An -c <"$g_bin")
  rm -f "$o_bin" "$g_bin"

  # stderr is compared as *text*, not merely for emptiness. It used to be the
  # latter, and what that cost showed up on 2026-08-21: our `wc` reached for
  # gnulib's `quote()` where upstream reaches for `quoteaf` in `cannot open %s
  # for reading`, and it printed the host's errno string where the rest of our
  # coreutils prints `errmsg::strerror`'s. Two wrong diagnostics, and this
  # harness called both of them a pass. A test that only asks *whether* a
  # program complained certifies the wording as unmeasured while looking like
  # coverage.
  local o_err_text g_err_text
  o_err_text=$(od -An -c <"$o_err"); g_err_text=$(od -An -c <"$g_err")

  if [ "$o_out" = "$g_out" ] && [ "$o_rc" = "$g_rc" ] &&
     [ "$o_err_text" = "$g_err_text" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  ours (rc=%s): %s  {%s}\n  gnu  (rc=%s): %s  {%s}' \
    "$o_rc" "$(printf '%s' "$o_out" | tr -s ' \n' ' ')" "$(tr '\n' '|' <"$o_err")" \
    "$g_rc" "$(printf '%s' "$g_out" | tr -s ' \n' ' ')" "$(tr '\n' '|' <"$g_err")")
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

run_case() { [ "$HAVE_GNU" = yes ] || return 0; compare - "$@"; report "wc $*"; }
run_stdin() {
  [ "$HAVE_GNU" = yes ] || return 0
  local input="$1"; shift
  compare "$input" "$@"
  report "printf '$input' | wc $*"
}

xfail_case() {
  [ "$HAVE_GNU" = yes ] || return 0
  local reason="$1"; shift
  compare - "$@"
  if [ "$AGREED" = no ]; then
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'XFAIL wc %s  (%s)\n' "$*" "$reason"
  else
    xpass=$((xpass+1))
    printf 'XPASS wc %s\n  now agrees with GNU, so this reason is stale: %s\n' "$*" "$reason"
  fi
  return 0
}

# --- the default three counts -----------------------------------------------
run_case plain.txt
run_case empty.txt
run_case blanks.txt
run_case unterminated.txt
run_case tabs.txt
run_stdin 'a b\nc\n'
run_stdin ''
run_stdin 'no newline'

# --- selecting counts, and the fixed order they print in ---------------------
run_case -l plain.txt
run_case -w plain.txt
run_case -c plain.txt
run_case -m utf8.txt
run_case -L plain.txt
run_case -lw plain.txt
run_case -wl plain.txt          # the order given must not change the order printed
run_case -Lc plain.txt
run_case -cL plain.txt
run_case -lwmcL utf8.txt
run_case -Lcmwl utf8.txt
run_case --lines plain.txt
run_case --words plain.txt
run_case --chars utf8.txt
run_case --bytes utf8.txt
run_case --max-line-length plain.txt
# Abbreviations, every one of which the hand-written parser refused.
run_case --lin plain.txt
run_case --w plain.txt
run_case --max plain.txt
run_case --b plain.txt
run_case --c utf8.txt

# --- a character is not a byte, and a column is neither ----------------------
run_case -c utf8.txt
run_case -m utf8.txt
run_case -L utf8.txt
run_case -mcL wide.txt          # U+4E2D is one character, three bytes, two columns
run_case -mcL badbyte.txt       # a byte that decodes to nothing is counted by -c only
run_case -w badbyte.txt
run_case -wL nbsp.txt           # U+00A0 separates words and still takes a column
run_case -wL lsep.txt           # U+2028 has a wcwidth but is not printable
run_case -wL softhyphen.txt     # U+00AD is printable to glibc, so it is a word char
run_case -wL zwsp.txt           # U+200B is zero columns
run_stdin '\xe4\xb8\xad\xe6\x96\x87\n' -L
run_stdin '\xc3\xa9\n' -mc

# --- the four different things a control character does to a line ------------
# CR and FF end the line for -L; VT does not; TAB advances to the next multiple
# of eight. All four separate words.
run_case -L controls.txt
run_case -w controls.txt
run_case -L tabs.txt
run_stdin 'ab\rcdefg\n' -L
run_stdin 'abcd\x0cef\n' -L
run_stdin 'ab\x0bcd\n' -Lw
run_stdin 'abc\tz\n' -L
run_stdin '\t\n' -L
run_stdin 'a\tb\tc\td\n' -L

# --- the column width, which is measured and not a fixed 7 -------------------
# The width is the digit count of the *sum of the operands' sizes*, so all of
# these differ from each other and from the stdin case.
run_case w1.txt
run_case forty.txt
run_case w1.txt forty.txt
run_case plain.txt plain.txt plain.txt
run_case -l w1.txt
run_case -L forty.txt
# `-` is standard input, whose size is not known ahead of reading it, so the
# whole row falls back to 7 — even alongside operands that do have a size.
run_stdin 'a\n' -
run_stdin 'a\n' - w1.txt
run_stdin 'a\n' w1.txt -
# A directory has no length to sum either.
mkdir -p adir
run_case adir
run_case adir w1.txt
# An operand that cannot be stat'ed contributes nothing and is not a reason to
# widen: this must pad to 1, exactly as `wc w1.txt` does.
run_case nosuchfile w1.txt
run_case w1.txt nosuchfile
# One count of one input is a lone number, printed with no padding at all —
# which overrides even the directory that would otherwise force 7.
run_case -l plain.txt
run_case -L forty.txt
run_case -l adir
run_case -c forty.txt
run_stdin 'a\n' -l
run_stdin 'a\n' -L
# …and the exemption is for *one* count and *one* input, so none of these get it.
run_case -l plain.txt w1.txt
run_case -lw plain.txt
run_stdin 'a\n' -lw

# --- the total line ----------------------------------------------------------
run_case plain.txt w1.txt
run_case -L plain.txt wide.txt      # the total's -L is the longest, not the sum
run_case --total=always w1.txt
run_case --total=never plain.txt w1.txt
run_case --total=only plain.txt w1.txt
run_case --total=only -L plain.txt wide.txt
run_case --total=auto w1.txt
run_case --total=a1ways w1.txt
# argmatch abbreviates its argument words too. `a` is ambiguous between `auto`
# and `always`; `al` is not, and resolves.
run_case --total=o plain.txt w1.txt
run_case --total=nev plain.txt w1.txt
run_case --total=al w1.txt

# --- --files0-from -----------------------------------------------------------
printf 'plain.txt\0w1.txt\0' > list0
printf 'plain.txt\0' > one0
printf 'adir\0' > dir0
: > empty0
run_case --files0-from=list0
run_case --files0-from=one0
run_case --files0-from=empty0
run_case -L --files0-from=list0
run_case --files0-from=nosuchlist
run_case --total=always --files0-from=empty0
run_case --files0-from=dir0
# A list on a pipe cannot be read in advance, so the width drops to 1 — the one
# case where `--files0-from=-` and `--files0-from=FILE` differ in output.
run_stdin 'plain.txt\x00w1.txt\x00' --files0-from=-
run_stdin 'plain.txt\x00\x00w1.txt\x00' --files0-from=-
run_stdin 'plain.txt' --files0-from=-
run_stdin '' --files0-from=-
run_stdin 'plain.txt\x00w1.txt\x00' -L --files0-from=-

# --- failure, and the exit status that reports it ---------------------------
run_case nosuchfile
run_case nosuchfile plain.txt
run_case plain.txt nosuchfile
run_case -- plain.txt
run_case -- -l

# --- option diagnostics, compared word for word ------------------------------
#
# The cases above compare only *whether* there was a diagnostic, which is right
# for the ones whose text comes from the host's error table. These compare the
# text, which for options comes from glibc's getopt and from `argmatch` — and is
# the half that a Cygwin reference would have certified wrongly. See
# `known-issues.md` -> TD-COREUTILS-GETOPT-DIAGNOSTICS-USE-THE-WRONG-SHAPE.
getopt_compare() {
  local o_err g_err o_out g_out o_rc g_rc
  o_err=$(mktemp); g_err=$(mktemp)
  o_out=$("$OURS_ABS" "$@" </dev/null 2>"$o_err"); o_rc=$?
  g_out=$($GNU "$@" </dev/null 2>"$g_err"); g_rc=$?
  local o_msg g_msg
  o_msg=$(tr '\n' '|' <"$o_err"); g_msg=$(tr '\n' '|' <"$g_err")
  rm -f "$o_err" "$g_err"
  if [ "$o_msg" = "$g_msg" ] && [ "$o_rc" = "$g_rc" ] && [ "$o_out" = "$g_out" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  ours (rc=%s): %s\n  gnu  (rc=%s): %s' "$o_rc" "$o_msg" "$g_rc" "$g_msg")
}

run_getopt() {
  [ "$HAVE_GNU" = yes ] || return 0
  getopt_compare "$@"
  if [ "$AGREED" = yes ]; then
    pass=$((pass+1))
    [ -n "${VERBOSE:-}" ] && printf 'OK   wc %s\n' "$*"
  else
    fail=$((fail+1))
    printf 'DIFF wc %s [stderr]\n%s\n' "$*" "$REPORT"
  fi
  return 0
}

# Currently called nowhere: the last three xfails, the U+2018/U+2019 quotation
# marks, became passes when `design-decisions.md` §351 was implemented. Kept
# anyway, and deliberately. It is not dead weight but the mechanism that made
# that cleanup happen on time: an xfail that starts agreeing reports XPASS with
# its own now-stale reason quoted back, so a divergence we stop having cannot
# sit here being asserted forever. The next genuine one wants this, and
# rewriting it under deadline is how a harness ends up without the XPASS half.
xfail_getopt() {
  [ "$HAVE_GNU" = yes ] || return 0
  local reason="$1"; shift
  getopt_compare "$@"
  if [ "$AGREED" = no ]; then
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'XFAIL wc %s  (%s)\n' "$*" "$reason"
  else
    xpass=$((xpass+1))
    printf 'XPASS wc %s\n  now agrees with GNU, so this reason is stale: %s\n' "$*" "$reason"
  fi
  return 0
}

# The status is 1, not sort's 2 — it is per-utility, and 1 is the common case.
run_getopt -x
run_getopt -Z
run_getopt --nope
run_getopt --nope=1

# An option that takes an argument, missing one — and one that does not, given
# one. The two sentences are different, and so is which of the two names glibc
# puts first.
run_getopt --total
run_getopt --files0-from
run_getopt --lines=1
run_getopt --max-line-length=2
run_getopt --help=x
run_getopt --version=x

# `argmatch` is a different sentence from getopt's, with a different status:
# `--total=zzz` exits 1 even for a utility whose bad-option status is 2.
#
# These three used to differ, in the *quotation marks* and nothing else, and
# were carried here as `quote-marks-under-a-utf8-locale` xfails. They pass now.
# GNU's `quote()` — the style used for option arguments, and only that style —
# asks the locale's charset and emits U+2018/U+2019 when it is UTF-8, so under
# this harness's `C.UTF-8` the reference prints `invalid argument ‘zzz’`; we
# printed `'zzz'`. `design-decisions.md` §351 settled that we follow GNU, on
# the grounds that SlateOS has no non-UTF-8 locale to have the other behaviour
# for, and `quote()` now emits the curly marks too. `quotef` and `quoteaf`,
# which render *file names*, were correct throughout and are untouched — they
# keep the straight marks in every locale, which is GNU's behaviour and is what
# keeps a file name pasteable back into a shell.
run_getopt --total=zzz
run_getopt --total=
run_getopt --total=a

# wc's own usage error. It is the one that keeps the `Try 'wc --help'` referral,
# because upstream calls `error (0, ...)` and then `usage (EXIT_FAILURE)` rather
# than `error (EXIT_FAILURE, ...)` — which is what `Program::usage_referring`
# exists to say.
run_getopt --files0-from=list0 plain.txt
run_getopt --files0-from=- plain.txt

# The empty prefix matches every option, so this one case pins the whole
# declaration order of upstream's `struct option[]` — which is observable, and
# which was measured with precisely this command rather than recalled.
run_getopt --=x

printf '\n%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
if [ "$xpass" -gt 0 ]; then
  printf ' (%d of which no longer do)' "$xpass"
fi
printf '\n'
# An xpass is not a failure — agreeing with GNU is never worse — but it does
# mean a recorded decision has gone stale, so it must not pass silently.
[ "$fail" -eq 0 ] && [ "$xpass" -eq 0 ]
