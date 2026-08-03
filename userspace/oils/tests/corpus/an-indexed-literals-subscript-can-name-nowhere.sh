# A `[sub]=v` element of an *indexed* literal can name a slot the array has no
# room for, and then bash refuses just that element. Two ways it can:
#
#   * The subscript expands to **nothing**. That is not index 0 — which is what
#     makes it worth saying, because two things that look just as empty *are*
#     index 0: whitespace (`[ ]=v`) and a name arithmetic reads as unset
#     (`[a]=v`) both evaluate to 0 the ordinary way.
#   * It is **negative and reaches back past the start**. A negative subscript
#     is not itself an error — it counts back from one past the highest index
#     the array holds *at that moment*, exactly as `a[-1]=v` does outside a
#     literal. So the clear a non-append literal does first, and every element
#     bound before this one, both count. And because it counts from the highest
#     *index* rather than from the element count, a sparse array answers
#     differently than its length would suggest.
#
# The complaint names the element's **expanded** halves, unquoted, and keeps the
# `+=` spelling — the same way whether the literal is a `declare` operand or a
# bare assignment, where an associative literal distinguishes those two. It is
# not fatal: the element is dropped without consuming the running index, every
# other element still binds, and the status stays 0. The subscript is settled
# before the value is looked at, so a refused element's value is never even
# evaluated.

echo "=== an empty subscript is not index 0"
declare -a n=([]=v)
declare -p n
declare -a n2=([""]=v)
declare -p n2
e=; v=VV
declare -a n3=([$e]=$v)
declare -p n3
declare -a n4
n4=([$e]+=v)
declare -p n4

echo "=== but whitespace and an unset name are"
declare -a w=([ ]=v)
declare -p w
declare -a u=([a]=v)
declare -p u

echo "=== a negative subscript counts back from the highest index"
declare -a a=(p q r)
a+=([-1]=Z)
declare -p a
declare -a b=(p q r)
b=(x y [-1]=Z)
declare -p b
declare -a s=([0]=p [9]=q)
s+=([-1]=Z)
declare -p s
declare -a s2=([0]=p [9]=q)
s2+=([-2]=Z)
declare -p s2
declare -a h=(p q)
h+=([-1]+=Z)
declare -p h

echo "=== reaching past the start has nowhere to live"
declare -a c=(p q)
c+=([-3]=Z)
declare -p c
declare -a g=()
g+=([-1]=Z)
declare -p g
declare -a m=([1-3]=y)
declare -p m

echo "=== the element is dropped, the rest binds, the status stays 0"
declare -a k=(p [-5]=y q)
declare -p k
echo "  s=$?"
declare -a k2=(p [5]=q []=r s)
declare -p k2
declare -a k3=([-1]=x [ ]=y []=z w)
declare -p k3

echo "=== the subscript is settled before the value"
declare -ai i1=([-5]=1/0)
declare -p i1
declare -ai i2=([-1]=1/0 2+2)
declare -p i2

echo "=== a bare literal names it the same way a declare operand does"
declare -a p1
p1+=([]=v)
f() { local -a l=([]=v); declare -p l; }
f

echo "=== the operand is traced before the refusal"
set -x
declare -a t=(p []=v [-1]=w q)
set +x
declare -p t
echo "=== done"
