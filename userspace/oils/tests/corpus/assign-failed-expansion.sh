# An expansion that fails still has to return *something*, and for arithmetic
# that something is `0`. Storing it would be worse than the error: the variable
# would silently read as `0` afterwards, so a script that meant to keep its old
# value gets a plausible wrong one instead of an obvious missing one. So a failed
# expansion abandons the assignment — the variable is left exactly as it was, and
# only the status and the diagnostic report the failure.
#
# The failure also discards the rest of the parse unit, so every check below puts
# its `echo` on a line of its own; on the same line it would never run. That is
# also why the value cannot simply be read back on the failing line.

echo "=== a scalar keeps its old value ==="
x=old
x=$(( a + ))
echo "syntax rc=$? x=[$x]"
x=$(( 1/0 ))
echo "divzero rc=$? x=[$x]"

echo "=== so does a concatenation, an append and an integer variable ==="
# The fabricated `0` would be visible in the middle of the word, or glued onto
# the end of the old value, or evaluated as an integer.
y=old
y=p$(( a + ))q
echo "concat rc=$? y=[$y]"
z=old
z+=$(( a + ))
echo "append rc=$? z=[$z]"
declare -i n=7
n=$(( a + ))
echo "integer rc=$? n=[$n]"

echo "=== an element assignment leaves the array alone ==="
declare -a A=(o1 o2)
A[0]=$(( a + ))
echo "indexed rc=$? A=[${A[*]}]"
declare -A M=([k]=o)
M[k]=$(( a + ))
echo "assoc rc=$? M=[${M[k]}]"

echo "=== an indexed literal is expanded whole, so nothing binds ==="
# Not even the clearing that a non-append literal would do first.
declare -a B=(o1 o2)
B=( p$(( a + ))q z )
echo "literal rc=$? B=[${B[*]}]"
declare -a C=(o1)
C+=( good x$(( a + )) )
echo "append-literal rc=$? C=[${C[*]}]"

echo "=== an associative literal binds element by element ==="
# So an append keeps the pairs that came before the failing one — the one place
# a partial result survives.
declare -A P=([k]=o)
P+=( [i]=good [j]=$(( a + )) )
echo "append-keyed rc=$? n=${#P[@]} i=[${P[i]}]"
declare -A Q=([k]=o)
Q+=( pk pv j$(( a + )) x )
echo "append-pairs rc=$? n=${#Q[@]} pk=[${Q[pk]}]"
# A non-append one swaps a table in at the end, and the swap is abandoned.
declare -A R=([k]=o)
R=( [i]=good [j]=$(( a + )) )
echo "keyed rc=$? n=${#R[@]} k=[${R[k]}]"

echo "=== a name that did not exist is not created ==="
NEW=$(( a + ))
declare -p NEW 2>&1
echo "new rc=$?"
# And an array that had no value yet does not acquire an empty one.
declare -a EA
EA=( x$(( a + )) )
declare -p EA 2>&1
echo "empty-indexed rc=$?"
declare -A EM
EM=( [k]=$(( a + )) )
declare -p EM 2>&1
echo "empty-assoc rc=$?"

echo "=== declare, export and a command prefix agree ==="
d=old
declare d=$(( a + ))
echo "declare rc=$? d=[$d]"
export e=old
export e=$(( a + ))
echo "export rc=$? e=[$e]"
p=old
p=$(( a + )) true
echo "prefix rc=$? p=[$p]"

echo "=== an assignment that is not made is not traced ==="
t=old
set -x
t=$(( a + ))
set +x
echo "xtrace t=[$t]"

echo done
