# The base of a nameref that designates an array **element** — the `base` of
# `declare -n r='base[2]'` — is a *name*, and reading through the reference
# follows it through a nameref chain of its own. bash reaches the array with
# `array_variable_part`, which is `find_variable` on the base, and that chases
# references like any other lookup.
#
# So with `n=(a b c)` and `declare -n base=n`, `declare -n r='base[2]'` reads
# `c`. A chain that ends on a scalar reads the scalar at index 0; one that ends
# on an associative array takes the subscript as a key; one that ends *nowhere*
# — unset, or on an element rather than a variable — reads as nothing. A
# subscript that names nowhere blames the base the chain arrived at, not the one
# that was written.
#
# The chain is walked **twice**, so a circular one is reported twice: the name
# is resolved once to find the array and again to read out of it, which is the
# rule for a subscripted read reached the ordinary way (`${c1[0]}`).
#
# This is the *read* side only. The write side deliberately does not follow —
# see the companion case.

clean() {
  unset -n r base mid c1 c2 2>/dev/null
  unset -v r base mid c1 c2 n n2 mm s 2>/dev/null
}

echo '=== base is a nameref to an indexed array'
clean; n=(a b c); declare -n base=n; declare -n r='base[2]'
echo "[$r]"
clean; n=(a b c); declare -n base=n; declare -n r='base[0]'
echo "[$r]"
echo "--- negative counts back from the resolved array"
clean; n=(a b c); declare -n base=n; declare -n r='base[-1]'
echo "[$r]"

echo '=== two links deep'
clean; n=(a b c); declare -n mid=n; declare -n base=mid; declare -n r='base[1]'
echo "[$r]"

echo '=== base is a nameref to a scalar — index 0 is the scalar'
clean; s=SCALAR; declare -n base=s; declare -n r='base[0]'
echo "[$r]"
clean; s=SCALAR; declare -n base=s; declare -n r='base[1]'
echo "[${r-DEF}]"

echo '=== base is a nameref to an associative array — the subscript is a key'
clean; declare -A mm=([k]=K [0]=ZERO); declare -n base=mm; declare -n r='base[k]'
echo "[$r]"
clean; declare -A mm=([k]=K [0]=ZERO); declare -n base=mm; declare -n r='base[0]'
echo "[$r]"
echo "--- a key that is not there is unset, not the base's own name"
clean; declare -A mm=([k]=K); declare -n base=mm; declare -n r='base[zz]'
echo "[${r-DEF}]"

echo '=== the subscript is still late-bound through the resolved base'
clean; n=(a b c d); i=2; declare -n base=n; declare -n r='base[$i]'
echo "[$r]"; i=1; echo "[$r]"; i=3; echo "[$r]"

echo '=== whole-array subscripts through a nameref base'
clean; n=(abc 'c d' e); declare -n base=n; declare -n r='base[@]'
echo "[$r]"
clean; n=(abc 'c d' e); declare -n base=n; declare -n r='base[*]'
echo "[$r]"
clean; n=(abc 'c d' e); declare -n base=n; declare -n r='base[*]'
( IFS=; echo "[$r]" )
clean; n=(abc 'c d' e); declare -n base=n; declare -n r='base[@]'
( IFS=; echo "[$r]" )

echo '=== base names nothing'
echo "--- unset"
clean; declare -n base=nope; declare -n r='base[0]'
echo "[${r-DEF}]"
echo "--- an element rather than a variable"
clean; n=(a b c); declare -n base='n[1]'; declare -n r='base[0]'
echo "[${r-DEF}]"

echo '=== a plain (non-nameref) variable holding a name is NOT followed'
clean; n=(a b c); base=n; declare -n r='base[0]'
echo "[$r]"

echo '=== a bad subscript blames the base the chain arrived at'
clean; n=(a b c); declare -n base=n; declare -n r='base[-9]'
echo "[$r] s=$?"

echo '=== a circular base is reported twice and reads as nothing'
clean; declare -n c1=c2; declare -n c2=c1; declare -n r='c1[0]'
echo "[$r]"
clean; declare -n c1=c2; declare -n c2=c1; declare -n r='c1[@]'
echo "[$r]"
clean; declare -n c1=c2; declare -n c2=c1; declare -n r='c1[*]'
echo "[$r]"
clean; declare -n c1=c2; declare -n c2=c1; declare -n r='c1[0]'
echo "[${r-DEF}]"
echo "--- and the same read written out in full walks the same twice"
clean; declare -n c1=c2; declare -n c2=c1
echo "[${c1[0]}]"

echo '=== every operator that reads the value reads it through the chain'
clean; n=(a b c); declare -n base=n; declare -n r='base[2]'
echo "[${r^^}] [${r:0:1}] [${r/c/Z}] [${r+SET}] [${r:-D}] [${r@Q}]"

echo '=== declare -p keeps both spellings as written'
clean; n=(a b c); declare -n base=n; declare -n r='base[1]'
declare -p r base n

echo '=== the reference itself is unaffected by the base being a reference'
clean; n=(a b c); declare -n base=n; declare -n r='base[1]'
[[ -v r ]] && echo "v yes" || echo "v no"
echo "done"
