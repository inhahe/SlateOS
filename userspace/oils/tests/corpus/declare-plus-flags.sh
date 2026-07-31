# `declare +x` / `declare +r`: the "off" direction of the export and readonly
# attributes.
#
# The two directions are collected separately and the "off" set is applied
# last, so a removal in the same command wins whichever order the two letters
# came in. `+x` really does remove the export attribute. `+r` never removes
# anything — readonly is the one attribute the shell cannot take back — so it
# is a no-op on a name that does not have it and a refusal on one that does,
# judged against the attribute the name *arrived* with, which is why
# `declare -r +r fresh` still leaves an ordinary variable.

echo "=== +x removes the export attribute"
x1=5; export x1
declare +x x1; echo "rc=$?"; declare -p x1

echo "=== +x on a name that was never exported is a no-op"
x2=5
declare +x x2; echo "rc=$?"; declare -p x2

echo "=== +x still performs the operand's assignment"
x3=5; export x3
declare +x x3=7; echo "rc=$?"; declare -p x3

echo "=== +x leaves the other flags alone"
x4=5; export x4
declare -i +x x4; echo "rc=$?"; declare -p x4

echo "=== +x on a subscripted operand clears the base array's export"
declare -a a5=(1 2); export a5
declare +x a5[0]=9; echo "rc=$?"; declare -p a5

echo "=== +x on a compound operand"
declare -a a6=(1 2); export a6
declare +x a6=(3); echo "rc=$?"; declare -p a6

echo "=== +x follows a nameref to its target"
y7=5; export y7; declare -n r7=y7
declare +x r7; echo "rc=$?"; declare -p y7 r7

echo "=== +x inside a function makes a fresh local, leaving the global exported"
x8=5; export x8
f8() { declare +x x8; declare -p x8; }
f8; declare -p x8

echo "=== -g +x reaches past the local and clears the global"
x9=5; export x9
f9() { declare -g +x x9; }
f9; declare -p x9

echo "=== the removal wins over -x, in either order"
declare -x +x v10=hi; echo "rc=$?"; declare -p v10
declare +x -x v11=hi; echo "rc=$?"; declare -p v11
declare -x +x q12=(1 2); echo "rc=$?"; declare -p q12

echo "=== +r on a name that is not readonly is a no-op"
x13=5
declare +r x13; echo "rc=$?"; declare -p x13
declare -i +r x14=3+4; echo "rc=$?"; declare -p x14

echo "=== +r still brings a bare name into being"
declare +r nope15; echo "rc=$?"; declare -p nope15

echo "=== +r on a readonly name is refused, and abandons the operand"
x16=5; readonly x16
declare +r x16; echo "rc=$?"; declare -p x16
declare -i +r x16; echo "rc=$?"; declare -p x16

echo "=== the refusal is tagged with the builtin that asked"
x17=5; readonly x17
typeset +r x17; echo "rc=$?"
f17() { local +r x17; echo "rc=$?"; }
f17

echo "=== the refusal is judged before the command's own -r, in either order"
x18=5; readonly x18
declare -r +r x18; echo "rc=$?"; declare -p x18
declare +r -r x18; echo "rc=$?"; declare -p x18

echo "=== but -r +r on a name that arrived plain cancels out"
declare -r +r x19=5; echo "rc=$?"; declare -p x19
declare +r -r x20=5; echo "rc=$?"; declare -p x20
declare -r +r q21=(1 2); echo "rc=$?"; declare -p q21

echo "=== +r follows a nameref to its target"
y22=5; readonly y22; declare -n r22=y22
declare +r r22; echo "rc=$?"; declare -p y22 r22

echo "=== +r and +x together on an exported readonly: the refusal wins"
x23=5; export x23; readonly x23
declare +x +r x23; echo "rc=$?"; declare -p x23

echo "=== but +x alone strips the export off a readonly name"
x24=5; export x24; readonly x24
declare +x x24; echo "rc=$?"; declare -p x24

echo "=== export and readonly take no + flags at all"
x25=5; export x25
export +x x25; echo "rc=$?"; declare -p x25
x26=5; readonly x26
readonly +r x26; echo "rc=$?"; declare -p x26
