# A `$'…'` written inside a `${ … }` body is translated where it is read, like
# one written in any other grouping construct — `parse_matched_pair` runs
# `ansiexpand` over it, re-quotes the result with `sh_single_quote`, and backs
# up over the `$'` it had already written (`retind -= 2`, parse.y:3882–3893).
# So the body that travels downstream holds an ordinary single-quoted run and
# no `$'` at all.
#
# The re-quoting is what keeps a value a value: the spliced text is read again
# by whatever reads the body, so an unquoted `'`, `}` or `*` in it would be
# that reader's. `$'\x27'` shows the one special case of `sh_single_quote`
# (shquote.c:105) — a translation that is nothing but a quote is written `\'`.
#
# Two consequences of translating this early:
#
#   * a NUL ends the string, so `${x:-$'a\0b'}` is `${x:-'a'}`;
#   * the text changes *shape*. A `$'x\ny'` puts a real newline where the
#     source had none, so a `$LINENO` after it inside the same `$( … )` body
#     sits one line lower than the line the whole thing was written on.
#
# In arithmetic the translation is the whole story. A single-quoted run is
# never valid arithmetic — and the quotes survive, because bash expands an
# arithmetic string with `Q_DOUBLE_QUOTES|Q_ARITH` (subst.c:8134) and
# `expand_word_internal`'s `'` arm is `if (quoted & (Q_DOUBLE_QUOTES|
# Q_HERE_DOCUMENT)) goto add_character` (subst.c:11577), so a `'` in the
# operand is an ordinary character handed to the evaluator. `$(( ${x:-'5'} ))`
# and `$(( ${x:-$'5'} ))` are the same error, and it names `'5'`.
#
# Not covered here: the same `$'…'` written inside double quotes, where bash
# 5.2 translates but does *not* re-quote — a defect it has already fixed behind
# `#if 0 /* TAG:bash-5.3 */`. See `known-issues.md`,
# `TD-OILS-AN-ANSI-C-STRING-IS-NOT-REQUOTED-AFTER-A-BRACE-BODY-TRANSLATES-IT`.
#
# Verified against bash 5.2.37.

p() { printf '%-46s -> [%s]\n' "$1" "$2"; }
e() { ( eval "$1" ) 2>&1; echo "  rc=$?"; }

unset x

echo "=== the translation is single-quoted, so arithmetic rejects it ==="
e 'echo $(( ${x:-$'\''5'\''} + 0 ))'
e 'echo $(( ${x:-$'\''a'\''} ))'
e 'echo $(( ${x:-$'\''a\tb'\''} ))'
# A run written with plain quotes is the same error, which is the point: the
# `$'…'` became one.
e 'echo $(( ${x:-'\''5'\''} ))'
# `$[ … ]` is a grouping construct too.
e 'echo $[ ${x:-$'\''5'\''} ]'

echo "=== every operator's word, not just :- ==="
e 'echo $(( ${x-$'\''5'\''} ))'
e 'y=1; echo $(( ${y:+$'\''5'\''} ))'
e 'echo $(( ${x:=$'\''5'\''} ))'

echo "=== but a double-quoted run is removed, so it still evaluates ==="
p 'a quoted operand' "$(( ${x:-"5"} ))"
p 'an unquoted one'  "$(( ${x:-5} ))"

echo "=== the escapes are resolved, and the result re-quoted ==="
# Written outside quotes throughout — inside them bash 5.2 takes the other
# branch, which is the defect named in the header.
v=${x:-$'a\tb'};   p 'a tab'            "$v"
v=${x:-$'a\0b'};   p 'a NUL ends it'    "$v"
v=${x:-$'a\047b'}; p 'an escaped quote' "$v"
v=${x:-$'\x27'};   p 'only a quote'     "$v"
v=${x:-$'a*b'};    p 'a glob character' "$v"
v=${x:-$'a}b'};    p 'a closing brace'  "$v"
# Quoted, so no splitting and no globbing: one field, tab and all.
printf '<%s>' ${x:-$'a\tb'}; echo
printf '<%s>' ${x:-$'a*b'}; echo

echo "=== in the pattern and replacement beside the operand ==="
y=xAy
v=${y#$'x'};        p 'a prefix pattern' "$v"
v=${y%$'y'};        p 'a suffix pattern' "$v"
v=${y/$'A'/$'\t'};  p 'a replacement'    "$v"

echo "=== the translation is what the printback shows ==="
f() {
  : ${x:-$'a\047b'}
  : ${x:-$'\x27'}
  : ${x#$'p'}
  : ${x:-$'x\ny'}
  : $(( ${x:-$'5'} ))
}
declare -f f

echo "=== and it changes the body's shape, so \$LINENO moves ==="
# The base is printed beside it because the interesting number is the
# difference: the translated run carries a real newline, so the command after
# it inside the body sits one line lower.
r=$(echo ${x:-$'p\nq'}; echo L=$LINENO); echo "A [$r] base=$LINENO"
s=$(echo ${x:-$'pq'}; echo L=$LINENO); echo "B [$s] base=$LINENO"

echo done
