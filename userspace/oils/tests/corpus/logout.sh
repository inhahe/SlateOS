# `logout` — `exit` with a gate in front of it, and the `login_shell` state that
# gate reads.
#
# A shell is a login shell because of how it was *started*: `-l`/`--login`, or an
# `argv[0]` beginning with `-`, which is how `login(1)` marks it. Nothing a script
# does can change that, so `shopt -s login_shell` is one of the two options bash
# gives a setter that silently succeeds and does nothing. Anywhere else `logout`
# refuses, naming the builtin that would have worked — and it refuses *before*
# looking at its operand, so a bad operand is invisible.
#
# Inside a login shell it is `exit`, which means it shares `exit`'s operand
# reading, and the second half of this case is about the part of that reading
# nothing else reaches: bash's `no_args`, which reports "too many arguments" and
# then unwinds with `top_level_cleanup()` + `jump_to_top_level(DISCARD)`. That is
# not an exit at all. It discards the rest of the current *top-level parse unit*
# and resumes with the next one, and because of the cleanup it escapes an
# enclosing `eval` — so `exit 3 4` on a line with anything after it takes that
# with it, and the following line still runs.

echo "=== a script is not a login shell"
shopt login_shell; echo "  rc=$?"
shopt -p login_shell
case ":$BASHOPTS:" in *:login_shell:*) echo "  in BASHOPTS";; *) echo "  not in BASHOPTS";; esac

echo "=== and cannot become one: the setter succeeds and changes nothing"
shopt -s login_shell; echo "  rc=$?"
shopt login_shell; echo "  rc=$?"
shopt -u login_shell; echo "  rc=$?"

echo "=== so logout refuses, and says what to use instead"
logout; echo "  rc=$?"

echo "=== the refusal comes first: the operand is never read"
logout abc; echo "  rc=$?"
logout 3 4; echo "  rc=$?"

echo "=== it is an ordinary builtin otherwise"
type -t logout
compgen -b | grep -x logout
compgen -A helptopic | grep -x logout
help -s logout
help -d logout

echo "=== -l makes one, and the flag shows up in every place that reports it"
"$BASH" -l -c 'echo "  MARK $(shopt -p login_shell)"' | grep '^  MARK'
"$BASH" --login -c 'echo "  MARK long-option ok"' | grep '^  MARK'
"$BASH" -lc 'case ":$BASHOPTS:" in *:login_shell:*) echo "  MARK in BASHOPTS";; esac' | grep '^  MARK'

echo "=== but not in \$-: it is not a set option"
"$BASH" -l -c 'case "$-" in *l*) echo "  MARK in dash";; *) echo "  MARK not in dash";; esac' | grep '^  MARK'

echo "=== in a login shell logout is exit"
"$BASH" -l -c 'logout 7; echo "  MARK not reached"' 2>/dev/null; echo "  rc=$?"
"$BASH" -l -c 'false; logout' 2>/dev/null; echo "  rc=$?"

echo "=== including exit's operand errors"
# Only the message, not the `<shell>: line N:` prefix the two shells naturally
# spell differently — the harness normalises that on *stderr*, and this is a
# child's stderr folded into stdout.
"$BASH" -l -c 'logout abc' 2>&1 >/dev/null | grep -o 'logout: .*'; echo "  rc=$?"
"$BASH" -l -c 'logout 3 4; echo "  MARK not reached"' 2>&1 >/dev/null | grep -o 'logout: .*'

echo "=== too many arguments is not an exit: the unit is discarded, the next runs"
exit 3 4; echo "  same line, not reached"
echo "  next unit runs, rc=$?"

echo "=== the same for a bare exit's -- form"
exit -- 3 4
echo "  still running"

echo "=== and inside a loop it takes the whole loop with it"
for i in 1 2; do echo "  i=$i"; exit 9 9; echo "  not reached"; done
echo "  after the loop"

echo "=== return the same way, in a function and out of one"
rf() { return 3 4; echo "  not reached"; }
rf; echo "  same line, not reached"
echo "  after the call"
return 3 4; echo "  same line, not reached"
echo "  after the top-level return"

echo "=== the unwind escapes an eval, unlike an expansion error"
eval "for i in 1; do exit 1 2; done
echo evalnext"; echo "  done=$?"
echo "  after the eval"

echo "=== a non-numeric operand still exits, so this is last"
exit abc
echo "  not reached"
