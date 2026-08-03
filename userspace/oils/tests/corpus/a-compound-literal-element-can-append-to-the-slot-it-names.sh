# An element of an array literal can be written `[k]+=v`, and then it does not
# replace the slot it names — it concatenates onto whatever is there *at the
# moment the element is bound*. Which value that is is not a special rule; it
# falls out of where each kind of literal puts its bindings.
#
#   * An **indexed** literal writes straight into the array, after the clear a
#     non-append literal does first. So `n=(A B); n=([0]+=x)` is `x` (the clear
#     already happened), `n+=([0]+=x)` is `Ax`, and elements earlier in the same
#     literal are visible: `n=(p q [0]+=Z)` is `pZ`.
#   * An **associative** non-append literal builds its pairs off to the side and
#     swaps them in at the end, so what `+=` reads is the *old* table — the
#     pending pairs are invisible to it, and the last pending write for a key is
#     the one that survives. `declare -A m=([a]=1 [a]+=x [a]+=y)` is therefore
#     `y`, not `1xy`.
#   * An associative *append* literal (`m+=(…)`) writes live, so there its
#     elements do accumulate.
#
# Under `-i` the append adds instead of concatenating, as it does everywhere
# else. In pair mode a `[k]+=v` element is not special at all — like `[k]=v`
# there, it is just the literal key `[k]+=v`.

echo "=== an indexed literal binds into the array itself"
declare -a n=(A B)
n=([0]+=x)
declare -p n
declare -a n2=(A B)
n2+=([0]+=x)
declare -p n2
declare -a n3=([0]=1 [0]+=x [0]+=y)
declare -p n3
declare -a n4
n4=(p q [0]+=Z)
declare -p n4

echo "=== an associative non-append literal reads the old table"
declare -A m=([a]=Q)
m=([a]=1 [a]+=x)
declare -p m
declare -A m2=([a]=1 [a]+=x [a]+=y)
declare -p m2
declare -A m3=([a]+=x [a]+=y [a]=Z)
declare -p m3

echo "=== an associative append literal writes live"
declare -A p=([a]=1)
p+=([a]+=x [a]+=y)
declare -p p

echo "=== -i adds instead of concatenating"
declare -ai i1=(1 2)
i1+=([0]+=3)
declare -p i1
declare -Ai i2=([a]=5)
i2+=([a]+=3)
declare -p i2

echo "=== a scalar widened by an append literal is element 0"
w=plain
w+=([0]+=x)
declare -p w

echo "=== pair mode makes it an ordinary key"
declare -A pm=(k v [k]+=Z)
echo "  n=${#pm[@]} k=[${pm[k]}] lit=[${pm['[k]+=Z']}]"

echo "=== the trace keeps the += spelling"
set -x
declare -a t=([0]+=x [1]=y)
declare -A t2=([a]+=x [b]=y)
set +x
echo "=== done"
