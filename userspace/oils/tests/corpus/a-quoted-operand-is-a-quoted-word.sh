# The quotes around a `"${x:-w}"` reach the operand: it is a word, and it is
# expanded as a *double-quoted* one. So everything a quoted word does, it does —
# and nothing a bare one does. A `[@]` inside it keeps one field per element,
# **whether or not it is itself quoted**, because the quoting that decides that
# is the substitution's; a `[*]` joins with `$IFS` for the same reason; and no
# text in it ever splits, under any `$IFS`.
#
# This is the quoted half of the rule
# `a-substituted-operand-is-the-word-it-was-written-as.sh` covers unquoted,
# where the operand's characters are handed to the enclosing word's own scan.
# Here there is no scan to hand them to, so the only breaks are the ones a list
# brings with it.
#
# Two things that are *not* this rule:
#
#   * The `[*]` spelling of the outer operator joins its **elements**, never its
#     operand — `"${z[*]:-"${a[@]}"}"` is two fields, because the star found no
#     elements to join and an operand is a word.
#   * A scalar substitution is one field however little it holds, so an
#     inactive `"${nope:+A}"` is one empty argument where the `[@]` spelling of
#     the same is none at all.

show() { printf '  %-24s(%d)' "$1" $(($# - 1)); shift; printf '<%s>' "$@"; printf '\n'; }

a=('p q' r)
z0=()
ee=('' '')
e=''
v=set

echo "### a [@] in the operand makes fields, quoted in it or not"
show 'q at'          "${nope:-"${a[@]}"}"
show 'unq at'        "${nope:-${a[@]}}"
show 'array q at'    "${z0[@]:-"${a[@]}"}"
show 'array unq at'  "${z0[@]:-${a[@]}}"
show 'alt q at'      "${v:+"${a[@]}"}"
show 'alt unq at'    "${v:+${a[@]}}"
show 'colon-less'    "${nope-"${a[@]}"}"

echo "### and a [*] joins with \$IFS, quoted in it or not"
show 'q star'        "${nope:-"${a[*]}"}"
show 'unq star'      "${nope:-${a[*]}}"

echo "### the derived lists answer the same way"
show 'keys'          "${nope:-${!a[@]}}"
show 'keys star'     "${nope:-${!a[*]}}"
show 'slice'         "${nope:-${a[@]:0:2}}"
show 'slice star'    "${nope:-${a[*]:0:2}}"
show 'bulk'          "${nope:-${a[@]^^}}"

echo "### nothing else splits, whatever \$IFS says"
IFS=:
show 'ifs q at'      "${nope:-"${a[@]}"}"
show 'ifs unq at'    "${nope:-${a[@]}}"
show 'ifs star'      "${nope:-${a[*]}}"
show 'ifs lit colon' "${nope:-a:b}"
show 'ifs lit space' "${nope:-a b}"
show 'ifs keys'      "${nope:-${!a[@]}}"
show 'ifs slice'     "${nope:-${a[@]:0:2}}"
IFS=
show 'null q at'     "${nope:-"${a[@]}"}"
show 'null star'     "${nope:-${a[*]}}"
unset IFS

echo "### an empty list contributes nothing, but the word is still a field"
show 'empty at'      "${nope:-"${z0[@]}"}"
show 'empty glued'   "${nope:-A"${z0[@]}"B}"
show 'empty star'    "${nope:-"${z0[*]}"}"
show 'array empty'   "${z0[@]:-"${z0[@]}"}"
# Quoted, an empty *element* is a field like any other — that is what quoting a
# list means, and the operand inherits it.
show 'nulls'         "${nope:-"${ee[@]}"}"
show 'nulls unq'     "${nope:-${ee[@]}}"

echo "### text glues to the ends, inside the operand and outside it"
show 'A at B'        "${nope:-A"${a[@]}"B}"
show 'outer glue'    X"${nope:-"${a[@]}"}"Y
show 'both'          X"${nope:-A"${a[@]}"B}"Y

echo "### the star spelling of the operator joins elements, never the operand"
show 'star, operand' "${z0[*]:-"${a[@]}"}"
show 'star, elems'   "${a[*]:-d}"
show 'at, elems'     "${a[@]:-d}"
IFS=:
show 'ifs star elems' "${a[*]:-d}"
show 'ifs star opnd'  "${z0[*]:-"${a[@]}"}"
unset IFS

echo "### a scalar is one field however little it holds"
show 'scalar empty'  "${nope:-}"
show 'scalar alt'    "${nope:+A}"
show 'scalar value'  "${e:-d}"
show 'array empty'   "${z0[@]:-}"
show 'array alt'     "${z0[@]:+A}"

echo "### nested one level deeper, and the quotes still reach"
show 'nested q at'   "${nope:-${nope2:-"${a[@]}"}}"
show 'nested unq at' "${nope:-${nope2:-${a[@]}}}"
show 'nested star'   "${nope:-${nope2:-${a[*]}}}"

echo "### a string context joins whatever it was handed, with a space"
IFS=:
s="${nope:-"${a[@]}"}";   echo "  assign q at:   [$s]"
s=${z0[*]:-A"${a[@]}"B};  echo "  assign star:   [$s]"
s=${z0[@]:-A"${a[@]}"B};  echo "  assign at:     [$s]"
s=${a[*]:-d};             echo "  assign elems:  [$s]"
unset IFS
