#!/usr/bin/env bash
# Differential test: our expr against GNU expr, both run inside WSL.
#
# expr has no stdin. Every case is argv and nothing else, which makes this the
# simplest harness in the tree and also the one whose corners are least
# guessable: `expr '' '|' ''` prints `0` rather than an empty line, `expr +0 '|'
# x` prints `+0` while `expr -0 '|' x` prints `x`, and `expr abc : 'a**'` is
# accepted by GNU and refused by us. A unit test asserts what we believe expr
# does; this asserts what expr does.
#
# ## What changed when this moved onto diff-wsl.sh
#
# It used to run on Windows against MSYS2's expr, and had three defects that
# each hid divergences rather than reporting them.
#
# **The reference was not GNU's.** MSYS2 is a Cygwin derivative; its coreutils
# are built against Cygwin's libc and its regex is not glibc's. A harness that
# compares against it certifies wording no GNU/Linux system prints.
#
# **stdout was captured with `$(...)`,** which strips every trailing newline and
# cannot hold a NUL. GNU expr terminates its one line of output with `\n`; a
# build of ours that stopped doing so would have passed this harness silently.
# stdout now goes to a file and is compared as a hex dump.
#
# **stderr was compared for presence only** — whether there was a diagnostic,
# never what it said. That is the right rule for exactly two of expr's messages
# and the wrong rule for all the rest, so the two rules are now separated; see
# below. It was hiding a real divergence: GNU quotes the offending argument
# with directional quotes (`syntax error: unexpected argument ‘2’`) in a UTF-8
# locale and with `'...'` under `C`, and nothing here had ever looked.
#
# It also needed `MSYS2_ARG_CONV_EXCL='*'` so that MSYS would not rewrite the
# pattern `.*/\(.*\)` into something with a drive letter in it. Inside WSL an
# argument is a byte string and arrives as written, which is also what makes the
# undecodable-byte cases at the end possible at all — they could not be written
# before, because MSYS could not carry the byte.
#
# ## Two verdicts per case
#
# | helper | stderr compared as |
# |---|---|
# | `run_case ARGS...` | presence — was there a diagnostic at all |
# | `msg_case ARGS...` | text — byte for byte |
#
# `msg_case` is the default for expr's own diagnostics, because they are plain
# reports about the command line (`syntax error: unexpected argument ‘2’`,
# `division by zero`, `non-integer argument`) and a script that greps expr's
# stderr should not have to know which expr it got.
#
# `run_case` is for the two that come out of the regex engine — `Unmatched ( or
# \(`, `Invalid content of \{\}` — which are glibc's `regcomp` strings rendered
# by glibc's `regerror`. Matching those exactly would be fitting our engine to
# glibc's internal error taxonomy rather than to expr, so we agree only about
# *whether* the pattern was rejected.
#
# `xfail_case` / `xmsg_case` take a reason first, for a divergence we chose. The
# script fails if a plain case differs, and also if an xfail stops differing,
# because then the recorded reason no longer describes reality.
set -u

DIFF_PROG=expr
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; xfail=0; xpass=0

# One entry on `PATH`, which is safe here in a way it would not be for awk or
# sed: expr runs no subprocess, so there is nothing else for it to need to find.
# The single entry is the guarantee that the `expr` being run is the symlink and
# not whatever else is installed.
run_side() {
  local side=$1 out=$2 err=$3; shift 3
  env PATH="$bindir/$side" expr "$@" >"$out" 2>"$err"
}

