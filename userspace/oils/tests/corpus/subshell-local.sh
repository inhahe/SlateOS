# A subshell is a fork, so it inherits the whole call-frame stack: `local`
# inside `( … )`, `$( … )`, a pipeline element or a background subshell of a
# function declares in *that function's* frame, and the binding goes away with
# the subshell rather than with a return.
e() { sed 's/^.*: line [0-9]*: //'; }

echo "=== a plain subshell of a function"
f1() { ( local q=1; echo "q=$q" ); echo "after=${q-unset}"; }
f1 2>&1 | e

echo "=== a compound operand"
f2() { ( local q=(1 2); declare -p q ); echo "after=${q-unset}"; }
f2 2>&1 | e

echo "=== it shadows the global for the subshell only"
g=outer
f3() { ( local g=inner; echo "in=$g" ); echo "out=$g"; }
f3 2>&1 | e

echo "=== a command substitution"
f4() { echo "sub=$( local s=1; echo "s=$s" )"; }
f4 2>&1 | e

echo "=== a pipeline element"
f5() { local p=1 | cat; echo "rc=$?"; }
f5 2>&1 | e

echo "=== a background subshell"
f6() { ( local b=1; echo "b=$b" ); }
f6 2>&1 | e

echo "=== the enclosing frame's own locals are still visible"
f7() { local o=1; ( local i=2; echo "o=$o i=$i" ); echo "i=${i-unset}"; }
f7 2>&1 | e

echo "=== declare -p and local -p read the inherited frame"
f8() { local a=1; ( local b=2; local -p ); }
f8 2>&1 | e

echo "=== a function called inside the subshell gets a frame of its own"
inner() { local n=deep; echo "n=$n"; }
f9() { local n=shallow; ( inner; echo "n=$n" ); }
f9 2>&1 | e

echo "=== …and outside a function it is still an error"
( local z=1; echo "rc=$?" ) 2>&1 | e
( echo "sub=$( local z=1; echo "rc=$?" )" ) 2>&1 | e

echo "=== local - inside a subshell"
fa() { local -; set -u; ( local -; set +u; echo "in=$-" ); }
fa 2>&1 | e | sed 's/[a-z]*u[a-z]*/OPTS/'

echo "=== unset of an inherited local inside the subshell"
fb() { local u=1; ( unset u; echo "in=${u-unset}" ); echo "out=${u-unset}"; }
fb 2>&1 | e

echo "=== done"
