# A `for` loop is the one write that does not go *through* a nameref: bash binds
# the reference itself, so each word becomes the reference's new referent. A
# referent has to name something, so a word that is not a name is refused — the
# loop stops there, with the reference still holding the last word that was one.
#
# `select` does not share this: its control variable is bound through the
# reference like any ordinary write, so the reference is left pointing where it
# was.

echo '=== a word that names something repoints the reference'
( aa=1; bb=2; declare -n r=zz; for r in aa bb; do echo "body $r"; done; echo "rc=$?"; declare -p r ) 2>&1
( x=(1 2); declare -n r=zz; for r in 'x[1]'; do echo "elem $r"; done; echo "rc=$?"; declare -p r ) 2>&1

echo '=== …and one that does not stops the loop where it stands'
( aa=1; declare -n r=zz; for r in aa 5 bb; do echo "body $r"; done; echo "rc=$?"; declare -p r ) 2>&1
( declare -n r=zz; for r in ''; do :; done; echo "blank rc=$?"; declare -p r ) 2>&1
( zz=keep; declare -n r=zz; for r in 5; do :; done; declare -p zz r ) 2>&1
( declare -n r=zz; for r in 5; do :; done; echo one; echo two ) 2>&1

echo '=== the refusal arms neither errexit nor the ERR trap'
( set -e; declare -n r=zz; for r in 5; do :; done; echo "after errexit" ) 2>&1
( trap 'echo ERR' ERR; declare -n r=zz; for r in 5; do :; done; echo "after trap rc=$?" ) 2>&1

echo '=== an empty list never asks, and a plain name never does either'
( declare -n r=zz; for r in; do :; done; echo "empty rc=$?" ) 2>&1
( for q in 5; do :; done; echo "plain rc=$? $q" ) 2>&1

echo '=== select binds through the reference instead, so it never asks'
( declare -n r=zz; select r in 5; do break; done <<< 1 >/dev/null; echo "rc=$? $(declare -p r)" ) 2>&1
( aa=1; declare -n r=zz; select r in aa; do break; done <<< 1 >/dev/null; echo "rc=$? $(declare -p r)" ) 2>&1

echo '=== and so do the other write paths'
( declare -n r=zz; read r <<< 5; echo "read: $zz $(declare -p r)" ) 2>&1
( declare -n r=zz; printf -v r 5; echo "printfv: $zz $(declare -p r)" ) 2>&1
( declare -n r=zz; r=5; echo "assign: $zz $(declare -p r)" ) 2>&1

echo still here
