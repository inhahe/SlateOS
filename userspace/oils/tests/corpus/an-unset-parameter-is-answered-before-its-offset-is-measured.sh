# `${x:off:len}` and `${a[@]:off:len}` measure nothing when there is nothing to
# measure. An unset scalar, an unset element, an empty array and a missing one
# evaluate neither the offset nor the length, so `${qq:i++}` leaves `i` alone and
# `${qq:0:-1}` is silent where the same length on something that *is* there is a
# fatal "substring expression < 0". Being empty is not being unset: `w=` has one
# position and takes none of these exits, and the positionals never do either,
# since `$0` heads their list even after `set --`.

declare -a a=(x y z)
declare -a e=()
declare -A m=([k]=v)
declare -A me=()
declare -n nref=missing
w=
v=xyz

echo "### the offset's own arithmetic reports whether it ran"
i=0; : ${qq:i++};        echo "  unset scalar      i=$i"
i=0; : ${qq[@]:i++};     echo "  unset via [@]     i=$i"
i=0; : ${a[9]:i++};      echo "  unset element     i=$i"
i=0; : ${m[nope]:i++};   echo "  unset assoc key   i=$i"
i=0; : ${e[@]:i++};      echo "  empty array       i=$i"
i=0; : ${me[@]:i++};     echo "  empty assoc       i=$i"
i=0; : ${zz[@]:i++};     echo "  missing array     i=$i"
i=0; : ${nref:i++};      echo "  nameref->missing  i=$i"
echo "  -- and the ones that are there:"
i=0; : ${v:i++};         echo "  set scalar        i=$i"
i=0; : ${w:i++};         echo "  empty scalar      i=$i"
i=0; : ${w[@]:i++};      echo "  empty via [@]     i=$i"
i=0; : ${a[1]:i++};      echo "  set element       i=$i"
i=0; : ${m[k]:i++};      echo "  set assoc key     i=$i"
i=0; : ${a[@]:i++};      echo "  set array         i=$i"
set --
i=0; : ${@:i++};         echo "  no positionals    i=$i"

echo "### the length is not reached either"
i=0; j=0; : ${qq:i++:j++};      echo "  unset scalar   i=$i j=$j"
i=0; j=0; : ${e[@]:i++:j++};    echo "  empty array    i=$i j=$j"
i=0; j=0; : ${zz[@]:i++:j++};   echo "  missing array  i=$i j=$j"
i=0; j=0; : ${w:i++:j++};       echo "  empty scalar   i=$i j=$j"
i=0; j=0; : ${w[@]:i++:j++};    echo "  empty via [@]  i=$i j=$j"
i=0; j=0; : ${a[@]:i++:j++};    echo "  set array      i=$i j=$j"
set -- p
i=0; j=0; : ${@:i++:j++};       echo "  positionals    i=$i j=$j"

echo "### so a negative length is fatal only when something is there"
(echo ${qq:0:-1});    echo "  unset scalar   rc=$?"
(echo ${a[9]:0:-1});  echo "  unset element  rc=$?"
(echo ${e[@]:0:-1});  echo "  empty array    rc=$?"
(echo ${me[@]:0:-1}); echo "  empty assoc    rc=$?"
(echo ${zz[@]:0:-1}); echo "  missing array  rc=$?"
(echo ${w:0:-1});     echo "  empty scalar   rc=$?"
(echo ${w[@]:0:-1});  echo "  empty via [@]  rc=$?"
(echo ${v:0:-9});     echo "  set scalar     rc=$?"
(echo ${a[@]:0:-1});  echo "  set array      rc=$?"
set -- p q
(echo ${@:0:-1});     echo "  positionals    rc=$?"

echo "### the values themselves are unchanged"
show() { printf '  %-14s(%d)' "$1" $(($# - 1)); shift; printf '<%s>' "$@"; printf '\n'; }
show 'qq:0'     ${qq:0}
show 'qq[@]:0'  ${qq[@]:0}
show 'e[@]:0'   ${e[@]:0}
show 'q e[@]:0' "${e[@]:0}"
show 'w:0'      ${w:0}
show 'q w[@]:0' "${w[@]:0}"
show 'v:1'      ${v:1}
show 'a[@]:1'   ${a[@]:1}

echo "### set -u still faults on the unset name"
(set -u; echo "[${qq:0}]");    echo "  rc=$?"
(set -u; echo "[${qq:0:1}]");  echo "  rc=$?"
(set -u; echo "[${w:0}]");     echo "  rc=$?"
(set -u; echo "[${e[@]:0}]");  echo "  rc=$?"
(set -u; echo "[${a[9]:0}]");  echo "  rc=$?"
(set -u; i=0; : ${qq:i++}; echo "  i=$i"); echo "  rc=$?"
