# The word a `${x:-w}` or `${x:+w}` hands back is *the word it is*: bash expands
# it where the substitution stands, so the quoting written inside it is still on
# the characters it produced. `${nope:-'a b'}` is one field and `${nope:-'*'}` a
# literal star, while `${nope:-a b}` is two fields and `${nope:-$g}` (g=`*`)
# globs — the operand splits and globs exactly as much as the same text would
# have on its own, no more and no less.
#
# Three consequences are worth pinning separately, because a shell that only
# remembered "the answer was quoted" would get them wrong:
#
#   * The operand's *own* field breaks survive. `${nope:-A"${a[@]}"B}` is two
#     fields, `<Ap q>` and `<rB>`, because the quoted `[@]` inside it broke them
#     — no amount of joining and re-splitting could put that break back.
#   * A quoted empty operand is still a field: `${nope:-''}` is one empty
#     argument where `${nope:-$e}` (e unset) is none.
#   * Only these two operators substitute a word. `${x:='a b'}` splits, and so
#     does the replacement half of `${x/p/'a b'}` — those quotings are gone.
#
# Pattern contexts follow the same rule, which is where it is easiest to see
# that this is about the characters and not about the field: the star an operand
# kept is not pattern syntax either.

show() { printf '  %-18s(%d)' "$1" $(($# - 1)); shift; printf '<%s>' "$@"; printf '\n'; }

: > one.dat
: > two.dat

v='p q'
g='*'
e=''
a=('p q' r)
z0=()

echo "### a quoted operand is one field, an unquoted one splits"
show "sq"        ${nope:-'a b'}
show "dq"        ${nope:-"a b"}
show "esc"       ${nope:-a\ b}
show "q var"     ${nope:-"$v"}
show "bare"      ${nope:-a b}
show "var"       ${nope:-$v}
show "mixed"     ${nope:-$v' '$v}

echo "### and the same for the alternate, and for the colon-less pair"
x=set
show ":+ sq"     ${x:+'a b'}
show ":+ bare"   ${x:+a b}
show "- sq"      ${nope-'a b'}
show "+ sq"      ${x+'a b'}

echo "### a quoted star is a star, an unquoted one is a glob"
show "sq star"   ${nope:-'*.dat'}
show "dq star"   ${nope:-"$g".dat}
show "var star"  ${nope:-$g.dat}
show "lit star"  ${nope:-*.dat}

echo "### an empty operand: quoted makes a field, unset makes none"
show "sq empty"  ${nope:-''}
show "var empty" ${nope:-$e}
show "dq empty"  ${nope:-""}

echo "### the operand's own field breaks survive the substitution"
show "q at"      ${nope:-A"${a[@]}"B}
show "unq at"    ${nope:-A${a[@]}B}
show "list"      ${nope:-'a b' c}
show "around"    A${nope:-'a b' c}B

echo "### and a nested operand keeps what the inner one kept"
show "nested"    ${nope:-${nope2:-'a b'}}
show "nested unq" ${nope:-${nope2:-a b}}

echo "### unquoted text in the operand still splits, under whatever IFS"
IFS=:
show "colon lit"  ${nope:-a:b}
show "colon sq"   ${nope:-'a:b'}
show "colon var"  ${nope:-$v}
show "space lit"  ${nope:-a b}
show "colon at"   ${nope:-${a[@]}}
show "colon q at" ${nope:-"${a[@]}"}
IFS=
show "empty ifs"  ${nope:-a b}
show "empty at"   ${nope:-${a[@]}}
unset IFS
show "back"       ${nope:-a b}

echo "### a list inside the operand becomes text, joined the way it is spelled"
# `[@]` glues its items with a space and `[*]` with `$IFS`, and that holds for
# the *derived* lists too — where the quoted spellings of the same two both join
# with `$IFS`. Only a quoted `[@]` keeps its breaks, and only a null `$IFS`
# leaves an unquoted one's standing.
for ifs in ' ' ':' ''; do
  IFS=$ifs
  printf '  [%s]\n' "$ifs"
  show "at"        ${nope:-${a[@]}}
  show "star"      ${nope:-${a[*]}}
  show "q at"      ${nope:-"${a[@]}"}
  show "q star"    ${nope:-"${a[*]}"}
  show "keys"      ${nope:-${!a[@]}}
  show "keys star" ${nope:-${!a[*]}}
  show "slice"     ${nope:-${a[@]:0:2}}
  show "A at B"    ${nope:-A${a[@]}B}
  show "A q at B"  ${nope:-A"${a[@]}"B}
  show "nested at" ${nope:-${nope2:-"${a[@]}"}}
done
unset IFS

echo "### the whole-array and positional spellings answer the same way"
show "array"      ${a[@]:-'a b' c}
show "array none" ${nope2[@]:-'a b' c}
( set -- ; show "posn" ${@:-'a b' c} )
( set -- ; show "posn star" ${*:-'a b' c} )

echo "### a pattern sees the quoting too"
case ab in ${nope:-'*'}) echo "  case sq: match";; *) echo "  case sq: no";; esac
case ab in ${nope:-*})   echo "  case unq: match";; *) echo "  case unq: no";; esac
[[ ab == ${nope:-'*'} ]] && echo "  [[ sq: match" || echo "  [[ sq: no"
[[ ab == ${nope:-*} ]]   && echo "  [[ unq: match" || echo "  [[ unq: no"

p='a*b'
echo "  trim sq:  [${p#${nope:-'a*'}}]"
echo "  trim unq: [${p#${nope:-a*}}]"
echo "  repl sq:  [${p/${nope:-'*'}/Z}]"
echo "  repl unq: [${p/${nope:-*}/Z}]"

echo "### a string context joins the operand's fields with a space, always"
# Only the *elements* of a list answer to the `[*]` separator; an operand is a
# word whose fields join with a space, whatever spelling asked for it.
IFS=:
s=${nope:-"${a[@]}"}; echo "  assign q at:  [$s]"
s=${nope:-'a b' c};   echo "  assign list:  [$s]"
s="${nope:-a b}";     echo "  assign quoted:[$s]"
s=${z0[*]:-'p q' r};  echo "  assign star:  [$s]"
s=${z0[@]:-'p q' r};  echo "  assign at:    [$s]"
s=${a[*]:-'p q' r};   echo "  assign elems: [$s]"
unset IFS

echo "### and quoted, the operand is one field — an empty word still being one"
# Which is the whole difference between the two operators here: `:-` always
# substitutes its word, so even an empty one is an argument, where an inactive
# `:+` substitutes nothing at all.
show "q empty"     "${z0[@]:-}"
show "q inactive"  "${z0[@]:+A}"
show "q operand"   "${z0[@]:-a b}"
show "q scalar"    "${nope:-}"

echo "### a bad substitution inside an operand names the operand, not the whole"
# The operand is expanded as a word of its own, so it is the word the
# diagnostic reports — bash recurses, and the innermost word in flight wins.
( echo "${nope:-${u@Z}}" ) 2>&1 >/dev/null
( echo ${nope:-pre${u@Z}post} ) 2>&1 >/dev/null
( echo "${z0[@]:-${u@Z}}" ) 2>&1 >/dev/null

echo "### but only these two operators substitute a word"
unset y; show "assign def"  ${y:='a b'}
echo "  y=[$y]"
w=xbx; show "replace"       ${w/b/'a c'}
r=b;   show "replace star"  ${r/b/'*.dat'}
unset z; show "error"       ${z:-'a b'}

echo "### the parameter's own value is never quoted by this"
u='m n'
show "value"      ${u:-'a b'}
show "value plus" ${u:+'a b'}
