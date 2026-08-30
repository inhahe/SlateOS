# Associative arrays (`declare -A`) in depth. The corpus touches them in
# arrays.sh and declare-attrs.sh; this case is about the places where they
# behave *differently from indexed arrays*, which is where the bugs live.
#
# Iteration order is hash-dependent and not part of the contract, so anything
# order-sensitive here is sorted before printing.

sorted() { printf '%s\n' "$@" | sort | tr '\n' ' '; echo; }

declare -A m
m[alpha]=1
m[beta]=2
m[gamma]=3
echo "n=${#m[@]}"
sorted "${!m[@]}"
sorted "${m[@]}"

# THE defining difference from an indexed array: a subscript is a *string key*,
# not an arithmetic expression. `a[1+1]` is index 2 in an indexed array but the
# literal three-character key `1+1` in an associative one.
declare -a idx
idx[1+1]=indexed
echo "idx2=${idx[2]} idx-n=${#idx[@]}"
declare -A ar
ar[1+1]=assoc
echo "ar-literal=${ar[1+1]} ar-two=${ar[2]-unset} ar-n=${#ar[@]}"
# Likewise a bare name is a variable reference in an indexed subscript but a
# plain key in an associative one.
k=beta
declare -a i2; i2[k]=viaK      # k=0 numerically -> index 0
declare -A a2; a2[k]=literalK  # the key is the letter k
echo "i2-0=${i2[0]} a2-k=${a2[k]} a2-beta=${a2[beta]-unset}"
echo "expand-k=${m[$k]}"

# Keys may contain anything, including spaces, glob metacharacters, `]`, and
# leading/trailing whitespace. Quoting the subscript is what keeps them intact.
declare -A odd
odd['two words']=w
odd['*']=star
odd['a]b']=bracket
odd['']=empty
odd[' pad ']=padded
echo "w=${odd['two words']} star=${odd['*']} br=${odd['a]b']} e=${odd['']} pad=[${odd[' pad ']}]"
echo "odd-n=${#odd[@]}"
# `*` and `@` as literal keys are shadowed by the all-elements forms, so they
# are only reachable through a quoted subscript.
echo "star-again=${odd[\*]}"

# Existence vs emptiness: `${m[k]+set}` and `[[ -v m[k] ]]` see a key whose
# value is the empty string; `${m[k]:+…}` does not.
declare -A e
e[present]=''
echo "plus=${e[present]+set} colonplus=${e[present]:+set}(end)"
[[ -v e[present] ]] && echo "v-present=yes" || echo "v-present=no"
[[ -v e[missing] ]] && echo "v-missing=yes" || echo "v-missing=no"
echo "default=${e[missing]:-fallback}"

# Compound assignment with explicit keys, and `+=` which *merges* rather than
# replacing (contrast with a plain `=`, which clears the array first).
declare -A c=([one]=1 [two]=2)
sorted "${!c[@]}"
c+=([three]=3 [one]=uno)
sorted "${!c[@]}"
echo "one=${c[one]} three=${c[three]} n=${#c[@]}"
c=([only]=x)
sorted "${!c[@]}"
echo "after-plain-assign n=${#c[@]}"

# `+=` on an *element* appends to that element's string, as for any variable.
declare -A s=([k]=abc)
s[k]+=def
echo "elem-append=${s[k]}"

# Unsetting: a single key, then the whole array. The subscript must be quoted
# or the shell may glob it against filenames.
declare -A u=([x]=1 [y]=2 [z]=3)
unset 'u[y]'
sorted "${!u[@]}"
echo "u-n=${#u[@]}"
unset u
echo "u-gone=${#u[@]} val=${u[x]-unset}"

# `${m[@]}` vs `${m[*]}`: the same join rules as indexed arrays. `[*]` inside
# double quotes joins with the first character of IFS.
declare -A j=([a]=1 [b]=2)
oldifs=$IFS
IFS=-
joined="${j[*]}"
IFS=$oldifs
# Two elements, unknown order — normalise by sorting the pieces.
echo "join-len=${#joined} sep-count=$(printf '%s' "$joined" | tr -cd - | wc -c)"
sorted "${j[@]}"

