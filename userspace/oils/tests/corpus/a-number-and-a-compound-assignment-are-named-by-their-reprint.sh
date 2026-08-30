# `error_token_from_token` renders some tokens rather than quoting the source.
#
# Its switch (parse.y:6145–6166) has a branch per token kind, and two of them
# are not the text that was written:
#
# A `NUMBER` returns `itos (yylval.number)` — the *value*. So `007>x` is
# reported near `7`, with the leading zeros gone, which is the discriminator:
# nothing that echoed the source could produce it.
#
# A compound `ASSIGNMENT_WORD` returns `yylval.word->word`, and for `a=( … )`
# that word is a re-print too. `read_token_word` builds it from the name, then
# `=`, `(`, the `string_list` of the elements and `)` (parse.y:5168–5181), and
# `string_list` joins with exactly one space (parse.y:6572–6576). So the gaps
# between elements are normalised — `a=(1   2)` comes back `a=(1 2)` and
# `a=(  )` comes back `a=()` — while each element keeps the spelling it was
# written with, quotes, subscripts, substitutions and all.
#
# A `REDIR_WORD` is the third case worth naming here: it is *absent* from the
# switch, so `error_token_from_token` returns NULL and `report_syntax_error`
# falls through to its text-scanning branch, which prints the other message
# shape — `syntax error near` with no `unexpected token`.
#
# A function body is the shortest position that reaches all three, because
# `function_body` is a `shell_command` and none of them is one. Each ends the
# parse unit, so they live in their own files run with `.`.
#
# Verified against bash 5.2.37.

mk() { printf '%s\n' "$2" > "$1"; }

echo "=== a NUMBER is named by its value, not its spelling"
mk a1 'f() 007>x'
. ./a1
mk a2 'f() 2>/dev/null'
. ./a2
mk a3 'f() 12>&1'
. ./a3
mk a4 'f() 0>x'
. ./a4

echo "=== a compound assignment is named by a re-print that joins with one space"
mk b1 'f() a=(1   2)'
. ./b1
mk b2 'f() a=(  )'
. ./b2
mk b3 'f() a=(1 2)'
. ./b3

echo "=== but each element keeps the spelling it was written with"
mk c1 'f() a=('"'"'x y'"'"' "z")'
. ./c1
mk c2 'f() a=([2]=v x)'
. ./c2
mk c3 'f() a=($(echo h) ~/t)'
. ./c3
mk c4 'f() a=(*.c {p,q} $((1+1)))'
. ./c4

echo "=== and the name part is carried through whole"
mk d1 'f() a+=(p)'
. ./d1
mk d2 'f() a[1+1]=(q)'
. ./d2
mk d3 'f() a[i]+=(r)'
. ./d3

echo "=== a REDIR_WORD is not in the switch at all, so the other branch reports"
mk e1 'f() {v}>/dev/null'
. ./e1
mk e2 'f() {v}<in'
. ./e2
