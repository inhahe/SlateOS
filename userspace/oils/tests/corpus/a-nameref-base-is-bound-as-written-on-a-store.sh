# The base of a nameref that designates an **element** is bound where it is
# written, not followed. bash reaches the array for a store with
# `find_or_make_array_variable`, which looks the base up with
# `find_variable_noref` — so a nameref sitting on the base is *not* chased:
# it is warned about, its attribute and value are stripped, and the freshly
# plain base becomes the array that is stored into.
#
# This is the opposite of the read side, where `array_variable_part` is
# `find_variable` and does chase (see
# a-nameref-base-is-followed-when-the-reference-is-read.sh), and of `unset`,
# which chases too. Only the store binds where it is written.
#
# Everything downstream is then about that base rather than about whatever it
# used to point at: its value attributes, its readonly state, and its kind —
# which is why a base pointing at an *associative* array still ends up as an
# indexed one, with the key evaluated as arithmetic.

echo '=== the store binds the base as written'
( n=(a b c); declare -n base=n; declare -n r='base[0]'
  r=V; declare -p base n 2>&1 )
echo '=== another subscript, same rule'
( n=(a b c); declare -n base=n; declare -n r='base[2]'
  r=V; declare -p base n 2>&1 )
echo '=== two links deep: the first link is the one that is bound'
( n=(a b c); declare -n mid=n; declare -n base=mid; declare -n r='base[0]'
  r=V; declare -p base mid n 2>&1 )
echo '=== the strip happens once; a second write finds a plain array'
( n=(a b c); declare -n base=n; declare -n r='base[0]'
  r=V; r=W; declare -p base n 2>&1 )
echo '=== a base that is not a reference is stored into directly'
( n=(a b c); base=n; declare -n r='base[0]'
  r=V; declare -p base n 2>&1 )
echo '=== a base naming nothing is made all the same'
( declare -n base=nosuch; declare -n r='base[0]'
  r=T; echo "s=$?"; declare -p base nosuch 2>&1 )

echo '=== every store path binds the same way'
echo '--- read'
( n=(a b c); declare -n base=n; declare -n r='base[0]'
  read -r r <<< RR; declare -p base n 2>&1 )
echo '--- printf -v'
( n=(a b c); declare -n base=n; declare -n r='base[0]'
  printf -v r PP; declare -p base n 2>&1 )
echo '--- arithmetic'
( n=(1 2 3); declare -n base=n; declare -n r='base[0]'
  (( r = 7 )); echo "s=$?"; declare -p base n 2>&1 )
echo '--- arithmetic compound'
( n=(1 2 3); declare -n base=n; declare -n r='base[1]'
  (( r += 40 )); echo "s=$?"; declare -p base n 2>&1 )
echo '--- += on the word'
( n=(a b c); declare -n base=n; declare -n r='base[0]'
  r+=X; declare -p base n 2>&1 )
echo '--- an env prefix'
( n=(a b c); declare -n base=n; declare -n r='base[0]'
  r=P declare -p base n 2>&1 )

echo '=== the strip comes before anything else judges the base'
echo '--- whole array is still a bad subscript, after the warning'
( n=(a b c); declare -n base=n; declare -n r='base[@]'
  r=V; echo "s=$?"; declare -p base n 2>&1 )
echo '--- and with a star'
( n=(a b c); declare -n base=n; declare -n r='base[*]'
  r=V; echo "s=$?"; declare -p base n 2>&1 )
echo '--- a bad subscript still blames the written base'
( n=(a b c); declare -n base=n; declare -n r='base[1+]'
  r=V; echo "s=$?"; declare -p base n 2>&1 )

echo '=== the readonly that matters is the one on the base as written'
echo '--- readonly on the base'
( n=(a b c); declare -n base=n; declare -n r='base[0]'; readonly base
  r=V; echo "s=$?"; declare -p base n 2>&1 )
echo '--- readonly on the array the base used to name'
( n=(a b c); readonly n; declare -n base=n; declare -n r='base[0]'
  r=V; echo "s=$?"; declare -p base n 2>&1 )
echo '--- readonly on the reference itself'
( n=(a b c); declare -n base=n; declare -n r='base[0]'; readonly r
  r=V; echo "s=$?"; declare -p base n 2>&1 )

echo '=== the kind and the attributes are the fresh base own'
echo '--- an associative base becomes an indexed one'
( declare -A mm=([k]=K); declare -n base=mm; declare -n r='base[k]'
  r=T; echo "s=$?"; declare -p base mm 2>&1 )
echo '--- an integer attribute on the old target does not carry'
( declare -ia n=(1 2 3); declare -n base=n; declare -n r='base[0]'
  r=5+5; declare -p base n 2>&1 )
echo '--- an uppercase attribute on the old target does not carry'
( declare -a n=(a b c); declare -u n; declare -n base=n; declare -n r='base[0]'
  r=vv; declare -p base n 2>&1 )

echo '=== the read side still follows'
( n=(a b c); declare -n base=n; declare -n r='base[0]'
  echo "[$r]"; declare -p base n 2>&1 )
echo '=== and a read that does not store leaves the base a reference'
( n=(a b c); declare -n base=n; declare -n r='base[0]'
  echo "[${r:=D}]"; declare -p base n 2>&1 )
echo '=== a written subscript follows its base, as it always did'
( n=(a b c); declare -n base=n; base[0]=V; declare -p base n 2>&1 )
( n=(a b c); declare -n base=n; declare -n r=base; r[1]=V; declare -p base r n 2>&1 )
echo '=== and so does a compound literal, and a plain scalar'
( n=(a b c); declare -n base=n; base=(x y); declare -p base n 2>&1 )
( n=hello; declare -n base=n; base=V; declare -p base n 2>&1 )
