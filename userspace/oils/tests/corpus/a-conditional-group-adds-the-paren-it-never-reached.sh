# A conditional diagnostic is not written by one frame. bash's `cond_term`
# reports what it could not parse; each enclosing `( … )` group then still
# checks for the `)` it never reached and reports that too; and only afterwards
# is the `syntax error near `X'` line appended:
#
#   [[ ( ;Q ) ]]   unexpected token `;' in conditional command
#                  expected `)'
#                  syntax error near `;Q'
#
# So the group's line lands *between* the two lines the failing frame produced,
# and one arrives per group the error passes through — three for three nested
# parentheses. It is the same line whatever went wrong inside: a token that
# cannot begin a term, a missing operand, a second-position operator, or a
# whole inner group that failed its own `)`.
#
# A group whose contents parsed adds nothing, because its check for `)` is then
# reached in the ordinary way: with a token there it says `unexpected token
# `X', expected `)'` as one line, and with the input exhausted it spells that
# token `EOF` and has no `near` line to point at.
#
# Each clause is reported at the line the frame *started* on, not the line the
# failure was found on — bash's `cond_term` saves `line_number` on entry and
# passes that saved one to `parser_error`. Nothing shows that on a single line,
# so the multi-line cases go through `l`, which sources a written file: that
# numbers the lines of the file it reads, keeping the expectations independent
# of this file's own numbering.

e() { ( eval "$1" ) 2>&1; }
l() { printf '%s\n' "$1" > sourced.sh; ( . ./sourced.sh ) 2>&1; rm -f sourced.sh; }

echo "=== one line per group the error passed through"
e '[[ ( ;Q ) ]]'
e '[[ ( ( ;Q ) ) ]]'
e '[[ ( ( ( ;Q ) ) ) ]]'

echo "=== whatever the failure inside was"
e '[[ ( a b ) ]]'
e '[[ ( a ; b ) ]]'
e '[[ ( -n ) ]]'
e '[[ ( -n ;Q ) ]]'
e '[[ ( a -eq ) ]]'
e '[[ ( ! ;Q ) ]]'
e '[[ ( a && ;Q ) ]]'
e '[[ ( 2>Q ) ]]'
e '[[ ( a '
e '[[ ( ( a '

echo "=== the closer reaches it with no line of its own before it"
e '[[ ( ]]'

echo "=== and an inner group that failed its own closer is just another failure"
e '[[ ( ( a ]]'
e '[[ ( ( a ; ]]'
e '[[ ( ( ( a ]]'

echo "=== a group whose contents parsed says it in one line instead"
e '[[ ( a ]]'
e '[[ ( a && b ]]'
e '[[ ( -n x ]]'
e '[[ ( ( a ) ]]'
e '[[ ( -n x '
e '[[ ( ( -n x '

echo "=== and one that closed adds nothing at all"
e '[[ ( a ) ]]; echo "status $?"'
e '[[ ( a ) b ]]'
e '[[ ( a ) && ;Q ]]'
e '[[ ;Q ) ]]'

echo "=== each clause is reported where its own group started"
l '[[ ( a &&
;Q ) ]]'
l '[[ ( a &&
( ;Q ) ) ]]'
l '[[ ( a &&
b ]]'
l '[[ (
-n x '
echo done
