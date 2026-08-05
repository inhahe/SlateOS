# `declare -n r='arr[1]'` makes `r` a name for one *element*. Reading it plainly
# has always given that element; the point of this case is that **every other**
# form that reads a parameter's value gives it too — the trims, the case and
# substitution operators, the slices, the `@`-transforms, and the `:-`/`:+`/`:?`
# family, which decide "set or unset" by the same read.
#
# Two answers cut the other way and are the reason this is worth pinning down:
#
#   * `${#r}` is **0**, not the length of the element. bash's length path asks
#     for a *variable* named `arr[1]`, finds none, and counts nothing — for every
#     element reference, indexed or associative, set or unset. So `${r}` is
#     `c d` while `${#r}` is 0, in the same shell, on the same name.
#   * `${r[@]}` is empty and `${!r}` is the reference's own text. Subscripting a
#     reference that already carries a subscript is one subscript too many, and
#     there is no array left to enumerate.
#
# A reference may also name a **whole** array — `declare -n g='n[*]'` or
# `'n[@]'` — and then reading it gives the elements joined with `$IFS`'s first
# character, for *both* spellings: what the reference names is a string that has
# already been through the join by the time any reader sees it, so `[@]` does not
# get its usual space-joined treatment and a null `$IFS` runs the elements
# together. Every operator above then works on that joined string.

declare -a n=('abc' 'c d' e)
declare -A m=([k]='K K' [j]=J)
declare -n E='n[1]'
declare -n Z='n[0]'
declare -n V='n[9]'
declare -n MK='m[k]'
declare -n W='n'
declare -n g='n[*]'
declare -n G='n[@]'
declare -n M='m[*]'

p() { printf '%-24s ' "$1"; shift; printf '<%s>' "$@"; echo; }
IFS=:

echo "=== an element reference reads as its element, whatever asks"
p '${E}'        "${E}"
p '${E#c}'      "${E#c}"
p '${E%d}'      "${E%d}"
p '${E^^}'      "${E^^}"
p '${E,,}'      "${E,,}"
p '${E/ /-}'    "${E/ /-}"
p '${E//[cd]/X}' "${E//[cd]/X}"
p '${E:1}'      "${E:1}"
p '${E:0:2}'    "${E:0:2}"
p '${E@Q}'      "${E@Q}"
p '${E@U}'      "${E@U}"
p '${E@E}'      "${E@E}"
p '${E@P}'      "${E@P}"
p '${E:-D}'     "${E:-D}"
p '${E:+Y}'     "${E:+Y}"
p '${E+Y}'      "${E+Y}"
p '${E?msg}'    "${E?msg}"
p '${Z}'        "${Z}"
p '${Z#a}'      "${Z#a}"
p '${MK}'       "${MK}"
p '${MK#K}'     "${MK#K}"
p '${MK@Q}'     "${MK@Q}"

echo "=== …but the length of one is zero, and it names no array"
p '${#E}'       "${#E}"
p '${#Z}'       "${#Z}"
p '${#MK}'      "${#MK}"
p '${#V}'       "${#V}"
p '${#W}'       "${#W}"
p '${E[@]}'     "${E[@]}"
p '${E[0]}'     "${E[0]}"
p '${!E}'       "${!E}"
p '${!MK}'      "${!MK}"

echo "=== an unset element is unset, and the defaults fire"
p '${V-D}'      "${V-D}"
p '${V:-D}'     "${V:-D}"
p '${V+Y}'      "${V+Y}"

echo "=== :=  never fires while the element is there"
declare -n Q='n[1]'
p '${Q:=ZZ}'    "${Q:=ZZ}"
p 'n after'     "${n[@]}"

echo "=== a reference to a whole array reads as the join"
p '"${g}"'      "${g}"
p '${g}'        ${g}
p '"${G}"'      "${G}"
p '${G}'        ${G}
p '"${M}"'      "${M}"
p '${g#a}'      "${g#a}"
p '${G#a}'      "${G#a}"
p '${g@Q}'      "${g@Q}"
p '${G@Q}'      "${G@Q}"
p '${g^^}'      "${g^^}"
p '${g/:/-}'    "${g/:/-}"
p '${g//:/-}'   "${g//:/-}"
p '${g:0:3}'    "${g:0:3}"
p '${g:5}'      "${g:5}"
p '${g:-D}'     "${g:-D}"
p '${#g}'       "${#g}"
p '${#G}'       "${#G}"
p '${#M}'       "${#M}"
p '${g[@]}'     "${g[@]}"
p '${g[*]}'     "${g[*]}"
p '${!g}'       "${!g}"
p '${!G}'       "${!G}"
p '${!M}'       "${!M}"

echo "=== the join is \$IFS's first character, for both spellings"
( IFS=,  ; p 'IFS=,'      "${g}" "${G}" )
( IFS=   ; p 'IFS=null'   "${g}" "${G}" )
( unset IFS; p 'IFS unset' "${g}" "${G}" )
( IFS=': '; p 'IFS=": "'  "${g}" "${G}" )

echo "=== the empty array, and a reference to a scalar's element"
declare -a q=()
declare -n QS='q[*]'
p '${QS}'       "${QS}"
p '${QS:-D}'    "${QS:-D}"
p '${#QS}'      "${#QS}"
s=plain
declare -n SE='s[0]'
p '${SE}'       "${SE}"
p '${SE#p}'     "${SE#p}"

echo "=== arithmetic subscripts instead of joining, so a whole array is a bad one"
declare -a k=(3 5 7)
declare -A ka=([q]=9)
declare -n KE='k[1]'
declare -n KS='k[*]'
declare -n KA='k[@]'
declare -n KM='ka[*]'
echo "elem:  $((KE))"
echo "star:  $((KS))"
echo "at:    $((KA))"
echo "assoc: $((KM))"

echo "=== set -u sees an unset element as unbound"
( set -u; declare -n U='miss[0]'; echo "plain <${U}>"; ) 2>&1 | head -1
( set -u; declare -n U='miss[0]'; echo "trim <${U#x}>"; ) 2>&1 | head -1
( set -u; echo "len <${V}>"; ) 2>&1 | head -1
