# `${var/pat}` and `${var/pat/}` are the same *expansion* and different *text*.
#
# At expansion time bash cannot tell them apart, and says so twice in the same
# function: `parameter_brace_patsub` sets `rep = NULL` when `skip_to_delim`
# finds no `/` at all, and then again when it finds one with nothing after it
# (subst.c:9158–9167):
#
#   if (lpatsub[delim] == '/') { lpatsub[delim] = 0; rep = lpatsub + delim + 1; }
#   else rep = (char *)NULL;
#   if (rep && *rep == '\0') rep = (char *)NULL;
#
# So both delete the match. The *printback* is a different faculty: bash does
# not reconstruct a word from a parse tree, it re-prints the source text it
# saved, so the separator appears under `declare -f` exactly when it was
# written. A shell that folds "absent" into "empty" while parsing has no way to
# put that back, and grows a `/` the script never had.
#
# The distinction survives every shape the operator has — `/`, `//`, `/#`, `/%`
# — and the `${a[@]…}` bulk forms, which are a separate operator node.
#
# Verified against bash 5.2.37.

q=zabz
s=zaaaz
sl='a/b'
a=(zabz zabz)

echo "=== the two spellings expand alike ==="
p() { printf '%-24s -> [%s]\n' "$1" "$2"; }
p '/ no separator'    "${q/ab}"
p '/ empty after it'  "${q/ab/}"
p '// no separator'   "${q//ab}"
p '// empty after it' "${q//ab/}"
p '/# no separator'   "${q/#za}"
p '/# empty after it' "${q/#za/}"
p '/% no separator'   "${q/%bz}"
p '/% empty after it' "${q/%bz/}"
# An empty pattern is not an absent one either: it matches at every position,
# so a global replacement with an empty replacement is the identity.
p 'empty pattern'     "${q/}"
p 'empty, global'     "${q//}"
p 'empty, anchored'   "${q/#}"

echo "=== including through the bulk operator ==="
printf '[%s]' "${a[@]/ab}"; echo
printf '[%s]' "${a[@]/ab/}"; echo
printf '[%s]' "${a[*]//ab}"; echo
printf '[%s]' "${a[@]/#za}"; echo

echo "=== but they do not print alike ==="
f() {
  echo "${q/ab}" "${q/ab/}" "${q//ab}" "${q//ab/}"
  echo "${q/#ab}" "${q/%ab}" "${q/#ab/}" "${q/%ab/}"
  echo "${q/}" "${q//}" "${q/#}" "${q/%}"
  echo "${a[@]/ab}" "${a[@]/ab/}" "${a[*]//ab}"
  echo "${q[0]/ab}"
}
declare -f f

echo "=== a separator inside a construct is still not one ==="
# The pattern reaches the end of the body, so there is no replacement here
# either — the `/` belongs to the `$( … )` and to the `${ … }`.
g() { echo "${s/$(echo a/b)}" "${s/${sl}}" "${s/`echo a/b`}" "${s/$((4/2))}"; }
declare -f g
p 'command substitution' "${s/$(echo a/b)aaa}"
p 'brace expansion'      "${s/${sl}aaa}"

echo "=== and the replacement half still expands when it is there ==="
h() { echo "${q/ab/X}" "${q//ab/X}" "${q/#za/X}"; }
declare -f h
p 'a replacement given' "${q/ab/X}"

echo done
