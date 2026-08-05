# `syntax error near `X'` does not name a token. bash's `error_token_from_text`
# slices its *input line*: from the character just past the token it steps back
# over trailing whitespace, back again to the nearest delimiter, and returns
# everything from there through the one character that followed the token. So
# the text bash prints is whatever was *written* around the error, and two
# things fall out of that which no token can express:
#
#   [[ P;QRS ]]   near `;Q'     — exactly one character after, never two
#   [[ P; Q ]]    near `;'      — a space after, so nothing comes along
#   [[ a>>b ]]    near `a>>b'   — the word before, because the scan back does
#                                 not stop at a token boundary
#
# The delimiters that stop the scan are exactly ` `, `\n`, `\t`, `;`, `|` and
# `&`. Everything else is ordinary text to it — parentheses above all, which is
# why `[[ -n @(a) ]]` is reported near `@(a', and quotes and redirection
# characters too.
#
# A multi-character operator is *not* reported whole: the scan stops at the
# first delimiter it meets inside one, so `[[ P;;Q ]]` is near `;Q' and
# `[[ a>|b ]]` near `|b', while `>>` and `<<<` (which hold no delimiter) come
# through entire.
#
# Where a newline stands, the position is the end of the *previous* token —
# bash's input line ends there, so stepping back off the end skips the newline
# and every space before it. `[[ a` is near `a' and `[[ a -eq` near `-eq'.
#
# None of this is reserved for the diagnostics that *name* a token. Every
# conditional error goes through the same slice, including the two that report
# only a bare sentence — `conditional binary operator expected` and `syntax
# error in conditional expression` — so a word in either of those positions is
# still reported as source and not as itself: `[[ a b;c ]]` is near `;', not
# near `b'.
#
# A `\<newline>` is deleted by the reader before the parser sees any of this, so
# the character after the token is the first one on the next line:
# `[[ P;\<newline>Q ]]` is near `Q', with no backslash in sight. (bash also
# blames that error on line 2 and echoes line 2's text; osh says line 1 — see
# TD-OILS-COND-ERROR-LINE-AFTER-A-CONTINUATION in known-issues.md, which is why
# that one case is run for its `near' text alone.)
#
# Each case runs under `eval` in a subshell, since a syntax error otherwise
# abandons the rest of the input.

e() { ( eval "$1" ) 2>&1; }

echo "=== one character after the token, and only one"
e '[[ P;Q ]]'
e '[[ P;QRS ]]'
e '[[ P; Q ]]'
e '[[ P;	Q ]]'
e '[[ P;$n ]]'
e '[[ P;"Q" ]]'
e "[[ P;'Q' ]]"
e '[[ P;(Q ]]'
e '[[ P;<Q ]]'
e '[[ P;\Q ]]'

echo "=== and whatever was written before it"
e '[[ a>>b ]]'
e '[[ a<<b ]]'
e '[[ a<<<b ]]'
e '[[ a=b;c ]]'
e '[[ P)Q ]]'
e '[[ -n @(a) ]]'
e '[[ a&&b;c ]]'
e '[[ a||b|c ]]'
e '[[ $(echo a);b ]]'
e '[[ ${x};b ]]'

echo "=== a delimiter inside the operator cuts it short"
e '[[ P;;Q ]]'
e '[[ P|Q ]]'
e '[[ P&Q ]]'
e '[[ a>|b ]]'
e '[[ P;&Q ]]'

echo "=== a newline reports the end of the token before it"
e '[[ a
== b ]]'
e '[[ a -eq
1 ]]'
e '[[ a>>b
]]'
e '[[ a'
e '[[ a -eq'

echo "=== the reader deleted the line continuation"
( eval '[[ P;\
Q ]]' ) 2>&1 | sed -n 's/.*\(syntax error near .*\)/\1/p'
e '[[ a b\
c ]]'
e '[[ -z x y\
z ]]'
e '[[ a $(echo x)\
b ]]'

echo "=== the same wherever the conditional is written"
e 'echo $([[ a>>b ]])'
e 'cat <([[ a>>b ]])'
e 'f() { [[ P;Q ]]; }'
e 'while [[ P;Q ]]; do :; done'
e 'case x in y) [[ P;Q ]];; esac'
e '{ [[ a>>b ]]; }'
e '[[ P;Q ]] && echo x'
e 'echo one
[[ P;Q ]]'

echo "=== a word position is sliced the same way, not named"
e '[[ a b ]]'
e '[[ -z x y ]]'
e '[[ 3 -gt 2 -gt 1 ]]'
e '[[ a "<" b ]]'
e "[[ 'a<' b ]]"
e '[[ ]]'
e '[[ -n ]]'
e '[[ a b;c ]]'
e '[[ a b|c ]]'
e '[[ a b&c ]]'
e '[[ a b;;c ]]'
e '[[ a b) ]]'
e '[[ a b( ]]'
e '[[ a b	c ]]'
e '[[ a $(echo x) ]]'
e '[[ a `echo x` ]]'
e '[[ a ${x}y ]]'
e '[[ a && b c;d ]]'
e '[[ ( a b) ]]'
e '[[ -z x y;z ]]'
e '[[ -z x $(echo q) ]]'
e '[[ a == b c|d ]]'
e '[[ a == b y) ]]'
e '[[ ( a ) b;c ]]'
echo done
