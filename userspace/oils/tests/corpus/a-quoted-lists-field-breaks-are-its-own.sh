# A `[@]`-spelled list inside `"…"` breaks a field between each pair of adjacent
# elements, and nothing else sharing the quotes can close those breaks up. Text
# before it joins its first element, text after joins its last, a second list
# starts counting from wherever the first one stopped — and an expansion that
# contributed no characters cannot glue two elements together, because the break
# was never the text's to make.
#
# Two things are *not* this rule:
#
#   * A `[*]` in the same quotes is text, so it glues onto the last field like
#     any other text would. Only the `[@]` spelling breaks.
#   * A list that yields no elements contributes nothing at all, so the field it
#     shares is the one the rest of the quotes made — `"${z0[@]}Z"` is `<Z>`,
#     one field, where the empty quoted run `""` is one *empty* field.

show() { printf '  %-24s(%d)' "$1" $(($# - 1)); shift; printf '<%s>' "$@"; printf '\n'; }

a=('p q' r)
z0=()
ee=('' '')
one=(s)
e=''
set -- 'x y' z

echo "### text glues to the end it touches, and the break stays"
show 'at then text'   "${a[@]}Z"
show 'text then at'   "Z${a[@]}"
show 'text both ends'  "Z${a[@]}Z"
show 'one element'    "${one[@]}Z"

echo "### a second list starts where the first one stopped"
show 'at at'          "${a[@]}${a[@]}"
show 'at text at'     "${a[@]}Z${a[@]}"

echo "### an expansion with nothing in it still cannot join two elements"
show 'at unset'       "${a[@]}${nope}"
show 'unset at'       "${nope}${a[@]}"
show 'at empty var'   "${a[@]}$e"
show 'at empty array' "${a[@]}${z0[@]}"

echo "### a [*] beside it is text, and glues"
show 'at star'        "${a[@]}${a[*]}"
show 'star at'        "${a[*]}${a[@]}"

echo "### every spelling that makes a list answers the same way"
show 'positionals'    "$@Z"
show 'text then pos'  "Z$@"
show 'keys'           "${!a[@]}Z"
show 'slice'          "${a[@]:0:2}Z"
show 'bulk'           "${a[@]^^}Z"
show 'operator'       "${a[@]:-d}Z"
show 'operand'        "${nope:-"${a[@]}"}Z"

echo "### an empty list contributes nothing; an empty run is still a field"
show 'empty at text'  "${z0[@]}Z"
show 'text empty at'  "Z${z0[@]}"
show 'text both'      "Z${z0[@]}Z"
show 'empty operator' "${z0[@]:-d}Z"
show 'empty run'      ""
show 'empty run glued' Z""Z
show 'empty scalar'   "${nope}"

echo "### an empty element is a field, and the text lands past it"
show 'nulls then text' "${ee[@]}Z"
show 'text then nulls' "Z${ee[@]}"
show 'nulls operator'  "${ee[@]:-d}Z"

echo "### \$IFS has no say in any of it"
IFS=:
show 'ifs at then text' "${a[@]}Z"
show 'ifs at star'      "${a[@]}${a[*]}"
IFS=
show 'null at then text' "${a[@]}Z"
show 'null at star'      "${a[@]}${a[*]}"
unset IFS

echo "### and a string context joins the lot, as it always did"
s="${a[@]}Z";      echo "  after:  [$s]"
s="Z${a[@]}";      echo "  before: [$s]"
s="${a[@]}${a[@]}"; echo "  twice:  [$s]"
