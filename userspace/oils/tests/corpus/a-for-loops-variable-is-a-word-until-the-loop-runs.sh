# bash's grammar asks only for a WORD where a `for` or `select` control variable
# goes. Nothing checks it there, so `for 'a[0]'` *parses*; it is
# `execute_for_command` that decides the word is not an identifier, complains and
# fails the loop with status 1. The difference from a syntax error is the whole
# point: a syntax error would abandon the rest of the parse unit, and this does
# not — everything after the loop still runs.
#
# The check is on the *spelling*, not on what the word would expand to. `"x"` is
# refused although a bare `x` is a fine name, and `$v` is refused without ever
# looking at `$v`. The word is quoted back exactly as it was written, quotes and
# backslashes intact.
#
# It happens before the loop does anything else: the `in` list is not expanded
# (a command substitution in it never runs), no `set -x` header is printed, and
# the body cannot run because there is nothing to bind.
#
# `for` and `select` agree on all of that and disagree on one thing: in posix
# mode the refusal ends a non-interactive shell — bash wrote that branch into
# `execute_for_command` only — so a posix `select` carries on where a posix
# `for` does not.

echo "### the spelling is what is checked, and what is quoted back"
for 1x in a; do echo NO; done; echo "  rc=$?"
for 'a[0]' in a; do echo NO; done; echo "  rc=$?"
for "a[0]" in a; do echo NO; done; echo "  rc=$?"
for a=b in a; do echo NO; done; echo "  rc=$?"
for a\[0\] in a; do echo NO; done; echo "  rc=$?"
for '' in a; do echo NO; done; echo "  rc=$?"
for 'a b' in a; do echo NO; done; echo "  rc=$?"
for ~ in a; do echo NO; done; echo "  rc=$?"
for a.b in a; do echo NO; done; echo "  rc=$?"

echo "### an expansion is refused unexpanded"
v=good
for $v in a; do echo NO; done; echo "  rc=$?"
for "$v" in a; do echo NO; done; echo "  rc=$?"
for "x" in a; do echo NO; done; echo "  rc=$?"
for \x in a; do echo NO; done; echo "  rc=$?"
echo "  and the bare name still works:"
for $v in a; do :; done 2>/dev/null
for good in one two; do echo "    $good"; done

echo "### the list is not expanded and nothing is traced"
for 'a[0]' in $(echo RAN >&2; echo a); do echo NO; done; echo "  rc=$?"
set -x
for 'a[0]' in a; do echo NO; done
set +x
echo "  rc=$?"

echo "### it is an ordinary failed command"
for 'a[0]' in a; do echo NO; done 2>/dev/null; echo "  quiet rc=$?"
! for 'a[0]' in a; do echo NO; done 2>/dev/null; echo "  negated rc=$?"
if for 'a[0]' in a; do echo NO; done 2>/dev/null; then echo "  then"; else echo "  else"; fi
for 'a[0]' in a; do echo NO; done 2>/dev/null && echo "  and"; echo "  after=$?"

echo "### it arms neither errexit nor the ERR trap"
# …though `$?` is 1 and `&&`/`||`/`if` all saw the failure above. The boundary
# is the enclosing *simple command*: a group, a loop body and a subshell's
# parent leave the exemption standing, but a function call, an `eval` or a `.`
# hands back a status of its own and that is an ordinary failure.
( set -e; for 'a[0]' in a; do echo NO; done; echo "  survived set -e" )
( set -e; { for 'a[0]' in a; do echo NO; done; }; echo "  a group is not a command" )
( set -e; while :; do for 'a[0]' in a; do echo NO; done; break; done; echo "  nor is a loop body" )
( trap 'echo "  ERR TRAP"' ERR; for 'a[0]' in a; do echo NO; done; echo "  no ERR trap either" )
( set -e; select 'a[0]' in a; do echo NO; done; echo "  select too" )
( set -e; f() { for 'a[0]' in a; do echo NO; done; }; f; echo NO-UNREACHABLE )
echo "  through a function: rc=$?"
( set -e; eval 'for "a[0]" in a; do echo NO; done'; echo NO-UNREACHABLE )
echo "  through eval:       rc=$?"
( set -e; ( for 'a[0]' in a; do echo NO; done ); echo NO-UNREACHABLE )
echo "  through a subshell: rc=$?"
( set -e; f() { for 'a[0]' in a; do echo NO; done; }; f || echo "  but || still spares it" )

echo "### with no in-list at all"
set -- x y
for 'a[0]'; do echo NO; done; echo "  rc=$?"

echo "### a reserved word is a fine loop variable"
for do in one two; do echo "  do=$do"; done
for in in one; do echo "  in=$in"; done

echo "### select checks it the same way"
select 1x in a; do echo NO; done; echo "  rc=$?"
select 'a[0]' in $(echo RAN >&2; echo a); do echo NO; done; echo "  rc=$?"
select "a[0]"; do echo NO; done; echo "  rc=$?"

echo "### posix mode: fatal for for, not for select"
( set -o posix; select 'a[0]' in a; do echo NO; done; echo "  select rc=$?" )
( set -o posix; for 'a[0]' in a; do echo NO; done; echo NO-UNREACHABLE ); echo "  for rc=$?"
( set -o posix; trap 'echo "  exit trap ran"' EXIT; for 'a[0]' in a; do echo NO; done )
( set -o posix; ! for 'a[0]' in a; do echo NO; done; echo NO-UNREACHABLE ); echo "  negated rc=$?"
( set -o posix; eval 'for "a[0]" in a; do echo NO; done'; echo NO-UNREACHABLE ); echo "  eval rc=$?"
( set -o posix; if for 'a[0]' in a; do :; done; then :; fi; echo NO-UNREACHABLE ); echo "  if rc=$?"
echo "  still here"

echo "### having parsed, it prints back the way it was written"
f() { for "a[0]" in x y; do :; done; }
g() { select $v in x; do :; done; }
h() { for do in x; do :; done; }
declare -f f g h

echo "### and a bad name still parses, so what follows it runs"
for 'a[0]' in a; do echo NO; done; echo "  the rest of the unit ran"
