# A `time` report is not a fixed shape: bash renders it from `$TIMEFORMAT`, and
# the two forms usually seen are just the strings it falls back to —
# `\nreal\t%3lR\nuser\t%3lU\nsys\t%3lS` by default, `real %2R\nuser %2U\nsys %2S`
# for `time -p`.
#
#   * `%R` is elapsed, `%U` user CPU, `%S` system CPU. Each takes an optional
#     one-digit precision (`0`–`3`, clamped) and then an optional `l` for the
#     `NmS.FFFs` minutes form — **in that order**, so `%3lR` is a directive and
#     `%l3R` is not. Only one digit is ever read, which is why `%99R` fails on
#     its second `9`. The fraction *truncates*: 140 ms at `%2R` is `0.14`.
#   * `%P` is the CPU percentage, and bash matches it *before* the modifier scan
#     — so it accepts neither of them, and `%0P` is an error rather than a
#     two-place `%P`.
#   * `%%` is a literal `%`, a `%` at the very end of the string is itself, and
#     everything else is copied through. One newline is always appended.
#   * An **unrecognised** directive is reported and throws the whole report
#     away, the part already rendered included — but it is a `builtin_error`,
#     so `$?` is untouched and neither errexit nor posix mode ends the shell.
#     The character named is whatever bash's `*s` found, so a format that ran
#     out mid-directive names the NUL.
#   * An **empty** `$TIMEFORMAT` prints nothing at all, not even the newline.
#     A **set** one displaces the default *and* the posix-mode shell-times form
#     — which is the only way to make a posix bare `time` print an elapsed
#     figure — but never `-p`'s, which ignores the variable completely.
#   * It is read when the report is printed, not when the command starts, so a
#     `$TIMEFORMAT` the timed command set itself is the one used.
#
# No figure here can be compared between the two shells — the CPU ones are as
# real as the elapsed one — so everything below either asks at precision 0
# (whole seconds, which every span of `:` rounds to in both shells) or passes
# the report through `d`, which blanks digit runs. The one exception is a
# *cumulative* figure, which is the shell's whole run and so not even zero
# reliably; those go through `d` too.

d() { sed 's/[0-9][0-9]*/N/g'; }
# bash's line number for the report is a parser artifact — inside a compound
# command it names the *closing* line rather than the `time` — so the
# diagnostics below are compared without it. (See known-issues
# TD-OILS-TIME-REPORT-ERROR-LINE-IS-THE-COMPOUND-COMMANDS.)
nol() { sed 's/^[^:]*: line [0-9][0-9]*: //'; }

echo "=== the fallbacks are format strings like any other"
{ time :   ; } 2>&1 | d
{ time -p :; } 2>&1 | d
TIMEFORMAT=; { time : ; } 2>&1; echo "  an empty one prints nothing"

echo "=== the directives and their modifiers"
TIMEFORMAT="[%0R][%0U][%0S][%P]"; { time : ; } 2>&1
TIMEFORMAT="[%2U][%3S][%1U][%0lU][%2lS][%3lU]"; { time : ; } 2>&1
TIMEFORMAT="[%R][%2R][%lR][%4R][%9R]"; { time : ; } 2>&1 | d

echo "=== literals"
TIMEFORMAT="a%%b|tab	end|"; { time : ; } 2>&1
TIMEFORMAT="%";              { time : ; } 2>&1
TIMEFORMAT="pre%0Rpost";     { time : ; } 2>&1
TIMEFORMAT="two
lines";                      { time : ; } 2>&1

echo "=== an unrecognised directive throws the whole report away"
for f in 'ok %0R then %z' '%l' '%5' '%99R' '%l3R' '%0P' '%lP' '%0lP'; do
  TIMEFORMAT=$f
  printf '  %-16s ' "$f"
  { time : ; } 2>&1 | nol | cat -v | tr '\n' '|'
  echo " rc=$?"
done

echo "=== but it is not fatal"
( set -e;       TIMEFORMAT="%z"; { time : ; } 2>&1; echo "  reached rc=$?" )
( set -o posix; TIMEFORMAT="%z"; { time : ; } 2>&1; echo "  reached rc=$?" )

echo "=== -p ignores the variable; posix's bare time does not"
TIMEFORMAT="R=%0R"; { time -p : ; } 2>&1
( set -o posix; TIMEFORMAT="U=%0U S=%0S"; { time ; } 2>&1 | d )
# The `%R` there is the *shell's* elapsed lifetime, not the null command's, so
# it is however long this script has been running — blank it out.
( set -o posix; TIMEFORMAT="R=%0R U=%0U"; { time ; } 2>&1 | d )
# Its default form is `user\t%2lU\nsys\t%2lS` — the shell's own CPU, which at
# two places is real enough in bash to differ run to run.
( set -o posix; unset TIMEFORMAT; { time ; } 2>&1 | d )

echo "=== it is read when the report is printed"
TIMEFORMAT="A=%0U"; { time { TIMEFORMAT="B=%0U"; : ; } ; } 2>&1
f() { local TIMEFORMAT="L=%0U"; { time : ; } 2>&1; }
TIMEFORMAT="OUT=%0U"; f; { time : ; } 2>&1
unset TIMEFORMAT; { time : ; } 2>&1 | d
echo "=== done"
