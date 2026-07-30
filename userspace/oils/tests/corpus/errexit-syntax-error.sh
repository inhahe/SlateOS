# Once errexit is in effect, a syntax error is reported on *one* line: bash
# stops after the first line of the diagnostic and drops whatever it would
# otherwise print next. That is not the same rule as "omit the echoed source
# line" — the dropped line is a follow-on message or a trailing `unexpected end
# of file` just as often as it is an echo.
#
# The test is errexit's state at the moment the error is *reported*, which makes
# the behaviour consistent rather than arbitrary:
#
#   * commands joined by `;` are parsed as one list, so a `set -e` among them has
#     not run yet when a later one fails to parse;
#   * commands on separate lines are parsed one at a time, so it has;
#   * errexit is unset inside a command substitution (the `inherit_errexit`
#     shopt, off by default), so a parse error in one is reported in full even
#     from an `-e` shell — while a plain `( … )` subshell inherits errexit and is
#     truncated.
#
# Every case runs in its own `$(…)`/subshell or child shell, because a syntax
# error under errexit exits the shell it happens in.

echo "=== the same error, with and without errexit ==="
# Two lines: the message and the echoed source line.
( set +e; eval 'for' ) 2>&1
echo "plain rc=$?"
# One line.
( set -e; eval 'for' ) 2>&1
echo "errexit rc=$?"

echo "=== the dropped line is not always an echo ==="
# `[[ a b ]]` is a three-line diagnostic; errexit keeps only the first, and the
# two it drops are a follow-on message and an echo.
( set +e; eval '[[ a b ]]' ) 2>&1
echo "cond-plain rc=$?"
( set -e; eval '[[ a b ]]' ) 2>&1
echo "cond-errexit rc=$?"
# `[[ -n a` would show the same thing with no echo involved at all — its second
# line is a bare `syntax error: unexpected end of file` — but it is left out
# here because osh blames a different *line number* for it than bash does, an
# unrelated divergence tracked as the residue of TD-OILS-CASE-PATTERN-EOF.
# `errexit_keeps_only_the_first_line_of_a_syntax_error` covers that shape as a
# unit test, where the line number is not what is being asserted.

echo "=== a one-line diagnostic is untouched ==="
( set +e; eval 'echo "' ) 2>&1
echo "quote-plain rc=$?"
( set -e; eval 'echo "' ) 2>&1
echo "quote-errexit rc=$?"

echo "=== when errexit takes effect decides ==="
# One `;`-separated list: `set -e` has not run when `for` fails to parse, so the
# full two-line form is used.
( eval 'set -e; for' ) 2>&1
echo "same-list rc=$?"
# Separate lines: `set -e` has run, so the terse form is used.
( eval 'set -e
for' ) 2>&1
echo "own-line rc=$?"
# And turning it back off restores the full form.
( eval 'set -e
set +e
for' ) 2>&1
echo "turned-off rc=$?"

echo "=== errexit is unset inside a command substitution ==="
# So the error inside is reported in full even though the outer shell has -e…
( set -e; x=$(eval 'for'); echo "assigned=[$x]" ) 2>&1
echo "cmdsub rc=$?"
# …while a plain subshell inherits errexit and is truncated.
( set -e; (eval 'for') ) 2>&1
echo "subshell rc=$?"
# `shopt -s inherit_errexit` makes the command substitution behave like the
# subshell.
( set -e; shopt -s inherit_errexit; x=$(eval 'for'); echo "assigned=[$x]" ) 2>&1
echo "inherit rc=$?"

echo "=== the status of a fatal parse error collapses to 2 ==="
# A command substitution whose body fails to parse is fatal and normally scores
# 127 in a `-c` shell; under errexit the shell leaves through errexit's own exit
# instead, carrying the syntax error's own status of 2.
( set +e; eval 'echo $( ! )' ) 2>&1
echo "fatal-plain rc=$?"
( set -e; eval 'echo $( ! )' ) 2>&1
echo "fatal-errexit rc=$?"

echo done
