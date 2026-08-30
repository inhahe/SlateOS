# A nameref redirects an assignment to its target, and everything the store
# *does* belongs to the target. Two things the writer sees do not.
#
#   * `set -x` shows the line as typed. `declare -n r=x; r=5` traces `+ r=5`,
#     never the `x=5` it became — and a reference that designates an *element*
#     traces `+ r=9` rather than the `x[1]=9` the store turned into.
#
#   * a readonly refusal names the reference for an **array-shaped** write — a
#     subscripted operand or a compound literal — and the target for a scalar
#     one. `r[0]=5` and `r=(a b)` through a reference to a readonly `x` are
#     `r: readonly variable`; the scalar `r=5` is `x: readonly variable`. It is
#     the same seam as everywhere else a nameref meets an array: a write that
#     makes one is about the reference's own name.
#
# The two meet at the end, because bash asks whether the variable may be written
# only once it has expanded what it was going to store: a refused assignment
# still runs its value's side effects and still traces.

echo '=== the trace is the line as typed'
( x=1; declare -n r=x; set -x; r=5 ) 2>&1
( x=1; declare -n r=x; set -x; r+=5 ) 2>&1
( x=(1); declare -n r=x; set -x; r[0]=5 ) 2>&1
( x=1; declare -n r=x; set -x; r=(a b) ) 2>&1
( declare -A m=([k]=1); declare -n r=m; set -x; r[k]=9 ) 2>&1

echo '=== …including where the reference designates an element'
( x=(1 2); declare -n r=x[1]; set -x; r=9 ) 2>&1
( x=(1 2); declare -n r=x[1]; set -x; r+=9 ) 2>&1

echo '=== …and along a chain, where it is the first link'
( x=1; declare -n r2=x; declare -n r1=r2; set -x; r1=5 ) 2>&1
( x=(1); declare -n r2=x; declare -n r1=r2; set -x; r1[0]=5 ) 2>&1

echo '=== the scalar trace still carries the expanded value'
( y=2; x=1; declare -n r=x; set -x; r=$y ) 2>&1
( y=2; x=1; declare -n r=x; set -x; r="a $y b" ) 2>&1

echo '=== and the store itself lands on the target'
( x=1; declare -n r=x; r=5; declare -p x r ) 2>&1
( x=(1 2); declare -n r=x; r[1]=9; declare -p x r ) 2>&1
( x=(1 2); declare -n r=x[1]; r=9; declare -p x r ) 2>&1

echo '=== an array-shaped refusal names the reference'
( readonly x=1; declare -n r=x; r[0]=5; echo "rc=$?" ) 2>&1
( readonly -a x=(1); declare -n r=x; r[0]=5; echo "rc=$?" ) 2>&1
( readonly x=1; declare -n r=x; r=(a b); echo "rc=$?" ) 2>&1
( declare -A m=([k]=1); readonly m; declare -n r=m; r[k]=5; echo "rc=$?" ) 2>&1
( readonly x=1; declare -n r2=x; declare -n r1=r2; r1[0]=5; echo "rc=$?" ) 2>&1

echo '=== …and a scalar-shaped one names the target'
( readonly x=1; declare -n r=x; r=5; echo "rc=$?" ) 2>&1
( readonly x=1; declare -n r=x; r+=5; echo "rc=$?" ) 2>&1
( readonly x=1; declare -n r2=x; declare -n r1=r2; r1=5; echo "rc=$?" ) 2>&1

echo '=== a reference *to* an element is not an array-shaped write'
( readonly -a x=(1 2); declare -n r=x[1]; r=9; echo "rc=$?" ) 2>&1

echo '=== without a reference the two names are one'
( readonly x=1; x[0]=5; echo "rc=$?" ) 2>&1
( readonly -a x=(1); x[0]=5; echo "rc=$?" ) 2>&1
( readonly x=1; x=(a b); echo "rc=$?" ) 2>&1

echo '=== a refusal comes after the value has been expanded and traced'
( readonly x=1; declare -n r=x; set -x; r=5 ) 2>&1
( readonly x=1; declare -n r=x; set -x; r=$(echo v) ) 2>&1
( readonly -a q=(1); declare -n r=q; set -x; r[0]=5 ) 2>&1
( readonly x=1; declare -n r=x; set -x; r=(a b) ) 2>&1

echo '=== …so the value runs its side effects, and the subscript runs second'
( readonly x=1; declare -n r=x; r=$(echo SIDE >&2; echo v) ) 2>&1
( readonly -a q=(1); declare -n r=q; r[$(echo SUB >&2; echo 0)]=$(echo VAL >&2; echo v) ) 2>&1
( declare -A m=([k]=1); readonly m; declare -n r=m; r[$(echo SUB >&2; echo k)]=$(echo VAL >&2; echo v) ) 2>&1

echo '=== …but a compound literal is refused without expanding anything'
( readonly x=1; declare -n r=x; r=($(echo SIDE >&2; echo a)) ) 2>&1

echo '=== everything the subscript itself owes is said first'
( readonly -a q=(1); q[]=v ) 2>&1
( readonly -a q=(1); q[1/0]=v ) 2>&1
( readonly -a q=(1); q[-5]=v ) 2>&1
( declare -A m=([k]=1); readonly m; b=; m[$b]=v ) 2>&1

echo '=== …and the value the variable owes is said first the other way round'
( readonly -a q=(1); declare -i q; q[0]=1/0 ) 2>&1
( readonly x=1; declare -i x; x=1/0 ) 2>&1

echo '=== an expansion that ends the shell says only that'
( readonly x=1; set -x; x=${u?boom} ) 2>&1
( readonly -a q=(1); q[0]=${u?boom} ) 2>&1
( set -u; readonly x=1; x=$nope ) 2>&1

echo still here