compare() {
  local o_out g_out o_msg g_msg o_bin g_bin o_err g_err o_rc g_rc
  o_err=$(mktemp); g_err=$(mktemp); o_bin=$(mktemp); g_bin=$(mktemp)
  # stdout to a file rather than through a pipe into `od`: in `x=$(expr | od)`
  # the recorded status is od's, and `PIPESTATUS` is set inside the command
  # substitution's subshell where it cannot be read — so every failing case
  # would compare od's success against od's success and pass.
  run_side ours "$o_bin" "$o_err" "$@"; o_rc=$?
  run_side gnu  "$g_bin" "$g_err" "$@"; g_rc=$?

  # A hex dump, so that the trailing newline and any byte that is not valid
  # UTF-8 are part of the comparison rather than lost on the way into a shell
  # variable.
  o_out=$(od -An -tx1 <"$o_bin"); g_out=$(od -An -tx1 <"$g_bin")
  o_msg=$(cat "$o_err"); g_msg=$(cat "$g_err")

  local o_loud=no g_loud=no
  [ -s "$o_err" ] && o_loud=yes
  [ -s "$g_err" ] && g_loud=yes
  rm -f "$o_bin" "$g_bin" "$o_err" "$g_err"

  local same_out=no
  [ "$o_out" = "$g_out" ] && [ "$o_rc" = "$g_rc" ] && same_out=yes

  AGREED=no; AGREED_MSG=no
  [ "$same_out" = yes ] && [ "$o_loud" = "$g_loud" ] && AGREED=yes
  [ "$same_out" = yes ] && [ "$o_msg" = "$g_msg" ] && AGREED_MSG=yes

  REPORT=$(printf '  ours (rc=%s): %s  {%s}\n  gnu  (rc=%s): %s  {%s}' \
    "$o_rc" "$(printf '%s' "$o_out" | tr -s ' \n' ' ')" "$(printf '%s' "$o_msg" | tr '\n' '|')" \
    "$g_rc" "$(printf '%s' "$g_out" | tr -s ' \n' ' ')" "$(printf '%s' "$g_msg" | tr '\n' '|')")
}

report() {
  local agreed=$1 label=$2
  if [ "$agreed" = yes ]; then
    pass=$((pass+1))
    [ -n "${VERBOSE:-}" ] && printf 'OK   %s\n' "$label"
  else
    fail=$((fail+1))
    printf 'DIFF %s\n%s\n' "$label" "$REPORT"
  fi
  return 0
}

report_x() {
  local agreed=$1 reason=$2 label=$3
  if [ "$agreed" = no ]; then
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'XFAIL %s  (%s)\n' "$label" "$reason"
  else
    xpass=$((xpass+1))
    printf 'XPASS %s\n  now agrees with GNU, so this reason is stale: %s\n' "$label" "$reason"
  fi
  return 0
}

run_case()  { compare "$@"; report   "$AGREED"     "expr $*"; }
msg_case()  { compare "$@"; report   "$AGREED_MSG" "expr $*"; }

xfail_case() { local r="$1"; shift; compare "$@"; report_x "$AGREED"     "$r" "expr $*"; }
xmsg_case()  { local r="$1"; shift; compare "$@"; report_x "$AGREED_MSG" "$r" "expr $*"; }

printf 'expr-diff:\n  ours: %s\n  gnu:  %s\n\n' "$OURS" "$gnu_real"

# --- arithmetic -------------------------------------------------------------
run_case 2 + 3
run_case 10 - 4
run_case 3 '*' 4
run_case 7 / 2
run_case 7 % 3
run_case -7 / 2
run_case -7 % 2
run_case 7 / -2
run_case -7 % -2
run_case 2 + 3 '*' 4
run_case '(' 2 + 3 ')' '*' 4
run_case 10 - 3 - 2
run_case 3 - 10
run_case 1 + 2 + 3
run_case 5 - -3
run_case 010 + 1
run_case -0 + 0
run_case 0 '*' 0

# The reason expr needs a bignum: GNU is arbitrary-precision, and a script
# reaches these sizes by multiplying two byte counts.
run_case 9223372036854775807 + 1
run_case 99999999999999999999 '*' 99999999999999999999
run_case 123456789012345678901234567890 / 987654321
run_case 123456789012345678901234567890 % 987654321
run_case 2 '*' 170141183460469231731687303715884105728

# --- arithmetic on things that are not numbers ------------------------------
# The wording is compared: `non-integer argument` is a plain report about the
# command line, not a rendering of anything's internals.
msg_case foo + 1
msg_case '' + 1
msg_case ' 10 ' + 1
msg_case +1 + 1
msg_case 1 - -
msg_case 1 / 0
msg_case 5 % 0
msg_case 1.5 + 1

# --- comparison -------------------------------------------------------------
run_case 5 = 5
run_case 5 = 6
run_case 5 '!=' 6
run_case 3 '<' 5
run_case 3 '<' 20
run_case 2 '>' 10
run_case 5 '<=' 5
run_case 3 '>=' 3
run_case 1 '==' 1
run_case apple '<' banana
run_case banana '<' apple
run_case foo = foo
run_case abc '>' abd
run_case +1 '<' 2
run_case 1 = +1
run_case 1 = 1 = 1
run_case 1 + 1 = 2
run_case 010 = 10

