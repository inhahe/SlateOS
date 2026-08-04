# `${!a[@]}` is the *keys* of `a` only while it stands alone. Give it any
# operator and it stops being the key listing and becomes ordinary indirection
# whose pointer happens to be `a[@]`: bash reads the whole of `a` as one string
# and uses that as the name of the variable the operator then works on. So
# `one=(v); v=hello` makes `${!one[@]#h}` the value `ello`, not a key with an
# `h` stripped off it, and a multi-element pointer usually names nothing legal.
#
# The join that builds the name is the *derived* one, which is the part that
# contradicts the spelling: written out, `x="${a[@]}"` glues the elements with a
# space whatever `$IFS` says, but read through a pointer the same `a[@]` glues
# them with `$IFS`. Only the empty-`$IFS` fallback to a space is shared, and it
# is what lets the `[*]` spelling — which joins with nothing — build a name out
# of pieces that `[@]` cannot.
#
# Three ways of having no name are three different outcomes: an unset array had
# nothing to point with and is the fatal "invalid indirect expansion"; an array
# that is set but *empty* points nowhere, which is not an error at all; and one
# empty element really does resolve to the empty name, which is not a legal one.

declare -a one=(v)
declare -a two=(v v)
declare -A am=([k]=v)
declare -a arr=(A B C)
v=hello

echo "=== bare, the keys — and with an operator, a pointer"
printf '[%s]\n' "${!one[@]}" "${!one[*]}" "${!am[@]}"
printf '[%s]\n' "${!one[@]#h}" "${!one[*]#h}" "${!am[@]#h}" "${!am[k]#h}"
printf '[%s]\n' "${!one[@]:1:3}" "${!one[@]/l/L}" "${!one[@]%o}" "${!one[@]/#/X}"
printf '[%s]\n' "${!one[@]@Q}" "${!one[@]@a}" "${!one[@]@A}" "${!one[@]:+set}"

echo "=== the name is built with IFS, unlike the same reference written out"
for i in ' ' ':' '' 'v'; do
  echo "--- IFS=[$i]"
  ( IFS=$i; x="${two[@]}"; printf '  written [%s]\n' "$x" )
  ( IFS=$i; printf '  at      [%s]\n' "${!two[@]#h}" ) 2>&1
  ( IFS=$i; printf '  star    [%s]\n' "${!two[*]#h}" ) 2>&1
done

echo "=== so an empty IFS lets the star spelling glue a name together"
declare -a hh=(he llo)
( IFS=; printf '  at   [%s]\n' "${!hh[@]#h}" ) 2>&1
( IFS=; printf '  star [%s]\n' "${!hh[*]#h}" ) 2>&1

echo "=== unset, empty, and an empty element are three different answers"
unset nope
declare -a mt=()
declare -a earr=('')
( printf '  unset [%s]\n' "${!nope[@]#x}"; echo "  st=$?" ) 2>&1
( printf '  empty [%s]\n' "${!mt[@]#x}";   echo "  st=$?" ) 2>&1
( printf '  blank [%s]\n' "${!earr[@]#x}"; echo "  st=$?" ) 2>&1
( set -u; printf '  nounset [%s]\n' "${!mt[@]#x}"; echo "  st=$?" ) 2>&1
( printf '  default [%s]\n' "${!mt[@]:-d}"; echo "  st=$?" ) 2>&1
( printf '  alt     [%s]\n' "${!mt[@]:+p}"; echo "  st=$?" ) 2>&1

echo "=== everything downstream is the ordinary indirection"
declare -a pt=(tgt)
( printf '  assign [%s]\n' "${!pt[@]:=set}"; printf '  tgt=[%s]\n' "$tgt" ) 2>&1
( printf '  error  [%s]\n' "${!pt[@]:?boom}"; echo "  st=$?" ) 2>&1
declare -n nr=v
declare -a np=(nr)
printf '  nameref [%s]\n' "${!np[@]#h}" 2>&1
w=arr
printf '  scalar-ptr [%s]\n' "${!w[@]}" "${!w[@]#A}" 2>&1
declare -a toarr=(arr)
printf '  to-array [%s]\n' "${!toarr[@]#A}" "${!toarr[0]}" 2>&1

echo "=== a name that is not a name says so, quoting the value"
printf '[%s]\n' "${!two[@]#h}" 2>&1; echo "st=$?"
declare -a three=(x y z)
printf '[%s]\n' "${!three[@]:0:2}" 2>&1; echo "st=$?"

echo "=== the shapes that are still refused"
printf '[%s]\n' "${#!one[@]}" 2>&1
al1=p; al2=q
printf '[%s]\n' "${!al@}" "${!al*}" 2>&1
printf '[%s]\n' "${!al@Q}" 2>&1
printf '[%s]\n' "${!al*/a/Z}" 2>&1
