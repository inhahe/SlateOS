# A matched-pair scan that runs off the end of the input does not abort the
# parse. The reader prints its own `unexpected EOF while looking for matching …`
# and then bails *to the parser*, handing it a token that is no word — so the
# parser objects in its turn and a second diagnostic follows. `[[ … ]]` is where
# that shows, because its operand slots each have a message of their own.
#
# The second names no token: there is none to name, so the operator forms lose
# their `X' and the primary form prints the one byte a `%c` of -1 gives. Nor
# does any of them get a `syntax error near …` line or a source echo. And it is
# reported at the line the *scan* ran to — one past the last line of the input,
# every line it swallowed having been counted on the way.
#
# Each case needs an input of its own, because the error ends the one it is in.
# Verified against bash 5.2.37.

e() { ( eval "$1" ) 2>&1; echo "  rc=$?"; }

echo "=== after a binary operator, in each construct the scan can die inside ==="
e '[[ x =~ ( ]]'
e '[[ x =~ (a ]]'
e '[[ x =~ " ]]'
e '[[ x =~ ` ]]'
e '[[ x =~ ${ ]]'

echo "=== after a unary operator, and in primary position ==="
e '[[ -n " ]]'
e '[[ " ]]'
# `!` and `&&` reach the same slots.
e '[[ ! x =~ ( ]]'
e '[[ -n x && " ]]'

echo "=== a substitution is scanned by a reader of its own ==="
# It has taken the newline by the time it gives up, so both lines are the one
# past rather than the scan's line and the one after it.
e '[[ x =~ $(echo a ]]'
e '[[ -n $(echo a ]]'
e '[[ $(echo a ]]'

echo "=== each enclosing group still adds the ) it never reached ==="
# After the bail message, and at the line its own `(` was on.
e '[[ ( ( x =~ " ) ) ]]'

echo "=== the line is where the reader stopped, not where the operator was ==="
e 'echo A
[[ x =~ (
one
two'
e 'echo A
[[ x =~ ( ]]
echo B
echo C'
# Nothing after the `[[` line is read, so B and C never run — but A already has.

echo "=== not every unclosed operand is a bail ==="
# Without a regex on the left of it a `(` is never given to the matched-pair
# scan, so it is an ordinary token objected to where it stands.
e '[[ x == ( ]]'
e '[[ ( ]]'
e '[[ x =~ ) ]]'
# A closed group is no failure at all.
e '[[ x =~ ( ) ]]; echo rc-ok=$?'
# An input that simply ends leaves the grammar to report it, in its own forms.
e '[[ a == b'
e '[[ a &&'

echo done
