# When a `[[ … ]]` expression is *complete* and something other than `]]`
# follows it, the frame that speaks is not `cond_term` but bash's `cond_error`,
# which reports at `cond_lineno` — the line the `[[` itself was written on:
#
#   [[ a &&        line 1: syntax error in conditional expression
#   b -gt c -gt d ]]
#                  line 2: syntax error near `-gt'
#                  line 2: `b -gt c -gt d ]]'
#
# So the two numbers disagree, and the first one does not move when the stray
# token moves further down: put another `&&` continuation in between and the
# first line still says 1. It is the only conditional diagnostic that behaves
# this way. Every other frame reports where *it* was: `cond_term` at the term it
# was reading, a `( … )` group at its own `(` — so `[[ a &&\nb ;Q ]]` (an
# incomplete term, `cond_term`'s to report) says line 2 for all three lines.
#
# Which of `cond_error`'s two spellings appears depends on the token: an
# operator it can name gets the `: unexpected token `X'` suffix, a word gets the
# bare sentence. Both carry the opening line.
#
# The multi-line cases go through `l`, which sources a written file so the line
# numbers are the snippet's own rather than this file's.

e() { ( eval "$1" ) 2>&1; }
l() { printf '%s\n' "$1" > sourced.sh; ( . ./sourced.sh ) 2>&1; rm -f sourced.sh; }

echo "=== on one line there is nothing to tell apart"
e '[[ 3 -gt 2 -gt 1 ]]'
e '[[ -z x y ]]'
e '[[ a b c ]]'
e '[[ ( a ) y ]]'
e '[[ ( a ) ;Q ]]'

echo "=== but a finished expression is blamed where it opened"
l '[[ a &&
b -gt c -gt d ]]'
l '[[ a &&
b &&
c -gt d -gt e ]]'
l '[[ a &&
b y ]]'
l '[[ ( a ) &&
( b ) y ]]'
l '[[ ( a &&
b ) y ]]'

echo "=== an operator it can name takes the other spelling, same line"
l '[[ a &&
b ) ]]'
l '[[ ( a ) &&
( b ) ;Q ]]'
l '[[ a &&
b &&
c ) ]]'

echo "=== and the frames that report where they were are unmoved"
l '[[ a &&
b &&
c ;Q ]]'
l '[[ a &&
;Q ]]'
l '[[ a &&
b ( ]]'
l '[[ a &&
b -eq ]]'
l '[[ ( a &&
b ; ]]'

echo "=== wherever in the file the conditional is written"
l 'echo one
echo two
[[ a &&
b -gt c -gt d ]]'
l 'echo one
[[ a &&
b &&
c y ]]'
echo done
