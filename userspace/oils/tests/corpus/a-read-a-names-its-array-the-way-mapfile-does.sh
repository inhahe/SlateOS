# `read -a` and `mapfile` fill a whole array, and they name one the same way.
#
# The operand is a plain name — an *element* is not an array to put records in
# — and a nameref is followed to the array actually filled. What is found there
# has to be an indexed array that can be written: an associative one is the
# builtin's own objection and carries its name, a readonly one is the
# variable's own and names the variable alone, and a name the shell maintains
# refuses without a word.
#
# The blame on those refusals stays with the *operand*, not with the name the
# chain landed on: `declare -A m; declare -n r=m; read -a r` reports
# `read: r: not an indexed array`, however many links the chain had. A bare
# operand only looks like it is blamed by its resolution because that is also
# what was written down.
#
# Where the two builtins part is *when* they ask. `mapfile` settles its target
# before it reads a byte, so a refused one leaves the input where it was;
# `read -a` has already consumed its record by the time bash looks at where to
# put it, so a refused one still eats a line.

echo "=== the reference is followed, and survives the write"
( declare -n r=zz; read -a r <<< 'p q'; declare -p zz; declare -p r ) 2>&1
( declare -n r=zz; mapfile -t r <<< p; declare -p zz; declare -p r ) 2>&1

echo "=== an associative target is the builtin's own objection"
( declare -A m; read -a m <<< x; echo "rc=$?"; declare -p m ) 2>&1
( declare -A m; mapfile -t m <<< x; echo "rc=$?"; declare -p m ) 2>&1
( declare -A m; declare -n r=m; read -a r <<< x; echo "rc=$?" ) 2>&1
( declare -A m; declare -n r=m; mapfile -t r <<< x; echo "rc=$?" ) 2>&1
( declare -A m; declare -n r1=r2; declare -n r2=m; read -a r1 <<< x; echo "rc=$?" ) 2>&1

echo "=== a reference designating an element names no array"
( declare -n r='q[1]'; read -a r <<< x; echo "rc=$?" ) 2>&1
( declare -n r='q[1]'; mapfile -t r <<< x; echo "rc=$?" ) 2>&1
( read -a 'a[0]' <<< x; echo "rc=$?" ) 2>&1
( read -a 'a b' <<< x; echo "rc=$?" ) 2>&1

echo "=== a readonly target names the variable alone, and outranks the kind"
( declare -ar k=(o); read -a k <<< x; echo "rc=$?"; declare -p k ) 2>&1
( declare -ar k=(o); declare -n r=k; read -a r <<< x; echo "rc=$?"; declare -p k ) 2>&1
( declare -ar k=(o); declare -n r1=r2; declare -n r2=k; read -a r1 <<< x; echo "rc=$?" ) 2>&1
( declare -Ar m=([k]=1); read -a m <<< x; echo "rc=$?" ) 2>&1
( declare -Ar m=([k]=1); mapfile -t m <<< x; echo "rc=$?" ) 2>&1

echo "=== a name the shell maintains refuses without a word"
( read -a GROUPS <<< x; echo "rc=$?" ) 2>&1
( declare -n r=GROUPS; read -a r <<< x; echo "rc=$?" ) 2>&1
( declare -n r=BASH_ARGC; read -a r <<< x; echo "rc=$?" ) 2>&1

echo "=== a refused read -a still eats its record; a refused mapfile does not"
( declare -A m; { read -a m; read y; } <<< $'p\nq' 2>/dev/null; echo "y=[$y]" ) 2>&1
( declare -A m; { mapfile -t m; read y; } <<< $'p\nq' 2>/dev/null; echo "y=[$y]" ) 2>&1
( declare -ar k=(o); { read -a k; read y; } <<< $'p\nq' 2>/dev/null; echo "y=[$y]" ) 2>&1
( read -a 'a b' <<< x; echo "rc=$?" ) 2>&1
