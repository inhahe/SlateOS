# A compound array assignment is not one indivisible act. It has three stages,
# and something can go wrong between them:
#
#   1. the whole literal is expanded — so the values still see the *old* array;
#   2. a non-append literal clears the variable;
#   3. each element is bound in turn, and an `-i` value is evaluated as it goes.
#
# Only stage 3 can fail (a bad arithmetic value under the integer attribute), and
# because it fails part-way through, what survives is the clearing plus the
# elements bound before the failure. The one exception is a non-append
# *associative* literal, which bash builds off to the side and swaps in — so that
# one is all-or-nothing.
#
# Every failing probe is followed by its reader on the *next* line: a failed
# integer assignment is fatal to the list it is in, so a reader after a `;` would
# never run at all.
#
# The associative probes name the keys they expect, and — bash listing keys in
# its hash order, which osh shares — also print `${!m[*]}` wherever an array ends
# up with more than one, so a partial append reports *which* keys survived and
# not merely that the named one did.

echo "=== stage 1: the values read the array being replaced ==="
a=(1 2)
a=(9 ${a[0]})
echo "self=[${a[*]}]"
declare -ia b=(5 6)
b=(1 ${b[1]} 3)
echo "self-int=[${b[*]}]"
declare -A h=([k]=old)
h=([n]=${h[k]})
echo "assoc-self=[${h[n]}]"
# The *subscript* of a keyed element is expanded in that same first stage, so it
# counts the old elements too.
c=(7 7 7)
c=([${#c[@]}]=x)
echo "old-count=[${!c[*]}]"

echo "=== stage 3: an indexed literal keeps what it bound ==="
declare -ia d=(5 6)
d=(1 1/0 3); echo "unreachable-indexed"
echo "indexed=[${d[*]}] keys=[${!d[*]}]"
# The clearing (stage 2) survives with it: the old 7s are gone even though the
# literal never finished.
declare -ia e=(7 7)
e=([2]=1 [3]=1/0 [4]=5); echo "unreachable-keyed"
echo "keyed=[${e[*]}] keys=[${!e[*]}]"
# `+=` binds in place as well, so the append that got through stays.
declare -ia f=(7 7 7)
f+=(1 1/0 3); echo "unreachable-append"
echo "append=[${f[*]}]"

echo "=== stage 3: a non-append associative literal is all-or-nothing ==="
# Nothing is cleared and nothing is bound — `g` is exactly as it was.
declare -iA g=([a]=9)
g=([x]=1 [y]=1/0); echo "unreachable-assoc"
echo "assoc-old=[${g[a]}] assoc-new=[${g[x]-absent}]"
# But `+=` on the same array binds in place, so the first element stays.
declare -iA i=([a]=9)
i+=([x]=1 [y]=1/0); echo "unreachable-assoc-append"
echo "append-old=[${i[a]}] append-new=[${i[x]-absent}] keys=[${!i[*]}]"

echo "=== a literal that does not fail is unaffected by any of this ==="
declare -ia ok=(9 9)
ok=(1+1 2+2 3+3)
echo "ok=[${ok[*]}] rc=$?"
declare -iA okm=([z]=9)
okm=([p]=1+1 [q]=2+2)
echo "okm=[${okm[p]} ${okm[q]}] z=[${okm[z]-absent}] keys=[${!okm[*]}] rc=$?"
# The empty literal still clears, in both flavours.
declare -a emp=(1 2 3)
emp=()
echo "empty=[${emp[*]}] n=${#emp[@]}"
declare -A empm=([k]=v)
empm=()
echo "empty-assoc=[${empm[k]-absent}] n=${#empm[@]}"

echo done
