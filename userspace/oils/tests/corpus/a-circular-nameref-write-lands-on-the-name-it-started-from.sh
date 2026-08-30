# A nameref chain that closes on itself names nothing, and bash's answer is not
# one answer but two, chosen by the *shape* of the value being stored.
#
# A **scalar** store has nowhere to go: it warns and fails, and a bare `c1=x`
# being a failed variable assignment, that ends the shell.
#
# An **array** store does not fail. bash warns and then treats the name the walk
# started from as the target — so the write lands on the reference variable
# itself, which stops being a reference (bash has no array namerefs: `declare -a
# a; declare -n a=x` is `reference variable cannot be an array`). That last part
# is what makes the shell usable again rather than merely quiet: the other end
# of the cycle still names `c1`, and with `c1` no longer a reference the cycle
# is gone, so `${c2[0]}` reads what was just written.
#
# The warning names the variable the walk *started* from, and it is printed once
# per walk of the chain — which is why a subscripted target, whose name bash
# resolves twice (once to find the array, once to bind the element), reports it
# twice where a whole-array fill reports it once.
#
# Every case runs in a subshell so a scalar refusal cannot take the file with
# it, and so each starts from the same untouched cycle.

cyc='declare -n c1=c2; declare -n c2=c1;'

echo '=== a whole-array fill lands on the name, and warns once'
( eval "$cyc"; read -a c1 <<< "p q"; echo "rc=$?"; declare -p c1 c2 )
( eval "$cyc"; mapfile -t c1 <<< x; echo "rc=$?"; declare -p c1 )
( eval "$cyc"; readarray -t c1 <<< x; echo "rc=$?"; declare -p c1 )

echo '=== so does a compound literal'
( eval "$cyc"; c1=(a b); echo "rc=$?"; declare -p c1 )
( eval "$cyc"; c1+=(a); echo "rc=$?"; declare -p c1 )
( eval "$cyc"; c1=(); echo "rc=$?"; declare -p c1 )
( eval "$cyc"; declare -A c1=([k]=v); echo "rc=$?"; declare -p c1 )

echo '=== a subscripted target lands too, and warns twice'
( eval "$cyc"; c1[0]=z; echo "rc=$?"; declare -p c1 )
( eval "$cyc"; c1[3]=v; echo "rc=$?"; declare -p c1 )
( eval "$cyc"; c1[0]+=z; echo "rc=$?"; declare -p c1 )
( eval "$cyc"; read 'c1[2]' <<< s; echo "rc=$?"; declare -p c1 )
( eval "$cyc"; printf -v 'c1[0]' x; echo "rc=$?"; declare -p c1 )

echo '=== a scalar store has nowhere to land: it warns and fails'
( eval "$cyc"; read c1 <<< q; echo "rc=$?"; declare -p c1 c2 )
( eval "$cyc"; read -N 1 c1 <<< q; echo "rc=$?" )
( eval "$cyc"; read c1 z <<< 'a b'; echo "rc=$?" )
( eval "$cyc"; printf -v c1 x; echo "rc=$?"; declare -p c1 )
( eval "$cyc"; getopts x c1; echo "rc=$?" )

echo '=== and a bare assignment ends the shell it is in'
( eval "$cyc"; c1=x; echo "not reached" ); echo "rc=$?"
( eval "$cyc"; c1+=x; echo "not reached" ); echo "rc=$?"

echo '=== a for loop is refused before its first iteration'
( eval "$cyc"; for c1 in v w; do echo "not reached"; done; echo "rc=$?"; declare -p c1 )
( eval "$cyc"; for c1 in; do echo "not reached"; done; echo "empty list rc=$?" )

echo '=== the cycle is broken by the write, so the other end reads it'
( eval "$cyc"; read -a c1 <<< x; echo "[${c2[0]}] rc=$?" )
( eval "$cyc"; read -a c1 <<< x; c1[1]=y; declare -p c1 )

echo '=== a longer cycle is blamed on the name that was written'
( declare -n a1=a2; declare -n a2=a3; declare -n a3=a1
  read -a a1 <<< x; echo "rc=$?"; declare -p a1 a2 a3 )

echo '=== a chain that resolves is untouched by any of this'
( t=orig; declare -n r=t; read -a r <<< "p q"; echo "rc=$?"; declare -p r t )
( t=orig; declare -n r=t; r[1]=z; echo "rc=$?"; declare -p r t )

echo still here