# --- logic ------------------------------------------------------------------
run_case hello '|' world
run_case 0 '|' world
run_case '' '|' world
run_case '' '|' ''
run_case 0 '|' 00
run_case -0 '|' x
run_case +0 '|' x
run_case ' 0' '|' x
run_case 0.0 '|' x
run_case - '|' x
run_case foo '&' bar
run_case x '&' y '&' z
run_case 0 '&' bar
run_case foo '&' ''
run_case 0 '&' 0
run_case 1 '<' 1 '|' 2

# --- the colon operator -----------------------------------------------------
run_case abc : 'a*'
run_case abc : 'b*'
run_case abc : '[a-c]*'
run_case abc : 'abcd'
run_case abc : ''
run_case '' : ''
run_case aab : 'a\{2\}'
run_case abc : 'a\{2\}'
run_case abc : '^abc'
run_case abc : 'abc$'
run_case abc : 'c$'
run_case abc : '.'
run_case abc : '.*'
run_case 1 : 1
run_case abc : 'a\|b'
run_case abc : 'a\+'
run_case abc : 'a\?'
run_case abc : '*'
run_case '*abc' : '*'
run_case abc : '[^x]*'
run_case a.c : 'a\.c'
run_case abc : 'a.c'

# ...with a group, which changes what comes back entirely
run_case abc : '\(b*\)'
run_case abc : 'a\(b\)'
run_case abcabc : '.*\(b\)'
run_case abc : '\(a\)\(b\)'
run_case abc : '^\(a\)'
run_case /usr/lib/libc.so : '.*/\(.*\)'
run_case v1.24.3 : 'v\([0-9]*\)'
run_case v1.24.3 : 'v\([0-9]*\)\.\([0-9]*\)'
run_case abc : '\(x\)*a'

# Patterns the engine rejects. Presence only: the text is glibc's `regcomp`
# error taxonomy rendered by `regerror`, and matching it would be fitting our
# engine to glibc's internals rather than to expr.
run_case abc : 'a\('
run_case abc : 'a\{3,1\}'
run_case abc : '[a-'
run_case abc : 'a\{1'

# --- match, substr, index, length -------------------------------------------
run_case match abc a
run_case match abc b
run_case match abc '\(b\)'
run_case substr abcdef 2 3
run_case substr abcdef 1 6
run_case substr abcdef 2 100
run_case substr abcdef 0 3
run_case substr abcdef 9 3
run_case substr abcdef 2 -1
run_case substr abcdef 2 0
run_case substr abcdef a 3
run_case substr abcdef 2 x
run_case substr abcdef 99999999999999999999 3
run_case index abcdef cd
run_case index abcdef fc
run_case index abcdef z
run_case index abcdef f
run_case index '' a
run_case length abcdef
run_case length ''
run_case length 12345

# --- precedence between the levels ------------------------------------------
run_case 2 '*' 3 : 3
run_case match abc a : x
run_case abc : a : b
run_case length '(' abc ')'
run_case 1 '<' 2 '&' 3 '<' 4

# --- the quote operator -----------------------------------------------------
run_case + length
run_case + match
run_case + +
run_case + ')'
run_case + :
run_case 1 + + 2
run_case + 1 + 2

# --- text that is not ASCII -------------------------------------------------
# Character operations under `C.UTF-8`, which is what this system is. Under `C`
# they are byte operations, and a harness that ran there would be certifying an
# artifact of the development host's environment rather than expr's behaviour.
run_case length héllo
run_case substr héllo 2 2
run_case index héllo é
run_case index héllo l
run_case héllo : '.é'
run_case héllo : '\(.é\)'
run_case héllo : '.*'

# ...and text that is not text. A byte that is not valid UTF-8 is data: it has
# to survive `substr` and be counted by `length` without being replaced by
# U+FFFD, which is the silent corruption `from_utf8_lossy` performs and this
# project forbids outright (CLAUDE.md's self-review item 7). Measured: GNU
# counts the byte, matches it, and says nothing about it.
#
# These cases could not be written under the old harness at all — MSYS could not
# carry the byte through argv.
raw=$(printf 'a\xffb')
run_case length "$raw"
run_case index "$raw" b
run_case substr "$raw" 2 1
run_case "$raw" '|' x
run_case "$raw" = "$raw"

