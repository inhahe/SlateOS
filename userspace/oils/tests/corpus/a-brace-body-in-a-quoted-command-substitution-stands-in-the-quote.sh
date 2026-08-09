# Whether a `$'…'` in a `${ … }` body is re-quoted after translation turns on
# `P_DQUOTE`, and `P_DQUOTE` is not "the `${` was written inside quotes" — it is
# `rflags = (qc == '"') ? P_DQUOTE : (flags & P_DQUOTE)` (parse.y:3696), where
# `qc` is the `cd` `read_token_word` hands down: `parse_matched_pair (cd, '{',
# '}', …)` (parse.y:5033). And `cd` is `current_delimiter (dstack)` — the top of
# a stack that only three sites push:
#
#   | site | pushes | reached by |
#   |------|--------|-----------|
#   | parse.y:4952 | the quote character | a `'`, `"` or `` ` `` starting a word |
#   | parse.y:5041 | `(` | a `$(` starting or continuing a word |
#   | parse.y:5071 | `(` | a `<(` / `>(` |
#
# `parse_matched_pair`'s own nested-`$(` arm pushes **nothing**:
#
#     nestret = parse_comsub (0, '(', ')', &nestlen, (rflags|P_COMMAND) & ~P_DQUOTE);
#                                                                (parse.y:3960)
#
# — and clearing `P_DQUOTE` there buys nothing, because `parse_comsub` runs a
# whole nested `yyparse` (parse.y:4153) and that flag never reaches the body's
# own `read_token_word`. What reaches it is the `dstack`, which still has the
# enclosing `"` on top because `read_token_word` pushed it at 4952 and does not
# pop until the whole `"…"` is read.
#
# So a `$( … )` **written inside double quotes** hands its body the `"`, and
# every `${ … }` the body reads directly is `P_DQUOTE` — spliced bare — even
# though nothing in the body is quoted. The same body written unquoted pushes a
# `(` of its own and re-quotes. The printbacks below are the ground truth: they
# are the token text, after the translation the parser already did.
#
# Everything that pushes clears it again, and nothing else does:
#
#   * a nested word-level `$(`, `<(` or `>(` in the body — its own `(` on top
#   * a `"…"` run in the body — its own `"`, which is `P_DQUOTE` again anyway
#   * a `` ` … ` `` — bash keeps that body as *source* (`parse_matched_pair`,
#     parse.y:3953, no parse), so the `$'…'` in it is not translated at parse
#     time at all and the expansion's own read starts from an empty stack
#
# and a `{ … }` or `( … )` group in the body pushes nothing, so it does not.
#
# Verified against bash 5.2.37.

unset x y

echo "=== the printbacks: what the parser translated, and how it re-quoted it"
f1() { echo "$(echo ${x:-$'a\tb'})"; }              # quoted $( : bare
f2() { echo $(echo ${x:-$'a\tb'}); }                # unquoted $( : re-quoted
f3() { echo "$(echo $(echo ${x:-$'a\tb'}))"; }      # inner $( pushes: re-quoted
f4() { echo "$(echo "$(echo ${x:-$'a\tb'})")"; }    # inner " pushes ": bare
f5() { echo "`echo ${x:-$'a\tb'}`"; }               # backtick: not translated
f6() { echo "$(cat <(echo ${x:-$'a\tb'}))"; }       # <( pushes: re-quoted
f7() { echo "$( { echo ${x:-$'a\tb'}; } )"; }       # a group pushes nothing: bare
f8() { echo "${y:-$(echo ${x:-$'a\tb'})}"; }        # the ${ } body's own "
declare -f f1 f2 f3 f4 f5 f6 f7 f8

echo "=== row two is still row two inside one: an operator re-quotes"
g1() { echo "$(echo ${x#$'a\tb'})"; }
g2() { echo "$(echo ${x%$'a\tb'})"; }
g3() { echo "$(echo ${x:-$'a\tb'} ${x#$'a\tb'})"; }
declare -f g1 g2 g3

echo "=== a word-level \$'…' is read_token_word's, so the delimiter is not its"
h1() { echo "$(echo $'a\tb')"; }
h2() { echo "$(echo x$'a\tb'y)"; }
declare -f h1 h2

echo "=== and the difference is live: a bare splice's tab is a splitting one"
# Every row captures each substitution's output whole, so the one splitting a
# row can show is the one the `${ … }` itself is the word of: a bare splice puts
# a real tab there and the `echo` reading it prints two fields, a re-quoted one
# keeps the tab inside a `'…'` and prints one. `[a b]` is bare, `[a<TAB>b]` is
# re-quoted — and the rows split exactly as the printbacks above say they will.
p() { printf '%-34s -> [%s]\n' "$1" "$2"; }
p 'quoted $('            "$(echo ${x:-$'a\tb'})"
v=$(echo ${x:-$'a\tb'})
p 'unquoted $('          "$v"
p 'inner word-level $('  "$( w=$(echo ${x:-$'a\tb'}); echo "$w" )"
p 'inner quoted $('      "$(echo "$(echo ${x:-$'a\tb'})")"
p 'a group in the body'  "$( { echo ${x:-$'a\tb'}; } )"
p 'a <( ) in the body'   "$(cat <(echo ${x:-$'a\tb'}))"
p 'a backtick body'      "`echo ${x:-$'a\tb'}`"

echo "=== a spliced brace closes the body's expansion, not the substitution"
e() { ( eval "$1" ) 2>&1; echo "  rc=$?"; }
e 'echo "$(echo ${x:-$'\''a}b'\''})"'
e 'echo "$(echo ${x:-$'\''a}b'\''} tail)"'
# Unquoted, the same body re-quotes and the `}` is just text in the value.
e 'echo $(echo ${x:-$'\''a}b'\''})'

echo "=== and a spliced NUL cuts the substitution's body where it lands"
# `nestlen = ttranslen` (parse.y:3892), so the NUL is in the token buffer and
# `savestring` ends the body there — the `$( … )` keeps a body that stops mid
# expansion, which is what the printback shows and what the re-read fails on.
n() { echo "$(echo ${x:-$'a\0b'})"; }
declare -f n
e 'echo "$(echo ${x:-$'\''a\0b'\''})"'
e 'echo $(echo ${x:-$'\''a\0b'\''})'

echo done
