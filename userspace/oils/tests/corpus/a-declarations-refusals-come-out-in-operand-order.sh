# bash has one loop over a declaration builtin's operands, so the refusals it
# raises against them come out in the order they were *written* — and a
# compound `name=(…)` operand takes its turn in that order like any other, even
# though the literal itself was bound earlier, back during word expansion.
#
# That is the whole of this case: every line below mixes the two kinds of
# operand and cares only about which name is named first. The refusals
# themselves (`cannot convert associative to indexed array`, `cannot destroy
# array variables in this way`, `reference variable cannot be an array`, and the
# doubly-reported local ones) are each pinned elsewhere.
#
# Note the local refusals report *twice* over: once from the compound-assignment
# machinery as the word is expanded — so all of those come first, in operand
# order — and once from the builtin as it reaches the operand, which is the
# order this case is about.

echo "=== -aA: a compound and a scalar, compound written first"
declare -aA m11=(1) m12=zz
echo "  rc=$?"

echo "=== and scalar written first"
declare -aA m21=zz m22=(1)
echo "  rc=$?"

echo "=== interleaved, four of them"
declare -aA i1=(1) i2=zz i3=(2) i4=ww
echo "  rc=$?"

echo "=== a flag written between them does not move anything"
declare -aA j1=(1) j2=zz
echo "  rc=$?"

echo "=== +a cannot destroy, either way round"
declare -a d1=(1) d2=(2)
declare +a d1=(9) d2
echo "  compound first: rc=$?"
declare -a e1=(1) e2=(2)
declare +a e2 e1=(9)
echo "  scalar first:   rc=$?"

echo "=== -n against an array, either way round"
declare -a q1=(1)
declare -n n1=(5) q1
echo "  compound first: rc=$?"
declare -a q2=(1)
declare -n q2 n2=(5)
echo "  scalar first:   rc=$?"

echo "=== the local refusals: machinery halves first, then the builtin's"
readonly ro1=1 ro2=2
f() { local ro1=(1) ro2=zz; }
f; echo "  compound first: rc=$?"
g() { local ro2=zz ro1=(1); }
g; echo "  scalar first:   rc=$?"

echo "=== an invalid option stops before any operand, so only the machinery spoke"
h() { local -Z ro1=(1); echo "  rc=$?"; }
h

echo "=== and what survived"
declare -p m11 m12 i1 i3 2>&1
