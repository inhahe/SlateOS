# An arithmetic subscript is *not* expanded like an ordinary word. bash hands it
# to `expand_arith_string`, which does parameter and command substitution and
# removes **double** quotes — but leaves **single** quotes alone, so the `'`
# reaches the arithmetic evaluator, which has no such token.
#
# The upshot is a pair that looks symmetric and is not: `a["1"]` is index 1,
# `a['1']` is a syntax error. Getting this wrong is not cosmetic — `a['']`
# silently becomes index 0 and overwrites the first element of the array.
#
# An *associative* subscript is a different animal entirely: it is a word, so
# single quotes there really do quote (that is how a key with spaces is
# written). Both rules are exercised below.

echo "=== indexed write path ==="
a=(0 1 2)
a['1']=x; echo "unreachable-1"
echo "sq st=$? keys=${!a[*]} vals=${a[*]}"
b=(0 1 2)
b["1"]=y
echo "dq st=$? v=${b[1]}"
# The empty case is the dangerous one: `''` must be a syntax error, not 0.
c=(0 1 2)
c['']=z; echo "unreachable-2"
echo "sq-empty st=$? n=${#c[@]} first=${c[0]}"
# …whereas a genuinely empty double-quoted subscript *is* index 0.
d=(0 1 2)
d[""]=z
echo "dq-empty st=$? n=${#d[@]} first=${d[0]}"

echo "=== indexed read path ==="
e=(0 1 2)
echo "dq=${e["1"]}"
echo "${e['1']}"; echo "unreachable-3"
echo "sq st=$?"
echo "len=${#e["1"]}"

echo "=== quotes in the middle of an expression ==="
f=(0 1 2)
f[1'+'1]=p; echo "unreachable-4"
echo "mid-sq st=$? keys=${!f[*]}"
g=(0 1 2)
g[1"+"1]=p
echo "mid-dq st=$? keys=${!g[*]} v=${g[2]}"

echo "=== a quote that arrives by expansion is rejected too ==="
# The evaluator, not the parser, is what refuses the `'` — so a variable whose
# value contains quotes fails in exactly the same way.
h=(0 1 2)
k="'1'"
h[$k]=w; echo "unreachable-5"
echo "viavar st=$?"

echo "=== substring and slice bounds follow the same rule ==="
s=abcdef
echo "${s:'1'}"; echo "unreachable-6"
echo "off st=$?"
echo "${s:0:'2'}"; echo "unreachable-7"
echo "len st=$?"
arr=(0 1 2 3)
echo "${arr[@]:'1'}"; echo "unreachable-8"
echo "slice st=$?"
# Double quotes are removed here as well, so these are ordinary bounds.
echo "dq-off=${s:"1"} dq-len=${s:0:"2"}"

echo "=== associative subscripts really are quoted words ==="
declare -A m
m['two words']=w
m['']=nope; echo "unreachable-9"
echo "assoc-empty st=$? n=${#m[@]}"
m["dq key"]=v
echo "spaces=${m['two words']} dq=${m["dq key"]} n=${#m[@]}"
# A key of `1` written three ways is the same key, because quote removal runs.
declare -A n
n[1]=a
n['1']=b
n["1"]=c
echo "assoc-n=${#n[@]} val=${n[1]}"

echo "=== a declare -i value is a word, so its quotes are removed ==="
# Contrast with the subscript: here the shell's own quote removal happens
# first, so the evaluator never sees the quotes at all.
declare -i q
q='1'+1
echo "decl-int=$q st=$?"

echo done
