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

# `+a`/`+A` are the same shape as `+r`: an array never becomes a scalar again,
# so each letter refuses when the name really is an array of the kind it names
# and does nothing at all otherwise.

echo "=== +a on a name that is not an indexed array is a no-op"
x27=5
declare +a x27; echo "rc=$?"; declare -p x27
declare +a nope28; echo "rc=$?"; declare -p nope28
declare -A m29=([k]=1)
declare +a m29; echo "rc=$?"; declare -p m29
declare -a q30=(1 2)
declare +A q30; echo "rc=$?"; declare -p q30

echo "=== +a on a real indexed array is refused, and abandons the operand"
declare -a q31=(1 2)
declare +a q31; echo "rc=$?"; declare -p q31
declare -i +a q31; echo "rc=$?"; declare -p q31
declare -A m32=([k]=1)
declare +A m32; echo "rc=$?"; declare -p m32

echo "=== the array the same command makes is refused too"
declare -a +a q33; echo "rc=$?"; declare -p q33
declare -A +A m34; echo "rc=$?"; declare -p m34
declare -a +a q35=5; echo "rc=$?"; declare -p q35
declare +a n36[0]=5; echo "rc=$?"; declare -p n36

echo "=== a compound literal binds first, then the refusal"
declare +a q37=(1 2); echo "rc=$?"; declare -p q37
declare -x +a q38=(1 2); echo "rc=$?"; declare -p q38
declare +A q39=(1 2); echo "rc=$?"; declare -p q39

echo "=== the refusal outranks the kind conflict"
declare -a q40=(1 2)
declare -A +a q40; echo "rc=$?"; declare -p q40

echo "=== a readonly refusal outranks it in turn"
declare -a q41=(1 2); readonly q41
declare +ar q41; echo "rc=$?"; declare -p q41

echo "=== the refusal quotes the operand, not the reference's target"
declare -a arr42=(1 2); declare -n r42=arr42
declare +a r42; echo "rc=$?"; declare -p arr42 r42
s43=5; declare -n r43=s43
declare +a r43; echo "rc=$?"; declare -p s43 r43

echo "=== inside a function the local shadow comes first"
declare -a q44=(1 2)
f44() { declare +a q44; echo "rc=$?"; declare -p q44; }
f44; declare -p q44
f45() { local -a q45=(1 2); declare +a q45; echo "rc=$?"; declare -p q45; }
f45
declare -a q46=(1 2)
f46() { declare -g +a q46; echo "rc=$?"; }
f46; declare -p q46

# The same "off is applied last" rule governs every letter that really can be
# removed, not just `-x`.

echo "=== -i +i in one command leaves no integer attribute, and no arithmetic"
declare -i +i x47=3+4; declare -p x47
declare +i -i x48=3+4; declare -p x48
declare -i x49=1; declare +i x49; declare -p x49; x49=5+5; declare -p x49

echo "=== -n +n leaves an ordinary variable holding the target's name"
w50=9
declare -n +n r50=w50; echo "rc=$?"; declare -p r50 w50
declare +n -n r51=w50; echo "rc=$?"; declare -p r51

echo "=== but the -n half still judges the operand"
declare -n +n r52='a b'; echo "rc=$?"
declare -n +n r53[1]=w50; echo "rc=$?"

echo "=== -t +t"
declare -t +t x54=5; declare -p x54
declare -t x55=5; declare +t x55; declare -p x55

echo "=== the same letter twice in one direction is not a conflict"
declare -i -i x56=3+4; declare -p x56
declare +i +i x57=3+4; declare -p x57

# The three case folds are mutually exclusive as *values* — a name carries at
# most one — but each letter keeps its own on and off bits. So `+u` takes off
# the uppercase fold and nothing else, and a name that is lowercase stays
# lowercase through it, where a single "clear the case attributes" directive
# would have dropped it.

echo "=== one enable sets its own fold and clears the other two"
declare -l c58=AB; declare -p c58
declare -u c59=ab; declare -p c59
declare -c c60=ab; declare -p c60

echo "=== the same letter both ways ends up off, in either order"
declare -l +l c61=AB; declare -p c61
declare +l -l c62=AB; declare -p c62
declare -u +u c63=ab; declare -p c63
declare -c +c c64=ab; declare -p c64

echo "=== a removal answers only for its own letter"
declare -l +u c65=AB; declare -p c65
declare -u +l c66=ab; declare -p c66
declare -l +c c67=AB; declare -p c67
declare -c +l c68=ab; declare -p c68

echo "=== and it leaves a fold the name already carried standing"
declare -l c69=AB; declare +u c69; declare -p c69
declare -u c70=ab; declare +l c70; declare -p c70
declare -c c71=ab; declare +l c71; declare -p c71

