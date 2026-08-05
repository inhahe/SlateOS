# A `\<newline>` is deleted by the *reader*, so it only moves a syntax error's
# line when the reader actually reached it. Whether it did is decided by the
# token in front of it, and bash's `read_token` gives a sharp rule.
#
# A shell metacharacter is read, and then `peek_char = shell_getc (1)` — a read
# *with* continuation removal — is taken immediately. If the peeked character
# completes a longer operator, that operator is returned right there and the
# reader stops on the character after it. If it does not, the peek is pushed
# back with `shell_ungetc` — but a continuation it deleted on the way is gone
# for good, and `line_number` has already moved.
#
# So: an operator crosses a flush continuation *unless it is a multi-character
# operator its own lookahead completed*.
#
#   `>` `<` `;` `&` `|` `(` `)`   one character — the peek was pushed back
#   `<<` `;;` `&>`                peek once more (`<<-`/`<<<`, `;;&`, `&>>`)
#     ↳ these cross
#
#   `>>` `>&` `<&` `<>` `>|` `|&` `;&` `&&` `||` `<<-` `<<<` `&>>` `;;&`
#     ↳ these do not: they return the moment the last character is read, so the
#       backslash is still standing there unread — and the error slice keeps it
#       (`[[ a>>\` and not `Q`).
#
# A word reaches one character further still. `read_token_word` reads its own
# terminator, and when that is `<` or `>` — both `shellexp` — it peeks once more
# to test for a `<( … )` process substitution. That peek deletes a continuation
# written after the `<`/`>` even though the word then pushes *both* characters
# back, and `shell_ungetc` cannot push past the start of the line it has just
# fetched, so the reader is left at the top of it. Hence `[[ 2>\<newline>Q ]]`
# is an error on line 2 near `Q`, where the very same NUMBER token in
# `[[ 2>Q ]]` is one on line 1 near `2>`.
#
# Each case is sourced from a written file so the line numbers are the
# snippet's own rather than this file's.

l() { printf '%s\n' "$1" > sourced.sh; ( . ./sourced.sh ) 2>&1; rm -f sourced.sh; }

echo "=== a one-character operator looks past the continuation"
l '[[ 2>\
Q ]]'
l '[[ 2<\
Q ]]'
l '[[ {fd}>\
Q ]]'
l '[[ {fd}<\
Q ]]'
l 'echo a )\
b'
l 'echo a ;\
;b'

echo "=== so does one that peeks again and pushes the peek back"
l '[[ 2<<\
Q ]]'
l '[[ 2&>\
Q ]]'
l 'echo a ;;\
b'

echo "=== but one completed by its own lookahead does not"
l '[[ 2>>\
Q ]]'
l '[[ 2>&\
Q ]]'
l '[[ 2<&\
Q ]]'
l '[[ 2<>\
Q ]]'
l '[[ 2>|\
Q ]]'
l '[[ 2<<<\
Q ]]'
l '[[ 2&>>\
Q ]]'
l '[[ {fd}>&\
Q ]]'
l '[[ {fd}>>\
Q ]]'
l '[[ a>>\
Q ]]'
l '[[ a>&\
Q ]]'
l '[[ >>\
Q ]]'

echo "=== the word's own terminator is what reaches past, so it must be there"
# No continuation after the `<`/`>`, so nothing is crossed and the reader is
# parked back on the operator character it pushed away.
l '[[ 2>Q ]]'
l '[[ 2> Q ]]'
l '[[ {fd}>Q ]]'
# …and a continuation *before* the operator is inside the word's own scan.
l '[[ 2\
>Q ]]'

echo "=== outside a conditional too"
l 'echo a 2>\
;;'
l 'echo a 2>>\
;;'
l 'echo a >\
;;'
l 'echo a |\
|| b'
l 'echo a &&\
&& b'
echo done
