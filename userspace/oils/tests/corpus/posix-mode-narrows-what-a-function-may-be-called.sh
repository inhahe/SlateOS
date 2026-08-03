# bash's grammar takes *any* word before the `()`, and outside posix mode almost
# any of them may become a function: `a-b`, `a.b`, `1f`, `@` and `%f` are all
# real function names. Only a name the parser could see was not written as a
# bare word — a quoted or expanded one — is refused, and that refusal is mild:
# the definition is skipped, the complaint quotes the word back exactly as typed,
# and the script carries on with status 1.
#
# Posix mode replaces that with POSIX's own two rules, and makes them fatal:
#
#   * the name must be a plain identifier, and
#   * it may not be one of the sixteen special builtins.
#
# The identifier test runs first, which is why `:` and `.` — special builtins
# that are also not identifiers — are reported as bad *names*, and `source` is
# the only spelling of the dot builtin that reaches the second message.
#
# Breaking either rule ends a non-interactive shell at once with status 2, and
# nothing spares it: not `!`, not an `if` condition, not an ERR trap. A subshell
# contains it like any other exit, and an EXIT trap still runs — which is what
# lets this script test the rule at all: every case below is a subshell, and the
# `rc=` after it is the status that subshell died with.

echo "=== outside posix mode nearly anything is a function name"
for n in 'a-b' 'a.b' '1f' '@' '%f' 'a[1]' 'f?' 'a/b'; do
  ( eval "$n() { echo BODY; }" && echo "  $n: defined" ) || echo "  $n: refused"
done
echo "--- a quoted or expanded name is refused, mildly"
( eval '"f"() { :; }'; echo "  rc=$? and still here" )
( v=g; eval '$v() { :; }'; echo "  rc=$? and still here" )
echo "--- and the special builtins are fair game"
( eval 'unset() { echo FN; }'; v=1; unset v; echo "  v=${v-gone}" )

echo "=== in posix mode the same names are fatal"
for n in 'a-b' 'a.b' '1f' '@' '%f' 'a[1]' 'f?' 'a/b' '"f"'; do
  ( set -o posix; eval "$n() { :; }"; echo "  UNREACHED" )
  echo "  $n -> rc=$?"
done

echo "=== …and so is every special builtin's name"
for n in ':' '.' source break continue eval exec exit export readonly return set shift times trap unset; do
  ( set -o posix; eval "$n() { :; }"; echo "  UNREACHED" )
  echo "  $n -> rc=$?"
done

echo "=== but a plain identifier, and a non-special builtin, are fine"
( set -o posix; _f() { echo "  OK"; }; _f; F_1x() { echo "  OK2"; }; F_1x )
( set -o posix; cd() { echo "  FN"; }; cd /nowhere; echo "  rc=$?" )
( set -o posix; local() { echo "  FN"; }; local; declare() { echo "  FN2"; }; declare )

echo "=== the function keyword spelling asks the same question"
( set -o posix; function set { :; }; echo "  UNREACHED" ); echo "  rc=$?"
( set -o posix; function a-b { :; }; echo "  UNREACHED" ); echo "  rc=$?"
( function a-b { echo "  OK"; }; a-b )

echo "=== nothing spares the abort"
( set -o posix; ! a-b() { :; }; echo "  UNREACHED" ); echo "  ! -> rc=$?"
( set -o posix; if a-b() { :; }; then echo T; fi; echo "  UNREACHED" ); echo "  if -> rc=$?"
( set -o posix; { a-b() { :; }; } || true; echo "  UNREACHED" ); echo "  || -> rc=$?"
( set -o posix; trap 'echo "  ERR-TRAP"' ERR; a-b() { :; }; echo "  UNREACHED" ); echo "  ERR -> rc=$?"
( set -o posix; trap 'echo "  EXIT-TRAP"' EXIT; a-b() { :; }; echo "  UNREACHED" ); echo "  EXIT -> rc=$?"
( set -o posix; f() { a-b() { :; }; }; f; echo "  UNREACHED" ); echo "  in fn -> rc=$?"
( set -o posix; eval 'a-b() { :; }'; echo "  UNREACHED" ); echo "  eval -> rc=$?"

echo "=== POSIXLY_CORRECT is the same switch"
( POSIXLY_CORRECT=1; a-b() { :; }; echo "  UNREACHED" ); echo "  rc=$?"
# …and leaving the mode gives the names back.
( POSIXLY_CORRECT=1; unset POSIXLY_CORRECT; a-b() { echo "  OK"; }; a-b )

echo "=== a name made before the mode was entered survives the switch"
( unset() { echo FN; }; set -o posix; echo "  still defined: $(declare -F unset)" )