# An associative array is not a list, so slicing and the indexed-array idioms
# that depend on position are meaningless. `${m[@]:0:1}` still yields *some*
# single element (order-dependent), so only its count is stable.
declare -A sl=([p]=1 [q]=2 [r]=3)
set -- "${sl[@]:0:2}"
echo "slice-count=$#"
set --

# A compound assignment has *two modes*, and the first element chooses which.
# If the first element carries no subscript, the whole list is consumed as
# alternating key/value words — so a bare list is not an error at all, it is
# pairwise. A trailing odd word gets the empty string.
declare -A bad=([keep]=yes)
bad=(1 2 3)
echo "bad-keep=${bad[keep]-gone} bad-n=${#bad[@]}"
sorted "${!bad[@]}"
echo "pair-1=${bad[1]} pair-3=[${bad[3]}]"
# In pair mode a later `[k]=v` is *not* interpreted — it becomes a literal key.
declare -A pm=(x 1 [y]=2)
sorted "${!pm[@]}"
echo "literal-key=${pm['[y]=2']-missing}"
# If the first element *is* subscripted, the other mode applies and a bare word
# is rejected, naming the offending word — but the rest of the list still lands
# and the status stays 0. The word is quoted for a `declare` operand (which the
# builtin re-parses) and bare for a plain compound assignment.
{ declare -A km=([a]=1 loose [b]=2); } 2>&1
echo "km-status=$? a=${km[a]-unset} b=${km[b]-unset} n=${#km[@]}"
declare -A km2
{ km2=([a]=1 loose [b]=2); } 2>&1
echo "km2-status=$? n=${#km2[@]}"

# `declare -p` round-trips. With a single key the output is order-independent.
declare -A one=([solo]=v)
declare -p one

# The -A attribute is sticky: re-declaring does not convert or clear, and
# `declare -A` on an existing *indexed* array is an error.
declare -A st=([a]=1)
declare -A st
echo "sticky=${st[a]} n=${#st[@]}"
declare -a conv=(x y)
{ declare -A conv; } 2>&1
echo "convert-status=$? conv0=${conv[0]-unset} n=${#conv[@]}"
# …and symmetrically. A rejected conversion leaves the array untouched, so the
# values are never silently orphaned behind an empty map of the other kind.
declare -A rev=([k]=v)
{ declare -a rev; } 2>&1
echo "revert-status=$? revk=${rev[k]-unset} n=${#rev[@]}"
# A compound assignment of the wrong kind is refused the same way.
{ declare -A conv=([k]=v); } 2>&1
echo "conv-lit-status=$? conv0=${conv[0]-unset}"

# An empty subscript has no representation in an associative array. Writing one
# reports the reference in *source* form and creates nothing; the failed
# assignment abandons the rest of its logical line, so `$?` is read on the next.
declare -A es=([k]=v)
{ es['']=x; } 2>&1
echo "es-status=$? es-n=${#es[@]}"
# The read is softer: the value form names the *base* and expands empty (0).
{ echo "es-read=[${es['']}]"; } 2>&1
echo "es-read-status=$?"

# Locality: `local -A` inside a function shadows a global of the same name and
# is discarded on return.
declare -A g=([scope]=global)
f() {
    local -A g=([scope]=local)
    echo "in-fn=${g[scope]} n=${#g[@]}"
}
f
echo "after-fn=${g[scope]}"

# Iterating keys to build a deterministic dump — the idiomatic way to print an
# associative array reproducibly.
declare -A d=([kiwi]=green [fig]=purple [date]=brown)
for key in $(printf '%s\n' "${!d[@]}" | sort); do
    printf '%s=%s ' "$key" "${d[$key]}"
done
echo

# A key that looks numeric is still a string key, and does not create the
# intervening "elements" an indexed array would.
declare -A num=([10]=ten [2]=two)
echo "num-n=${#num[@]} ten=${num[10]} two=${num[2]}"
sorted "${!num[@]}"
