# The operand of a substitution — the `w` of `${x:-w}` — is read with whatever
# quoting the substitution itself was written in. Bare, it is an ordinary word:
# `'…'` quotes, and a backslash escapes whatever follows it. Inside `"…"` the
# quotes never stopped applying, so a `'` is just a character and a backslash
# reaches only as far as it does anywhere else in double quotes — before `$`,
# `` ` ``, `"`, `\` and the `}` that ends the substitution, and nothing else.
#
# Three things are *not* this rule:
#
#   * A pattern is not an operand. `${v#'s'}`, `${v/'e'/X}` and `${v^'s'}` do
#     their own quote removal, in or out of quotes, so the `'` goes either way.
#   * The operand is still a *word*, not a literal run: `$…`, `` `…` `` and a
#     nested `"…"` stay live, and so do `$'…'` and `$"…"`, which a real
#     double-quoted body would have left alone.
#   * A here-document body is a double-quoted run without any quotes written,
#     so an operand in it follows the quoted rule — unless the delimiter was
#     quoted, in which case nothing in the body is read at all.

show() { printf '  %-26s(%d)' "$1" $(($# - 1)); shift; printf '<%s>' "$@"; printf '\n'; }

v=set
x=
a=('p q' r)

echo "### a quote written in the operand is a character, not a quote"
show 'sq'             "${nope:-'a b'}"
show 'sq empty'       "${nope:-''}"
show 'sq colonless'   "${nope-'a b'}"
show 'sq alternate'   "${v:+'a b'}"
show 'bare sq'        ${nope:-'a b'}

echo "### the backslash reaches exactly as far as it does in quotes"
show 'bs dollar'      "${nope:-\$v}"
show 'bs backtick'    "${nope:-\`}"
show 'bs dquote'      "${nope:-\"}"
show 'bs backslash'   "${nope:-\\}"
show 'bs brace'       "${nope:-\}}"
show 'bs t'           "${nope:-a\tb}"
show 'bs n'           "${nope:-a\nb}"
show 'bs squote'      "${nope:-\'a\'}"
show 'bs space'       "${nope:-a\ b}"
show 'bs star'        "${nope:-a\*b}"
show 'bs digit'       "${nope:-a\1b}"
show 'bare bs t'      ${nope:-a\tb}
show 'bare bs space'  ${nope:-a\ b}

echo "### it is still a word, so the expansions in it are live"
show 'param'          "${nope:-[$v]}"
show 'cmdsub'         "${nope:-[$(echo hi)]}"
show 'backtick'       "${nope:-[`echo hi`]}"
show 'nested dquote'  "${nope:-x"'y'"z}"
show 'ansi c'         "${nope:-$'a\tb'}"
show 'locale'         "${nope:-$"hi"}"
show 'nested operand' "${nope:-${nope2:-'a b'}}"
show 'bare nested'    ${nope:-${nope2:-'a b'}}

echo "### the assignment stores the characters the operand spelled"
show 'assign'         "${x:='a b'}"
echo "  x=[$x]"
unset x
show 'bare assign'    ${x:='a b'}
echo "  x=[$x]"

echo "### a pattern is not an operand, and never was"
show 'trim'           "${v#'s'}"
show 'trim empty'     "${v#''}"
show 'subst pattern'  "${v/'e'/X}"
show 'subst repl'     "${v/e/'X'}"
show 'case pattern'   "${v^'s'}"
show 'bare trim'      ${v#'s'}

echo "### a list-valued operand answers the same way"
show 'array alt'      "${a[@]:+'a b'}"
show 'array default'  "${nada[@]:-'a b'}"
show 'positional alt' "${@:+'a b'}"

echo "### a here-document body is quoted unless its delimiter said otherwise"
cat <<EOF
  [${nope:-'a b'}] [${nope:-a\tb}] [${nope:-\$v}] [${nope:-\}}]
EOF
cat <<'EOF'
  [${nope:-'a b'}] [${nope:-a\tb}]
EOF
cat <<"EOF"
  [${nope:-'a b'}]
EOF
