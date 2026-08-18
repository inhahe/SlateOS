#!/usr/bin/env bash
# Differential test: our expr against the host's GNU expr.
#
# Each case is `run_case ARGS...`. Both exprs get identical argv — there is no
# stdin to give them — and stdout and the exit status are compared byte for
# byte. This is the check a unit test cannot make, because a unit test asserts
# what we believe expr does rather than what expr does, and expr's corners are
# where belief and reality part company: `expr '' '|' ''` prints `0`, not an
# empty line, and `expr +0 '|' x` prints `+0` while `expr -0 '|' x` prints `x`.
#
# `xfail_case REASON ARGS...` is a case where we differ deliberately and the
# reason is written down — see the head of `expr.rs` for the list. The script
# fails if a plain case differs, and also if an xfail *stops* differing, because
# that means the recorded reason no longer describes reality.
#
# stderr only has to agree about *whether* there was a diagnostic. Matching
# GNU's wording exactly would be fitting to glibc's regcomp, not to expr.
#
# GNU expr is run in a UTF-8 locale on purpose. `length`, `substr`, `index` and
# the count `:` returns are character operations there and byte operations in
# the C locale; this system is UTF-8 throughout, so the UTF-8 answers are the
# ones we are claiming to match. Running the comparison in the C locale would
# be comparing against an artifact of the development host's environment.
set -u

# Our expr is a native Windows binary, so MSYS would helpfully rewrite an
# argument that looks like a path — turning the pattern `.*/\(.*\)` into
# something with a drive letter in it.
export MSYS2_ARG_CONV_EXCL='*'

OURS=${OURS:-"target/x86_64-pc-windows-gnu/debug/expr.exe"}
GNU=${GNU:-expr}
export LC_ALL=${LC_ALL:-C.UTF-8}

pass=0; fail=0; xfail=0; xpass=0

compare() {
  local o_out g_out o_err g_err o_rc g_rc
  o_err=$(mktemp); g_err=$(mktemp)
  o_out=$("$OURS" "$@" 2>"$o_err"); o_rc=$?
  g_out=$("$GNU" "$@" 2>"$g_err"); g_rc=$?

  local o_loud=no g_loud=no
  [ -s "$o_err" ] && o_loud=yes
  [ -s "$g_err" ] && g_loud=yes

  if [ "$o_out" = "$g_out" ] && [ "$o_rc" = "$g_rc" ] && [ "$o_loud" = "$g_loud" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  ours (rc=%s): %s  {%s}\n  gnu  (rc=%s): %s  {%s}' \
    "$o_rc" "$(printf '%s' "$o_out" | tr '\n' '|')" "$(tr '\n' '|' <"$o_err")" \
    "$g_rc" "$(printf '%s' "$g_out" | tr '\n' '|')" "$(tr '\n' '|' <"$g_err")")
  rm -f "$o_err" "$g_err"
}

run_case() {
  compare "$@"
  if [ "$AGREED" = yes ]; then
    pass=$((pass+1))
    [ -n "${VERBOSE:-}" ] && printf 'OK   expr %s\n' "$*"
  else
    fail=$((fail+1))
    printf 'DIFF expr %s\n%s\n' "$*" "$REPORT"
  fi
  return 0
}

xfail_case() {
  local reason="$1"; shift
  compare "$@"
  if [ "$AGREED" = no ]; then
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'XFAIL expr %s  (%s)\n' "$*" "$reason"
  else
    xpass=$((xpass+1))
    printf 'XPASS expr %s\n  now agrees with GNU, so this reason is stale: %s\n' "$*" "$reason"
  fi
  return 0
}

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
run_case foo + 1
run_case '' + 1
run_case ' 10 ' + 1
run_case +1 + 1
run_case 1 - -
run_case 1 / 0
run_case 5 % 0
run_case 1.5 + 1

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
run_case length héllo
run_case substr héllo 2 2
run_case index héllo é
run_case index héllo l
run_case héllo : '.é'
run_case héllo : '\(.é\)'
run_case héllo : '.*'

# --- syntax errors ----------------------------------------------------------
run_case ''
run_case 1 +
run_case '(' 1
run_case '(' 1 ']'
run_case '(' 1 ')' ')'
run_case 1 2
run_case ')'
run_case length
run_case length ')'
run_case substr abc 1
run_case match abc
run_case index abc
run_case abc :
run_case : abc
run_case -
run_case - 1
run_case substr abc 1 1 extra
run_case '(' 42 ')'
run_case hello
run_case 42

# --- backreferences ---------------------------------------------------------
run_case abc : '\(a\)\1'
run_case aab : '\(a\)\1'
run_case abcabcx : '\(abc\)\1'
run_case aa : '\(a*\)\1'
run_case a2 : '\(a\)\2'

# --- where we differ on purpose ---------------------------------------------
xfail_case 'a stacked quantifier is refused; GNU folds a** to a*' \
  abc : 'a**'

printf '\n%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
if [ "$xpass" -gt 0 ]; then
  printf ' (%d of which no longer do)' "$xpass"
fi
printf '\n'
# An xpass is not a failure — agreeing with GNU is never worse — but it does
# mean a recorded decision has gone stale, so it must not pass silently.
[ "$fail" -eq 0 ] && [ "$xpass" -eq 0 ]
