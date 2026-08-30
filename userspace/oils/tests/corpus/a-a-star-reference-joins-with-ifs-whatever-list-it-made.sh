# `$IFS` decides how a `[*]` reference joins, and *every* list a reference can
# make answers to it — not just the elements. The keys (`${!a[*]}`), the names
# carrying a prefix (`${!pre*}`), a slice (`${a[*]:1:2}`), an element-wise
# transform (`${a[*]@Q}`) and the `${a[*]:-…}` family all join with the first
# character of `$IFS`, exactly as `${a[*]}` itself does. It is one rule about
# the *reference*, so what the list is made of never enters into it.
#
# The first character, so an unset `IFS` joins with a space and an empty one
# joins with nothing. (Whether that first character can be multi-byte is a
# question about the locale rather than about the reference, so it is not asked
# here.)
#
# Only the star form asks. The `[@]` spelling of any of those references is one
# field per element, so `"${a[@]@Q}"` is three fields under every `IFS` — which
# is why the counts are printed alongside.

declare -a n=(x y z)
declare -A m=([k1]=v1)
zz1=1; zz2=2

show() { printf '  %-6s count=%s' "$1" "$#"; shift; printf ' <%s>' "$@"; echo; }

echo "=== every star form joins, and joins the same way"
for i in ':' '' ' ' 'a' '::'; do
  echo "--- IFS=[$i]"
  ( IFS=$i; show 'elem' "${n[*]}" )
  ( IFS=$i; show 'keys' "${!n[*]}" )
  ( IFS=$i; show 'names' "${!zz*}" )
  ( IFS=$i; show 'slice' "${n[*]:0:3}" )
  ( IFS=$i; show 'quote' "${n[*]@Q}" )
  ( IFS=$i; show 'upper' "${n[*]^^}" )
  ( IFS=$i; show 'defalt' "${n[*]:-D}" )
  ( IFS=$i; show 'assoc' "${m[*]}" "${!m[*]}" )
done

echo "=== …and no [@] form joins at all"
for i in ':' '' ' ' 'a'; do
  echo "--- IFS=[$i]"
  ( IFS=$i; show 'elem' "${n[@]}" )
  ( IFS=$i; show 'keys' "${!n[@]}" )
  ( IFS=$i; show 'names' "${!zz@}" )
  ( IFS=$i; show 'slice' "${n[@]:0:3}" )
  ( IFS=$i; show 'quote' "${n[@]@Q}" )
  ( IFS=$i; show 'defalt' "${n[@]:-D}" )
done

echo "=== an unset IFS is a space, not an empty separator"
( unset IFS; show 'elem' "${n[*]}" )
( unset IFS; show 'keys' "${!n[*]}" )
( unset IFS; show 'quote' "${n[*]@Q}" )

echo "=== the positional forms are the same reference"
set -- 'a b' c
for i in ':' '' 'a'; do
  ( IFS=$i; show 'star' "$*" )
  ( IFS=$i; show 'at' "$@" )
  ( IFS=$i; show 'sl*' "${*:1:2}" )
  ( IFS=$i; show 'q*' "${*@Q}" )
  ( IFS=$i; show 'q@' "${@@Q}" )
done

echo "=== one element is still a join, and none is still no field"
declare -a one=(solo)
declare -a none=()
( IFS=:; show 'one' "${one[*]@Q}" )
( IFS=:; show 'none' "${none[*]@Q}" )
( IFS=:; show 'none@' "${none[@]@Q}" )
( IFS=:; show 'nokey' "${!none[*]}" )
