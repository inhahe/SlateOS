# A variable holds a C string, so a NUL cannot be *in* one — but the three ways
# a NUL is kept out are all different, and which one applies depends on where
# the byte was headed. A store truncates at it, reading skips it, and a command
# substitution strips every one and complains. Only `printf` and a file can put
# one in front of the shell in the first place.
printf 'p\0q\n' > n1
printf 'm\0n\nq\n' > n2

echo "=== a store ends at the first NUL"
printf -v v 'a\0b'; echo "${#v} [$v]"
printf -v w '%b' 'x\0y'; echo "${#w} [$w]"
# The cut is the value becoming a C string, which happens before the attributes
# the assignment then applies — so `-i` evaluates `1`, not `1002`, and folding
# sees one letter rather than three.
declare -i n; printf -v n '1\0002'; echo "n=$n"
declare -u u; printf -v u 'a\0b'; echo "u=$u ${#u}"
# There is nothing left to render, so neither `declare -p` nor `@Q` has a NUL
# to spell.
declare -p v; echo "${v@Q}"

echo "=== an array or hash element is stored the same way"
declare -A h; printf -v 'h[k]' 'e\0f'; echo "${#h[k]} [${h[k]}]"
arr=(); printf -v 'arr[0]' 'g\0h'; echo "${#arr[0]} [${arr[0]}]"
declare -p h arr

echo "=== reading skips the byte instead of stopping at it"
{ IFS= read -r r; echo "${#r} [$r]"; } < n1
# …including when the record is measured in characters: the NUL is not one.
{ read -r -N 2 c2; echo "${#c2} [$c2]"; } < n1
{ read -r -n 4 c4; echo "${#c4} [$c4]"; } < n1
# The skip is off when the NUL is the delimiter — it has to end the record then.
{ read -r -d '' d; echo "rc=$? ${#d} [$d]"; } < n1
# A field split from the record has already lost the byte.
{ read -r -a fa; echo "${#fa[@]} [${fa[0]}]"; } < n1
# Input that is nothing but NULs reads as if it were empty: status 1, and the
# name is cleared rather than left alone.
printf '\0\0' > n3
x=PRESET; { read -r x; echo "rc=$? [$x]"; } < n3

echo "=== mapfile keeps the record whole and loses the byte at the store"
mapfile -t a2 < n2; echo "${#a2[@]} ${#a2[0]} [${a2[0]}]"
readarray -t a3 < n2; echo "${#a3[0]}"
# The callback is handed the value that will be stored, so it is cut too.
mapfile -t -C 'printf "cb=[%s]\n"' -c 1 a4 < n2

echo "=== a word never carries one to begin with"
v2=$'c\0d'; echo "${#v2} [$v2]"
# A command substitution drops every NUL, not just the tail after the first, and
# says so on stderr.
s=$(printf 'A\0B\0C'); echo "${#s} [$s]"