echo "=== removing the name's own letter really does take the fold off"
declare -l c72=AB; declare +l c72; declare -p c72; c72=XY; declare -p c72
declare -u c73=ab; declare +u c73; declare -p c73
declare -c c74=ab; declare +c c74; declare -p c74
declare -l c75=AB; declare +l +u +c c75; declare -p c75

echo "=== two different enables cancel, and drop the fold the name had"
declare -l -u c76=Ab; declare -p c76
declare -u -l c77=Ab; declare -p c77
declare -l -c c78=Ab; declare -p c78
declare -c c79=ab; declare -l -u c79; declare -p c79
declare -lu c80=Ab; declare -p c80

echo "=== the same letter twice is not a conflict"
declare -l -l c81=AB; declare -p c81
declare -l c82=AB; declare -l -u c82; declare -p c82

echo "=== enable, remove, enable: the cancellation still wins"
declare -l +l -u c83=aB; declare -p c83
declare -u +u -l c84=Ab; declare -p c84

# A compound literal does not bind under the attributes the command leaves the
# name holding. If some `-`-direction word names a kind or a scope (`a`, `A`,
# `g`), or the first mention of a value letter (`i`, `l`, `u`, `c`, `I`) is in
# the `-` direction, the literal is *claimed*: it binds under every value
# letter the command names in *either* direction — so `-a +i` still evaluates
# and `-a +l` still folds. Otherwise it binds under what the name arrived
# with, less what this command takes off.
echo "=== a claimed literal binds under letters named in the off direction too"
declare -a +i d1=(2+3); declare -p d1
declare -a +l d2=(AB); declare -p d2
declare -A +l d3=([q]=AB); declare -p d3
declare -g +i d4=(2+3); declare -p d4
declare -g +l d5=(AB); declare -p d5

echo "=== the first value letter in the - direction claims it as well"
declare -l +i d6=(2+3); declare -p d6
declare -i +l d7=(AB); declare -p d7
declare -u +i d8=(2+3); declare -p d8

echo "=== an unclaimed literal binds under what the name arrived with"
declare +i -i d9=(2+3); declare -p d9
declare +i -a d10=(2+3); declare -p d10
declare +l -i d11=(AB); declare -p d11
declare -l d12=(A); declare +u d12=(bc); declare -p d12
declare -l d13=(A); declare +l d13=(BC); declare -p d13

echo "=== two case letters named leave the literal unfolded, and the name too"
declare -a +l +u d14=(Ab); declare -p d14
declare -a -l +u d15=(Ab); declare -p d15
declare -l d16=(A); declare -a +u d16=(BC); declare -p d16
declare -c d17=(ab); declare -a -l -u d17=(cD); declare -p d17

echo "=== no case letter named: a claimed literal keeps the name's own fold"
declare -l d18=(A); declare -a d18=(BC); declare -p d18
declare -u d19=(a); declare -a +i d19=(bc); declare -p d19

echo "=== a scalar or a subscript uses the final attributes, as always"
declare -a +i d20=2+3; declare -p d20
declare +i -i d21=2+3; declare -p d21
declare -a +i d22[0]=2+3; declare -p d22
declare -a +l d23[0]=AB; declare -p d23

echo "=== a refused +a/+A abandons the operand, so the removals never run"
# `+a` cannot un-make the array the literal just bound, and that refusal lands
# exactly where the builtin would have taken the fold or the integer attribute
# back off again — so the name keeps whatever the literal bound *under*.
e() { sed 's/^.*: line [0-9]*: //'; }
{ declare -a d24=(1); declare +a -l +l d24=(AB); echo "rc=$?"; declare -p d24; } 2>&1 | e
{ declare -a d25=(1); declare +a -i +i d25=(2+3); echo "rc=$?"; declare -p d25; } 2>&1 | e
# The array this very command makes counts as one it cannot un-make.
{ declare +a -l +l d26=(AB); echo "rc=$?"; declare -p d26; } 2>&1 | e
{ declare +a -i +i d27=(2+3); echo "rc=$?"; declare -p d27; } 2>&1 | e
{ declare -A d28=([k]=v); declare +A -l +l d28=([k]=AB); echo "rc=$?"; declare -p d28; } 2>&1 | e
# …but `+a` against an *associative* name refuses nothing, so they do run.
{ declare -A d29=([k]=v); declare +a -l +l d29=([k]=AB); echo "rc=$?"; declare -p d29; } 2>&1 | e
{ declare +A -l +l d30=([k]=AB); echo "rc=$?"; declare -p d30; } 2>&1 | e
