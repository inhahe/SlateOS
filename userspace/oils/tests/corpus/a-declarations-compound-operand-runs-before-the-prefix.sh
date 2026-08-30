# `declare -a x=(1 2)` is not one command but two halves, and the command's own
# temporary assignments sit between them.
#
# Only a compound operand can bind a whole array — there is no argv string that
# hands one to a builtin — so bash binds it in place while the words are being
# expanded. A *scalar* operand is merely expanded to a `name=value` string there
# and left for the builtin. The prefix assignments belong to the builtin's half:
# they are expanded after the word-expansion pass is over.
#
# So the compound operand goes first in every sense — its trace line, its command
# substitutions, and the assignments its `${u=…}` makes — and a prefix assignment
# expanded afterwards already sees the array the operand bound. What the operand
# does *not* see is the prefix, in either direction: it expands before the prefix
# value does, and the prefix binding is temporary anyway.
#
# `-g` retargets the operand at the global binding without leaking into the
# prefix, which still reads the local it shadows: the two halves take the global
# scope separately.

echo "=== the operand's trace comes before the prefix's"
set -x
P=1 Q=2 declare -a m=(a) n=(b)
set +x

echo "=== and so do its side effects"
A=$(echo pre >&2; echo a) declare -a x=($(echo op >&2; echo o))
declare -p x

echo "=== an assignment the operand makes is the one the prefix reads"
A=${u=FROMPREFIX} declare -a z=(${u=FROMOP})
declare -p z
echo "u=$u"

echo "=== the operand does not see the prefix"
v=old
v=new declare -a w=($v)
declare -p w
echo "v=$v"

echo "=== but the prefix sees what the operand bound"
y=(old)
Y=$(echo "prefix-sees=${y[0]}" >&2) declare -a y=(new)
echo "after=${y[0]}"

echo "=== -g does not leak into the prefix"
f() {
  local g=(loc)
  G=$(echo "prefix-sees=${g[0]}" >&2) declare -ga g=(glob)
  echo "inside=${g[0]}"
}
f
echo "outside=${g[0]}"

echo "=== the same split for readonly, export and local"
P=1 readonly -a ra=(1 2); declare -p ra
P=2 export -a ea=(3 4); declare -p ea
h() { P=3 local -a la=(5 6); declare -p la; }
h

echo "=== a scalar operand is still the builtin's, not the expansion's"
c=old
declare -a b=($c) c=2
declare -p b c

echo "=== the whole command dies with the operand, prefix unexpanded"
readonly ro=1
( P=$(echo prefix-expanded >&2) declare -a ro=(2); echo NOT-REACHED )
echo "rc=$?"

echo "=== the builtin's trace still comes last"
set -x
P=9 declare -x SE=1 brr=(1 2) SF=2
set +x
declare -p SE brr SF
