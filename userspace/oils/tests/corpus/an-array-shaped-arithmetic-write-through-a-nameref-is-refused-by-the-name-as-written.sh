# An arithmetic assignment through a nameref stores where the reference points,
# and a refusal normally says so — `((r=5))` onto a readonly scalar `x` is
# `x: readonly variable`. But an **array-shaped** write is about the reference's
# own name, the same seam a plain assignment has:
#
#   * a subscripted operand — `((r[0]=5))` — is `r`, whatever it reached;
#   * and, unlike a plain assignment, so is a *bare* operand whose target is an
#     **indexed** array: `((r=5))` onto an array `q` is really `q[0]=5`, so it
#     is `r: readonly variable` where `r=5` written as an assignment is `q:`.
#
# An associative array is not in that second case, which is measured rather than
# reasoned: a bare `((r=5))` onto one stores under key `0` just as readily, and
# is still blamed on `m`. Only a written subscript moves it back to `r`.
#
# A reference that already designates one element (`declare -n r=q[1]`) makes no
# array, so it is blamed on the target like any other scalar store.
#
# Along a chain the name as written is the outermost one — the one the reader
# can see — not the link it went through.

echo '=== a bare write onto a scalar names the target'
( readonly x=1; declare -n r=x; ((r=5)); echo "rc=$?" ) 2>&1
( readonly x=1; declare -n r=x; ((r+=5)); echo "rc=$?" ) 2>&1
( readonly x=1; declare -n r=x; ((r++)); echo "rc=$?" ) 2>&1
( readonly x=1; declare -n r=x; let 'r=5'; echo "rc=$?" ) 2>&1

echo '=== …and onto an indexed array it names the reference'
( readonly -a q=(1); declare -n r=q; ((r=5)); echo "rc=$?" ) 2>&1
( readonly -a q=(1); declare -n r=q; ((r+=2)); echo "rc=$?" ) 2>&1
( readonly -a q=(1); declare -n r=q; ((++r)); echo "rc=$?" ) 2>&1
( declare -a q; readonly q; declare -n r=q; ((r=5)); echo "rc=$?" ) 2>&1

echo '=== …but an associative one still names the target'
( readonly -A m=([k]=1); declare -n r=m; ((r=5)); echo "rc=$?" ) 2>&1
( declare -A m; readonly m; declare -n r=m; ((r=5)); echo "rc=$?" ) 2>&1

echo '=== a subscripted operand names the reference whatever it reached'
( readonly -a q=(1); declare -n r=q; ((r[0]=5)); echo "rc=$?" ) 2>&1
( readonly -A m=([k]=1); declare -n r=m; ((r[k]=5)); echo "rc=$?" ) 2>&1
( readonly x=1; declare -n r=x; ((r[0]=5)); echo "rc=$?" ) 2>&1

echo '=== a reference to one element is not an array-shaped write'
( readonly -a q=(1 2); declare -n r=q[1]; ((r=5)); echo "rc=$?" ) 2>&1
( readonly -a q=(1 2); declare -n r=q[1]; ((r++)); echo "rc=$?" ) 2>&1

echo '=== along a chain it is the outermost name'
( readonly -a q=(1); declare -n r2=q; declare -n r1=r2; ((r1=5)); echo "rc=$?" ) 2>&1
( readonly x=1; declare -n r2=x; declare -n r1=r2; ((r1[0]=5)); echo "rc=$?" ) 2>&1
( readonly x=1; declare -n r2=x; declare -n r1=r2; ((r1=5)); echo "rc=$?" ) 2>&1

echo '=== every arithmetic entry point spells it the same way'
( readonly -a q=(1); declare -n r=q; let 'r=5'; echo "rc=$?" ) 2>&1
( readonly -a q=(1); declare -n r=q; echo "[$((r+=1))]"; echo "rc=$?" ) 2>&1
( readonly -a q=(1); declare -n r=q; for ((r=5;0;)); do :; done; echo "rc=$?" ) 2>&1
( readonly -a q=(1); declare -n r=q; declare -i n; n=$((r=5)); echo "rc=$?" ) 2>&1

echo '=== but an integer-attributed assignment is not an arithmetic write'
( readonly -a q=(1); declare -n r=q; declare -i r; r=5; echo "rc=$?" ) 2>&1
( readonly -a q=(1); declare -n r=q; r=5; echo "rc=$?" ) 2>&1
( readonly -A m=([k]=1); declare -n r=m; r=5; echo "rc=$?" ) 2>&1

echo '=== without a reference the two names are one'
( readonly -a q=(1); ((q=5)); echo "rc=$?" ) 2>&1
( readonly -a q=(1); ((q[0]=5)); echo "rc=$?" ) 2>&1
( readonly -A m=([k]=1); ((m[k]=5)); echo "rc=$?" ) 2>&1
( readonly x=1; ((x=5)); echo "rc=$?" ) 2>&1

echo '=== and where nothing is readonly the store still lands on the target'
( declare -a q=(1 2); declare -n r=q; ((r=5)); declare -p q ) 2>&1
( declare -A m=([k]=1); declare -n r=m; ((r=5)); declare -p m ) 2>&1
( declare -a q=(1 2); declare -n r=q[1]; ((r=9)); declare -p q ) 2>&1

echo still here
