# An alias value and the line that called it are one text, not two, and the
# reader has no seam between them at any level below the token.
#
# `push_string` (parse.y:2694) makes the replacement the current
# `shell_input_line` and stacks the line it displaced *with the reader's index
# just past the alias word*; when the replacement runs dry `pop_string` restores
# that line at that index and the same scan continues. So a construct may be
# written half in the value and half on the calling line and still be one
# construct: `take_operator` extends a `<` into `<<` across it, a quote or a
# `$(` opened in the value is simply still open when the reader arrives on the
# calling line, and a comment opened in the value keeps eating there
# (`discard_until('\n')` reads through `shell_getc`, which pops transparently).
#
# One thing does *not* span it: a word begun on the calling line cannot reach
# back into the value, because the alias word is always ended by a blank or a
# metacharacter — that is what ended it. `alias V='echo ab'` with `V cd` is two
# words in both shells.

shopt -s expand_aliases

echo "=== operators are one operator"
alias G='echo hi >'
G> out; G> out; cat out          # `>>`, twice, so the appending is visible

alias P='true |'
P| echo "or ran"                 # `||`, and `true` succeeded, so it does not

alias N='false &'
N& echo "and did not run"        # `&&` after a failure
wait

echo "=== a substitution opened in the value closes on the calling line"
alias C='echo $(echo'
C spanned)

echo "=== a comment opened in the value eats the calling line"
alias A='echo hi #c'
A there
echo "next"

echo "=== the error a spanned operator causes is the spanned operator's"
# `;` from the value and `;` from the calling line make one `;;`, which is a
# grammar error outside a `case` — and bash names `;;`, not either `;`.
( eval 'alias S="echo a ;"
S; echo b' ) 2>&1
echo "rc=$?"

echo "=== but a word does not reach back"
alias V='echo ab'
V cd

echo "=== nor does the alias word itself extend"
# `G` above is a candidate only because the `>` after it ended the word; had it
# been part of the word there would have been no alias at all.
alias W='echo W-ran'
Wx() { echo "Wx ran"; }
Wx

# Not probed: an unclosed `'`, `"` or `$(` that the calling line closes —
# `alias U="echo 'x"` plus `U y'`. bash reads it as the one word `x y`; osh
# lexes the whole script before the alias pass can run, so the value's half is
# unterminated at lex time and the script never reaches the pass. See
# TD-OILS-AN-UNCLOSED-QUOTE-IN-AN-ALIAS-VALUE-IS-A-LEX-ERROR.
