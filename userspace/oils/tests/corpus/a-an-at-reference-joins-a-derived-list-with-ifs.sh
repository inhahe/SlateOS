# The `[*]` half of this question is settled — every `[*]` reference joins with
# `$IFS` — and the `[@]` half looked like the mirror of it: `[@]` keeps fields
# apart, so where a *scalar* is wanted it must glue them back with a space.
#
# It does not. Only the parameter's own elements do that. `"${n[@]}"` is
# `p q r` under every `$IFS`, and so is `"${n[@]:-w}"` because a non-empty array
# makes it the same list — but every list an *operator* derives joins with the
# first character of `$IFS` exactly as the `[*]` spelling does. The keys, the
# names carrying a prefix, a slice, a transform, a case fold and a strip are all
# the derived kind:
#
#   IFS=:  "${n[@]}"    p q r          "${n[@]:0:2}"  p:q
#          "${n[@]:-w}" p q r          "${n[@]@Q}"    'p':'q':'r'
#
# Where the two spellings part company is an *empty* `$IFS`: `[*]` joins with
# nothing, `[@]` falls back to a space. So an empty `$IFS` makes every `[@]`
# form agree with the space-joining ones again, which is the reading that hides
# the rule if you only ever test with the default.

declare -a n=(p q r)
declare -A m=([k1]=v1)
pre1=A
pre2=B

show() { printf '  %-7s [%s]\n' "$1" "$2"; }

echo "=== the elements join with a space, whatever IFS says"
for i in ' ' ':' '' 'a' '=='; do
  echo "--- IFS=[$i]"
  ( IFS=$i; x="${n[@]}"; show elem "$x" )
  ( IFS=$i; x="${n[@]:-w}"; show defalt "$x" )
  ( IFS=$i; x="${n[@]:+y}"; show plus "$x" )
  ( IFS=$i; set -- s t u; x="$@"; show at "$x" )
done

echo "=== a derived list joins with IFS, and with a space when IFS is empty"
for i in ' ' ':' '' 'a' '=='; do
  echo "--- IFS=[$i]"
  ( IFS=$i; x="${!n[@]}"; show keys "$x" )
  ( IFS=$i; x="${!pre@}"; show names "$x" )
  ( IFS=$i; x="${n[@]:0:2}"; show slice "$x" )
  ( IFS=$i; x="${n[@]@Q}"; show quote "$x" )
  ( IFS=$i; x="${n[@]^^}"; show upper "$x" )
  ( IFS=$i; x="${n[@]#p}"; show strip "$x" )
  ( IFS=$i; x="${n[@]/q/Z}"; show subst "$x" )
  ( IFS=$i; x="${!m[@]}"; show akeys "$x" )
  ( IFS=$i; set -- s t u; x="${@:1:2}"; show pslice "$x" )
  ( IFS=$i; set -- s t u; x="${@@Q}"; show pquote "$x" )
done

echo "=== an unset IFS joins with a space either way"
( unset IFS; x="${n[@]}"; show elem "$x" )
( unset IFS; x="${!n[@]}"; show keys "$x" )
( unset IFS; x="${n[@]@Q}"; show quote "$x" )

echo "=== the star spelling of each, for the contrast"
for i in ':' ''; do
  echo "--- IFS=[$i]"
  ( IFS=$i; x="${n[*]}"; show elem "$x" )
  ( IFS=$i; x="${!n[*]}"; show keys "$x" )
  ( IFS=$i; x="${n[*]:0:2}"; show slice "$x" )
  ( IFS=$i; x="${n[*]@Q}"; show quote "$x" )
done

echo "=== one element is still a join with nothing to show for it"
declare -a one=(solo)
( IFS=:; x="${one[@]@Q}"; show one "$x" )
( IFS=:; x="${nope[@]@Q}"; show empty "$x" )
