# `unset` only checks that its operand is spelled like a variable when an
# explicit `-v` says the operand names one. Plain `unset X` and `unset -f X`
# take any word at all — the first because it falls back to the function
# namespace, the second because a function may be named anything — and `-n`
# takes any word too. Add a `-v` anywhere in the options and a word that is
# neither an identifier nor an element reference is reported as
# `` unset: `WORD': not a valid identifier `` with status 1.
#
# One rejected name never stops the rest: every bad operand is reported and
# every good one is still unset.

e() { sed -e 's/^.*: line [0-9]*: /SH: /' -e 's/^/    /'; }

echo "== 1. without -v anything goes"
for cmd in 'unset 1x' 'unset -f 1x' 'unset -n 1x' 'unset -- 1x' 'unset "a b"' 'unset ""'; do
  { eval "$cmd"; echo "  $cmd rc=$?"; } 2>&1 | e
done

echo "== 2. an explicit -v checks, wherever the v sits"
for cmd in 'unset -v 1x' 'unset -nv 1x' 'unset -vn 1x' 'unset -v -n 1x' 'unset -v -- 1x'; do
  { eval "$cmd"; echo "  $cmd rc=$?"; } 2>&1 | e
done

echo "== 3. an element reference is a name -v accepts"
n=(a b c)
declare -A m=([k]=1 [k v]=2)
{ unset -v 'n[1]'; echo "  rc=$? n=${n[*]}"; } 2>&1 | e
{ unset -v 'm[k v]'; echo "  rc=$? m=${#m[@]}"; } 2>&1 | e
{ unset -v 's[0]'; echo "  rc=$?"; } 2>&1 | e

echo "== 4. a malformed one is not, and is quoted whole"
for w in 'n[]' 'n[0]junk' '1x[0]' 'n[0' 'n]0[' '[0]'; do
  { unset -v "$w"; echo "  unset -v '$w' rc=$?"; } 2>&1 | e
done
echo "  n=${n[*]}"

echo "== 5. the empty operand and the lone signs"
for w in '' '-' '+' 'a b' 'a+' '@' '1' '_ok' 'ok_1'; do
  { unset -v "$w"; echo "  unset -v '$w' rc=$?"; } 2>&1 | e
done

echo "== 6. every bad name speaks, every good one is still unset"
a=A b=B
{ unset -v 1x a 2y b; echo "  rc=$? [${a-gone}][${b-gone}]"; } 2>&1 | e

echo "== 7. the check and the readonly refusal each take their turn"
ro=RO; readonly ro
{ unset -v ro 1x; echo "  rc=$?"; } 2>&1 | e
{ unset -v 1x ro; echo "  rc=$?"; } 2>&1 | e

echo "== 8. -v does not reach the function namespace"
q() { echo body; }
{ unset -v q; echo "  rc=$?"; q; } 2>&1 | e

echo "== 9. no operands at all"
{ unset -v; echo "  rc=$?"; } 2>&1 | e
{ unset; echo "  rc=$?"; } 2>&1 | e
