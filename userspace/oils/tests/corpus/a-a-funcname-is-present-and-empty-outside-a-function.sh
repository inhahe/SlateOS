# bash keeps `FUNCNAME` present at every level, exactly as it keeps
# `BASH_SOURCE` and `BASH_LINENO` — `declare -p` reports the name at a script's
# top level too. What differs is how the three were *built*: bash gives the
# other two an empty array, so they print `=()`, and creates `FUNCNAME` without
# ever assigning it, so it prints bare. That is the same never-assigned state a
# `declare -a q` leaves behind, and it reads out the same way: the name is
# reported by `declare -p` and passed over by every listing built from values.
#
# The expansion side does not follow the listing. `${FUNCNAME+SET}` is empty at
# the top level and `${#FUNCNAME[@]}` is 0, though the name is there to be
# reported — the two views genuinely disagree, and a function call settles them.
#
# `unset` is the one thing that tells the three apart for good: bash refuses to
# unset the two it assigned and lets `FUNCNAME` go, after which a later call
# finds nothing there.

echo "=== at top level"
declare -p FUNCNAME; echo "rc=$?"
echo "count=[$(declare -p | grep -c FUNCNAME)]"
declare -a | grep FUNCNAME
readonly -p | grep -c '\bFUNCNAME\b'
declare -i | grep -c '\bFUNCNAME\b'
echo "set=[${FUNCNAME+SET}] n=[${#FUNCNAME[@]}] keys=[${!FUNCNAME[*]}]"

echo "=== …but a value listing passes over it"
echo "star=[${!FUNCNAME*}] [${!BASH_SOURCE*}] [${!BASH_LINENO*}]"
echo "var=[$(compgen -A variable FUNCNAME)] arrayvar=[$(compgen -A arrayvar FUNCNAME)]"
f() { echo "in-fn star=[${!FUNCNAME*}] var=[$(compgen -A variable FUNCNAME)]"; }
f
echo "after star=[${!FUNCNAME*}]"

echo "=== a frame settles the two views"
g() { declare -p FUNCNAME; echo "set=[${FUNCNAME+SET}] n=[${#FUNCNAME[@]}]"; }
g
declare -p FUNCNAME

echo "=== it is an array, so a reference cannot point at it"
( declare -n FUNCNAME=t; echo "rc=$?" ) 2>&1
( declare -n BASH_SOURCE=t; echo "rc=$?" ) 2>&1

echo "=== the declarations it accepts"
( declare -a FUNCNAME; declare -p FUNCNAME ) 2>&1
( declare -i FUNCNAME; declare -p FUNCNAME ) 2>&1
( declare -A FUNCNAME; declare -p FUNCNAME ) 2>&1
( declare -r FUNCNAME; declare -p FUNCNAME ) 2>&1

echo "=== and the writes it swallows"
FUNCNAME=x;    echo "rc=$? [${FUNCNAME[*]}]"; declare -p FUNCNAME
FUNCNAME[2]=y; echo "rc=$? [${FUNCNAME[*]}]"; declare -p FUNCNAME
FUNCNAME+=(z); echo "rc=$? [${FUNCNAME[*]}]"; declare -p FUNCNAME
read FUNCNAME <<< 'q'; echo "rc=$? [${FUNCNAME[*]}]"

echo "=== a local shadows it with an ordinary name"
h() { local FUNCNAME; declare -p FUNCNAME; echo "[${FUNCNAME[*]}]"; }; h
i() { local BASH_SOURCE; declare -p BASH_SOURCE; }; i
j() { declare -p FUNCNAME; }; j

echo "=== a subshell and a command substitution keep the frame"
( declare -p FUNCNAME )
echo "[$(declare -p FUNCNAME)]"
k() { echo "[$(declare -p FUNCNAME)]"; }; k

echo "=== unset lets FUNCNAME go and refuses the other two"
unset FUNCNAME;    echo "rc=$?"
unset BASH_SOURCE; echo "rc=$?"
unset BASH_LINENO; echo "rc=$?"
declare -p FUNCNAME; echo "rc=$?"
l() { declare -p FUNCNAME 2>&1; echo "[${FUNCNAME[*]}] set=[${FUNCNAME+S}]"; }; l
echo "=== …and an assignment afterwards is an ordinary variable"
FUNCNAME=zz; declare -p FUNCNAME
m() { declare -p FUNCNAME; }; m