# The regex half is where GNU stops agreeing with itself, and the two cases
# below are the demonstration. The three cases above establish that GNU calls
# the undecodable byte a character: `length` says 3, `index ... b` says 3, and
# `substr ... 2 1` hands the byte back. But its matcher cannot see past it —
# `.*` matches only the leading `a`, and `a.b` does not match at all. A string
# that is three characters long to `length` and one character long to `.*` is
# not a model a script can be written against.
#
# Ours counts the byte in both halves (design-decisions.md §322). That is also
# the only reading under which `expr "$path" : '.*/\(.*\)'` — one of the oldest
# spellings of `basename` — keeps working on a path this filesystem allows,
# which is every byte but `/` and NUL.
xfail_case 'GNU: length calls the undecodable byte a character but the matcher stops at it; ours is bytes throughout (§322)' \
  "$raw" : '.*'
xfail_case 'GNU: `a.b` cannot match across an undecodable byte its own `length` counts; ours is bytes throughout (§322)' \
  "$raw" : 'a.b'

# --- syntax errors ----------------------------------------------------------
# Text-compared. Every one of these names the offending argument back to the
# user, which is the part a person reads and a script greps.
msg_case ''
msg_case 1 +
msg_case '(' 1
msg_case '(' 1 ']'
msg_case '(' 1 ')' ')'
msg_case 1 2
msg_case ')'
msg_case length
msg_case length ')'
msg_case substr abc 1
msg_case match abc
msg_case index abc
msg_case abc :
msg_case : abc
msg_case -
msg_case - 1
msg_case substr abc 1 1 extra
run_case '(' 42 ')'
run_case hello
run_case 42

# --- the two option-shaped arguments ----------------------------------------
# `--` ends the options, so `expr -- 1 + 2` is arithmetic and not an error.
# `--help` and `--version` are recognised only in the first position; anywhere
# else they are ordinary strings, which is why `expr abc --version` is a syntax
# error about an unexpected argument rather than a version banner.
run_case -- 1 + 2
msg_case abc --version
msg_case 1 + --help

# `--help` is checked for shape rather than text. Its content is not GNU's to
# dictate to us — GNU's ends in translation-project and info-page addresses
# that would be a lie coming from this binary — but the three things that go
# wrong with a `--help` are not about content at all, and all three are checked:
# it must exit 0, it must write to *stdout* and not stderr, and it must say
# something. A `--help` that exits 2, or that a pager cannot be piped, is the
# bug this case exists to catch.
help_shape() {
  local out err rc
  out=$(mktemp); err=$(mktemp)
  run_side ours "$out" "$err" --help; rc=$?
  if [ "$rc" -eq 0 ] && [ -s "$out" ] && [ ! -s "$err" ]; then
    AGREED=yes
  else
    AGREED=no
    REPORT=$(printf '  rc=%s  stdout=%s bytes  stderr=%s bytes' \
      "$rc" "$(wc -c <"$out")" "$(wc -c <"$err")")
  fi
  rm -f "$out" "$err"
  report "$AGREED" 'expr --help (exits 0, writes stdout, says nothing on stderr)'
}
help_shape

# --- backreferences ---------------------------------------------------------
run_case abc : '\(a\)\1'
run_case aab : '\(a\)\1'
run_case abcabcx : '\(abc\)\1'
run_case aa : '\(a*\)\1'
run_case a2 : '\(a\)\2'

# --- two divergences that stopped being divergences --------------------------
# Both were recorded as deliberate and both are now plain cases, caught as XPASS
# the first time this harness ran against real GNU expr instead of MSYS2's.
# Backreferences were the Pike VM's one real limitation and were implemented on
# 2026-08-18 (known-issues.md); the stacked quantifier `a**` is now folded
# exactly as GNU folds it. They are kept as cases precisely because they were
# once wrong.
run_case abc : 'a**'
run_case aab : 'a**'
run_case abcabcx : '\(abc\)\1'

printf '\n%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
if [ "$xpass" -gt 0 ]; then
  printf ' (%d of which no longer do)' "$xpass"
fi
printf '\n'
# An xpass is not a failure — agreeing with GNU is never worse — but it does
# mean a recorded decision has gone stale, so it must not pass silently.
[ "$fail" -eq 0 ] && [ "$xpass" -eq 0 ]
