# A malformed `-i` value stores nothing, but the name it was aimed at still
# counts as *assigned*. Nothing shows while the name is a scalar — `declare -p`
# reports the same bare declaration either way and `${n+…}` stays empty — but
# the distinction keeps, and it is read out the moment the name becomes an
# array: an assigned one prints `=()` where a never-assigned one prints bare.
#
# Every write aimed at the whole variable marks it, whichever builtin asks.
# A write aimed at an *element* does not, and neither does an array literal,
# whose pairs never reach the table.

echo "=== nothing shows while it is a scalar"
declare -i sc=2+
echo "after"
declare -p sc; echo "rc=$?"
echo "set=[${sc+SET}] val=[$sc]"

echo "=== …until the name becomes an array"
declare -i n1=2+
declare -a n1; declare -p n1
declare -i q1
declare -a q1; declare -p q1

echo "=== whichever write asked"
declare -i n2
n2=2+
declare -a n2; declare -p n2
declare -i n3
n3+=2+
declare -a n3; declare -p n3
declare -i n4
export n4=2+
declare -a n4; declare -p n4
declare -i n5
readonly n5=2+
declare -a n5; declare -p n5
declare -i n6
read n6 <<< '2+'
declare -a n6; declare -p n6
declare -i n7
printf -v n7 '%s' '2+'
declare -a n7; declare -p n7
declare -i t8
declare -n rf8=t8
rf8=2+
declare -a t8; declare -p t8

echo "=== the associative kind reads it the same way"
declare -i s9=2+
declare -A s9; declare -p s9

echo "=== a local carries it, and does not leak out"
declare -ai g=(1)
f() {
local -i g=2+
declare -a g
declare -p g
}
f
declare -p g

echo "=== an element write leaves its array alone"
declare -ai e1
e1[0]=2+
declare -p e1
declare -Ai e2
e2[k]=2+
declare -p e2
declare -ai e3
declare -n re3='e3[0]'
re3=2+
declare -p e3
declare -ai e4
read 'e4[0]' <<< '2+'
declare -p e4

echo "=== …and so does a literal whose pairs never land"
declare -Ai m1
m1=([k]=2+)
declare -p m1
declare -Ai m2
m2+=([k]=2+)
declare -p m2
declare -ai m3
m3=(1 2+)
declare -p m3

echo "=== a whole-array write marks it even when nothing lands"
declare -ai w1
read -a w1 <<< '2+'
declare -p w1
declare -ai w2
mapfile -t w2 <<< '2+'
declare -p w2

echo "=== unset clears it again"
declare -i u1=2+
unset u1
declare -a u1; declare -p u1
