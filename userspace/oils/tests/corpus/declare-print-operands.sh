# `-p` turns a declaration builtin around: instead of being *told* about its
# operands it is *asked* about them. A compound `name=(…)` operand is still
# bound — bash performs the assignment during word expansion, before the
# builtin runs at all — so what `-p` reports is the array the same command just
# made. What it does *not* do is apply any of the builtin's own attributes.
e() { sed 's/^.*: line [0-9]*: //'; }

echo "=== an assignment operand is bound, then printed"
declare -p h1=1; echo "rc=$?"
declare -p h2=(1 2); echo "rc=$?"
declare -p h2b=([k]=v); echo "rc=$?"

echo "=== plain names and assignments together, in source order"
h3=zz
declare -p h3 h4=(9); echo "rc=$?"
declare -p h5=(1) h6; echo "rc=$?"

echo "=== the kind letters still choose the kind the literal binds as"
declare -pa h7=(1); echo "rc=$?"
declare -pA h8=([k]=v); echo "rc=$?"

echo "=== but the value attributes are not applied at all"
declare -px h9=(1); echo "rc=$?"; declare -p h9
declare -pr h10=(1); echo "rc=$?"; h10[0]=9; echo "write rc=$?"; declare -p h10
declare -pi h11=(2+3); echo "rc=$?"; declare -p h11
declare -pl h12=(AB); echo "rc=$?"; declare -p h12

echo "=== nor do the refusals a non-printing command would reach"
declare -a h13=(1 2)
declare -p +a h13=(3); echo "rc=$?"
declare -p -a -A h14=(3); echo "rc=$?"
declare -p -n h15=(1); echo "rc=$?"

echo "=== a name with no value prints bare; an unknown one is named"
declare h16
declare -p h16 nosuch1 2>&1 | e; echo "rc=$?"
# The complaint carries the name the command was *called* by.
typeset -p nosuch2 2>&1 | e; echo "rc=$?"

echo "=== -g -p reaches the global from inside a function"
gg=outer
f1() { local gg=inner; declare -gp gg; declare -p gg; }
f1

echo "=== local -p answers a different question: this frame's locals"
f2() { local m1=1 m2; local -p; echo "rc=$?"; }
f2
f3() { local m3=v; local -p m3; echo "rc=$?"; }
f3
# A global is not a local, and neither is a *caller's* local.
f4() { local -p gg; echo "rc=$?"; }
f4 2>&1 | e
outer() { local m4=1; inner; }
inner() { local -p m4; echo "rc=$?"; }
outer 2>&1 | e
# An assignment operand is taken whole as a name — and assigns nothing.
f5() { local -p m5=5; echo "rc=$?"; declare -p m5 2>&1; }
f5 2>&1 | e
# Kind letters do not filter the listing.
f6() { local m6=1; local -a m7=(x); local -pa; echo "rc=$?"; }
f6
# A compound operand binds as a local first, then is listed.
f7() { local -p m8=(1 2); echo "rc=$?"; }
f7
# …and outside a function there is no frame to list.
local -p 2>&1 | e; echo "rc=$?"

echo "=== readonly -p and export -p are not this: they still mark"
readonly h17=(1); echo "rc=$?"
readonly -p h18=(1) >/dev/null; echo "rc=$?"; declare -p h18
export -p h19=(1) >/dev/null; echo "rc=$?"; declare -p h19

echo "=== done"
