# A subscript that is *lexically empty* — `a[]` — is not index 0. bash refuses
# it, and the shape of the refusal differs by where the reference appears:
#
#   * in arithmetic, it is a *complaint*, not an error. A read yields 0 and a
#     write is dropped, but the expression carries on and the command keeps the
#     status its value gives it. A read is complained about twice, because bash
#     validates the subscript both when resolving the reference and again when
#     fetching its value.
#   * in a `${…}` expansion, it parses fine and is only rejected when the word
#     is expanded — a runtime "bad substitution", which abandons the rest of its
#     list but does not exit the shell.
#
# The check is purely lexical: `a[  ]` is *not* empty, and arithmetic-evaluates
# its blanks to index 0 like any other subscript.

echo "=== an arithmetic read complains twice and is worth 0 ==="
a=(one two)
((a[])); echo "read rc=$?"
x=$((a[])); echo "sub rc=$? x=[$x]"
y=$(( a[] + a[] )); echo "twice rc=$? y=[$y]"
z=$(( a[] + 5 )); echo "sum rc=$? z=[$z]"
for ((i=a[]; i<1; i++)); do echo "for-body"; done
echo "for rc=$?"

echo "=== an arithmetic write is dropped, and the expression keeps its value ==="
# The refusal carries the enclosing builtin's tag, as an arithmetic error would.
((a[]=9)); echo "assign rc=$? n=${#a[@]} a0=[${a[0]}]"
w=$((a[]=3)); echo "sub-assign rc=$? w=[$w]"
let 'a[]=5'; echo "let rc=$?"
# A read-modify-write does both, in that order — and `a[]+=2` is worth 0+2, so
# the `((` succeeds even though nothing was stored.
((a[]++)); echo "incr rc=$?"
((a[]+=2)); echo "compound rc=$?"

echo "=== the name does not matter, and a nameref is not followed ==="
declare -A m=([k]=v)
((m[])); echo "assoc-read rc=$?"
((m[]=1)); echo "assoc-write rc=$? n=${#m[@]}"
((zz[])); echo "unset-read rc=$?"
((zz[]=4)); echo "unset-write rc=$? n=${#zz[@]}"
s=3
((s[])); echo "scalar rc=$?"
declare -n r=xx
xx=(7 8)
((r[])); echo "nameref-read rc=$?"
((r[]=1)); echo "nameref-write rc=$? n=${#xx[@]}"

echo "=== it is complained about only when reached ==="
((1 ? 7 : a[])); echo "ternary rc=$?"
((a[], 1)); echo "comma rc=$?"
# Nesting: the inner read is refused, so the outer subscript is 0 and its store
# lands on element 0 like any other.
b=(9)
((a[b[]]=42)); echo "nested rc=$? a0=[${a[0]}]"

echo "=== a brace expansion defers to expansion time ==="
# Never reached, so never complained about — this is what makes it a runtime
# error rather than a parse error.
if false; then echo "${a[]}"; fi
echo "guarded rc=$?"
# Reached, it abandons the rest of its list but not the script. The subject
# named is the whole word with its quotes removed.
echo "${a[]}"; echo "unreachable-bare"
echo "bare rc=$?"
echo "X${a[]}Y"; echo "unreachable-affix"
echo "affix rc=$?"
v=${a[]}; echo "unreachable-assign"
echo "assign rc=$? v=[$v]"
# Every brace form that can carry a subscript defers the same way.
echo "[${#a[]}]"; echo "unreachable-len"
echo "len rc=$?"
echo "[${!a[]}]"; echo "unreachable-bang"
echo "bang rc=$?"
echo "[${a[]:-def}]"; echo "unreachable-op"
echo "op rc=$?"
echo "[${a[]#o}]"; echo "unreachable-strip"
echo "strip rc=$?"

echo "=== blanks are not empty ==="
c=(11 22)
echo "blank=[${c[  ]}] arith=[$(( c[  ] + 1 ))] rc=$?"

echo done
