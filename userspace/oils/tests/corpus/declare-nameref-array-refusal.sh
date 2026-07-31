# `declare -n` with a compound operand asks for a variable that is both a
# reference and an array, which bash refuses. The literal still binds — as
# whatever kind the letters name — but the builtin abandons the operand, so
# `n` and every other attribute it would have applied is dropped, including the
# case/integer *removals* that normally run once the literal has bound.
p() { echo -n "$1 -> "; declare -p "$2" 2>&1 | sed 's/^.*: line [0-9]*: //'; }
e() { sed 's/^.*: line [0-9]*: //'; }

echo "=== the plain refusal"
{ declare -n a1=(1); echo "rc=$?"; } 2>&1 | e
p 'a1' a1
echo "=== a following command on the same line still runs"
{ declare -n a2=(1); echo "same-line rc=$? reached"; } 2>&1 | e

echo "=== +n does not ask for it; -n +n still does"
{ declare +n a3=(1); echo "rc=$?"; } 2>&1 | e
p 'a3 (+n)   ' a3
{ declare -n +n a4=(1); echo "rc=$?"; } 2>&1 | e
p 'a4 (-n +n)' a4

echo "=== each of declare/typeset/local names itself"
{ typeset -n a5=(1); echo "rc=$?"; } 2>&1 | e
{ f() { local -n a6=(1); echo "rc=$?"; declare -p a6; }; f; } 2>&1 | e
{ f() { declare -gn a7=(1); echo "rc=$?"; }; f; p 'a7 (-gn)' a7; } 2>&1 | e

echo "=== the refused operand keeps what the literal bound under"
{ declare -n -l +l a8=(AB); echo "rc=$?"; } 2>&1 | e
p 'a8 (-l +l)' a8
{ declare -n +l -l a9=(AB); echo "rc=$?"; } 2>&1 | e
p 'a9 (+l -l)' a9
{ declare -n -i +i a10=(2+3); echo "rc=$?"; } 2>&1 | e
p 'a10 (-i +i)' a10

echo "=== and takes none of the builtin's own attributes"
{ declare -n -r a11=(1); echo "rc=$?"; } 2>&1 | e
p 'a11 (-r)' a11
a11[0]=9; echo "write rc=$?"
{ declare -n -t a12=(1); echo "rc=$?"; } 2>&1 | e
p 'a12 (-t)' a12
{ declare -n -x a13=(1); echo "rc=$?"; } 2>&1 | e
p 'a13 (-x)' a13

echo "=== it outranks the destroy refusal"
declare -a a14=(1 2)
{ declare -n +a a14=(3); echo "rc=$?"; } 2>&1 | e
p 'a14 (-n +a)' a14
declare -A a15=([k]=v)
{ declare -n +A a15=(3); echo "rc=$?"; } 2>&1 | e
p 'a15 (-n +A)' a15

echo "=== and the array-kind self-conflict, in either order"
{ declare -n -a -A a16=(3); echo "rc=$?"; } 2>&1 | e
p 'a16 (-n -aA)' a16
{ declare -a -A -n a17=(3); echo "rc=$?"; } 2>&1 | e
p 'a17 (-aA -n)' a17

echo "=== but not the phase-A refusals, which stop the bind entirely"
declare -A a18=([k]=v)
( declare -n -a a18=(3); echo "not reached" ) 2>&1 | e
readonly a19=1
( declare -n a19=(2); echo "not reached" ) 2>&1 | e

echo "=== every operand is reported, and every one binds"
{ declare -n a20=(1) a21=(2); echo "rc=$?"; } 2>&1 | e
p 'a20' a20; p 'a21' a21

echo "=== a scalar operand alongside one still becomes a reference"
w=hello
{ declare -n a22=(1) a23=w; echo "rc=$?"; } 2>&1 | e
p 'a22' a22; p 'a23' a23

echo "=== a name that is already a reference is not being made one"
declare -A a24=([k]=v)
declare -n a25=a24
{ declare -n a25=(z); echo "rc=$?"; } 2>&1 | e
p 'a25' a25; p 'a24' a24

echo "=== bare and subscripted operands are not compounds"
{ declare -n a26; echo "rc=$?"; } 2>&1 | e
p 'a26 bare' a26
{ declare -n a27[0]=z; echo "rc=$?"; } 2>&1 | e
p 'a27 sub ' a27

echo "=== readonly/export read -n as their own flag, so they never refuse"
{ readonly -n a28=(1); echo "rc=$?"; } 2>&1 | e
p 'readonly -n' a28
{ export -n a29=(1); echo "rc=$?"; } 2>&1 | e
p 'export -n  ' a29

echo "=== done"
