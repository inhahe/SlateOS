# The companion to `a-joined-context-joins-a-slice-and-a-key-list-with-a-space`,
# for the per-element operators — where bash splits the family in half. In a
# context that cannot make more than one field (an assignment's value, a
# redirect target), an unquoted `[@]` list joins with a **space** for the
# case-conversion (`^ ^^ , ,,`) and replacement (`/ //`) operators, but with
# `$IFS` for trim (`# ## % %%`) and transform (`@Q @E @P @a`). One would expect
# a uniform rule and there isn't one; the line is by operator, and measured.
#
# Quoting flips the space back to `$IFS`: `x="${n[@]^^}"` is `A:B:C:D` where
# `x=${n[@]^^}` is `A:B C:D`. So the space belongs to the *unquoted* spelling,
# unlike the slice's space, which survives quoting-by-context. The trim and
# transform rows are indifferent to quoting and so read the same either way.
#
# The `[*]` spellings are never on this path: they are $IFS's first character
# whatever the operator and whatever the quoting.

declare -a n=(a:b c:d)
declare -a u=(A:B C:D)
SAVE=$IFS

for lbl in default colon null space-colon; do
  echo "=== IFS $lbl"
  case $lbl in
    default) IFS=$SAVE ;;
    colon) IFS=: ;;
    null) IFS= ;;
    space-colon) IFS=' :' ;;
  esac

  # The half that takes a space.
  a=${n[@]^};      echo "  \${n[@]^}      [$a]"
  a=${n[@]^^};     echo "  \${n[@]^^}     [$a]"
  a=${u[@],};      echo "  \${u[@],}      [$a]"
  a=${u[@],,};     echo "  \${u[@],,}     [$a]"
  a=${n[@]/a/Z};   echo "  \${n[@]/a/Z}   [$a]"
  a=${n[@]//:/Z};  echo "  \${n[@]//:/Z}  [$a]"

  # The half that keeps $IFS.
  a=${n[@]#a};     echo "  \${n[@]#a}     [$a]"
  a=${n[@]##a};    echo "  \${n[@]##a}    [$a]"
  a=${n[@]%d};     echo "  \${n[@]%d}     [$a]"
  a=${n[@]%%d};    echo "  \${n[@]%%d}    [$a]"
  a=${n[@]@Q};     echo "  \${n[@]@Q}     [$a]"
  a=${n[@]@E};     echo "  \${n[@]@E}     [$a]"
  a=${n[@]@P};     echo "  \${n[@]@P}     [$a]"
  a=${n[@]@a};     echo "  \${n[@]@a}     [$a]"

  # Quoting takes the space away from the first half and leaves the second
  # half alone.
  a="${n[@]^^}";   echo "  \"\${n[@]^^}\"   [$a]"
  a="${n[@]/a/Z}"; echo "  \"\${n[@]/a/Z}\" [$a]"
  a="${n[@]#a}";   echo "  \"\${n[@]#a}\"   [$a]"
  a="${n[@]@Q}";   echo "  \"\${n[@]@Q}\"   [$a]"

  # `[*]` is $IFS's first character throughout.
  a=${n[*]^^};     echo "  \${n[*]^^}     [$a]"
  a=${n[*]/a/Z};   echo "  \${n[*]/a/Z}   [$a]"
  a=${n[*]#a};     echo "  \${n[*]#a}     [$a]"
  a="${n[*]^^}";   echo "  \"\${n[*]^^}\"   [$a]"
done
IFS=$SAVE

echo "=== the positionals answer as a named array does"
set -- a:b c:d
IFS=:
a=${@^^};   echo "  \${@^^}    [$a]"
a=${@/a/Z}; echo "  \${@/a/Z}  [$a]"
a=${@#a};   echo "  \${@#a}    [$a]"
a=${@@Q};   echo "  \${@@Q}    [$a]"
a="${@^^}"; echo "  \"\${@^^}\"  [$a]"
a=${*^^};   echo "  \${*^^}    [$a]"
IFS=$SAVE

echo "=== the other spellings of an assignment"
declare -a t=(- -)
IFS=:
t[0]=${n[@]^^};        echo "  t[0]=          [${t[0]}]"
export E=${n[@]^^};    echo "  export E=      [$E]"
declare D=${n[@]/a/Z}; echo "  declare D=     [$D]"
lp() { local L=${n[@]^^}; echo "  local L=       [$L]"; }
lp
rp() ( readonly R=${n[@]/a/Z}; echo "  readonly R=    [$R]" )
rp
declare -A h; h[k]=${n[@]^^}; echo "  h[k]=          [${h[k]}]"
IFS=$SAVE

echo "=== a redirect target is one word too"
mkdir -p rd && cd rd
IFS=:
: > ${n[@]^^}
for f in *; do echo "  file [$f]"; done
rm -f -- *
: > ${n[@]#a}
for f in *; do echo "  file [$f]"; done
rm -f -- *
cd ..
IFS=$SAVE

echo "=== the join is a join, not a rewrite of the elements"
declare -a c=('a b' 'c:d')
IFS=:
a=${c[@]^^}; echo "  colon [$a]"
IFS=
a=${c[@]^^}; echo "  null  [$a]"
IFS=$SAVE
a=${c[@]^^}; echo "  deflt [$a]"

echo "=== an empty or one-element list joins nothing"
declare -a z=()
declare -a o=('q:r')
IFS=:
a=${z[@]^^}; echo "  empty [$a]"
a=${o[@]^^}; echo "  one   [$a]"
a=${z[@]#a}; echo "  e trim[$a]"
IFS=$SAVE
