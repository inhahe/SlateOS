# bash's `cond_term` tries each thing that can *begin* a conditional term —
# `]]`, `(`, `!`, a unary operator, a word — and everything left over falls to a
# final `else` that names the token:
#
#   [[ ;Q ]]   unexpected token `;' in conditional command
#
# In practice "everything left over" is exactly the operators, because a word is
# a term. The one word that is not a term — the `]]` closer — leaves through an
# earlier arm, so `[[ ]]` prints no such line at all. End of input does print
# one, spelled `EOF`.
#
# The wording is not the one a *second*-position operator gets. There the term
# already has an operand and bash is looking for a binary operator, so `[[ P;Q ]]`
# says ``unexpected token `;', conditional binary operator expected`` — a
# different sentence from a different place.
#
# The token is spelled as written, so a two- or three-character operator comes
# through whole (`&&`, `;;&`, `>|`). Two cannot be spelled at all:
# `error_token_from_token` has no text for `IO_NUMBER` or `REDIR_WORD`, which
# carry a number and a word rather than a fixed spelling, and bash falls through
# to printing the raw yacc token *number* — so `[[ 2>Q ]]` really does say
# `unexpected token 284`, unquoted.
#
# A `!`, a `(` and a leading newline are all terms or skipped, so none of them
# reaches this message; `[[ !Q ]]` is a negated string test and succeeds.
#
# Each case runs under `eval` in a subshell, since a syntax error otherwise
# abandons the rest of the input. Two cases run through `grep` as well: when the
# offending token is inside a `( … )` group bash adds an `expected `)'` line
# that osh does not — see TD-OILS-COND-ERROR-DROPS-THE-EXPECTED-PAREN-LINE in
# known-issues.md — so they are pinned for everything else they print.

e() { ( eval "$1" ) 2>&1; }
g() { ( eval "$1" ) 2>&1 | grep -v "expected \`)'"; }

echo "=== an operator cannot begin a term, so it is named"
e '[[ ;Q ]]'
e '[[ ; ]]'
e '[[ |Q ]]'
e '[[ &Q ]]'
e '[[ &&Q ]]'
e '[[ ||Q ]]'
e '[[ ;;Q ]]'
e '[[ ;&Q ]]'
e '[[ ;;&Q ]]'
e '[[ )Q ]]'
e '[[ >Q ]]'
e '[[ <Q ]]'
e '[[ >>Q ]]'
e '[[ <&Q ]]'
e '[[ >&Q ]]'
e '[[ >|Q ]]'
e '[[ <<Q ]]'
e '[[ &>Q ]]'

echo "=== the two bash cannot spell, which it numbers instead"
e '[[ 2>Q ]]'
e '[[ {fd}>Q ]]'

echo "=== end of input is named too, but the closer is not"
e '[[ '
e '[[ ]]'
e '[[ a && ]]'

echo "=== and a term that can begin one never reaches the message"
e '[[ !Q ]]; echo "status $?"'
e '[[
Q ]]; echo "status $?"'
e '[[ a &&
]]'

echo "=== primary position is wherever a term is expected, not just the first"
e '[[ ! ;Q ]]'
e '[[ P == P && ;Q ]]'
e '[[ P == P || ;Q ]]'
g '[[ ( ;Q ) ]]'
g '[[ ( a '

echo "=== a second-position operator gets the other sentence"
e '[[ P;Q ]]'
e '[[ P|Q ]]'
e '[[ P&Q ]]'
e '[[ P
Q ]]'
e '[[ a'

echo "=== and an operand slot after a unary or binary operator gets a third"
e '[[ -n ;Q ]]'
e '[[ -n ]]'
e '[[ a -eq ]]'
e '[[ -n '
e '[[ a -eq '
echo done
