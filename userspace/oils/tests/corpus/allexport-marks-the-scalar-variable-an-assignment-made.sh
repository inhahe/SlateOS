# `set -a` does not mark "whatever an assignment touched": it marks the *scalar
# variable the assignment made*. Three things follow from that wording.
#
#   * A write whose shape is an array — a subscripted operand, or a compound
#     `(…)` literal — makes an array, and an array has no representation in the
#     environment, so nothing is marked.
#   * A write the name turns away — a dynamic special, which has an assign
#     function instead of a cell — makes no variable at all, so again nothing is
#     marked, however scalar the operand looked.
#   * The shape that counts is the one *as written*, not as resolved: a nameref
#     whose target is an array element still spells a plain name, so its target
#     is marked.
#
# A declaration builtin is judged by its own words: its compound operand is
# marked only when it did not name `-a`/`-A` and is not making a local — and
# `declare -n` marks the reference from its own declaration, before any write
# through it.

echo '=== every scalar-shaped write marks the name'
( set -a; zz=5; d=$(declare -p zz); echo "assign: ${d%%=*}" ) 2>&1
( set -a; read zz <<< 5; d=$(declare -p zz); echo "read: ${d%%=*}" ) 2>&1
( set -a; printf -v zz 5; d=$(declare -p zz); echo "printfv: ${d%%=*}" ) 2>&1
( set -a; for zz in 5; do :; done; d=$(declare -p zz); echo "for: ${d%%=*}" ) 2>&1
( set -a; ((zz=5)); d=$(declare -p zz); echo "arith: ${d%%=*}" ) 2>&1
( set -a; set -- -a; getopts a zz; d=$(declare -p zz); echo "getopts: ${d%%=*}" ) 2>&1
( set -a; select zz in a; do break; done </dev/null >/dev/null; d=$(declare -p zz); echo "select: ${d%%=*}" ) 2>&1
( set -a; : ${zz:=5}; d=$(declare -p zz); echo "default: ${d%%=*}" ) 2>&1
( set -a; n=zz; : ${!n:=5}; d=$(declare -p zz); echo "default indirect: ${d%%=*}" ) 2>&1
( declare -n r=q; set -a; : ${r:=5}; d=$(declare -p q); echo "default nameref: ${d%%=*}" ) 2>&1
( set -a; f() { local zz; : ${zz:=5}; declare -p zz; }; f ) 2>&1

echo '=== …and an array-shaped one makes an array, which is marked by nothing'
( set -a; zz[1]=9; d=$(declare -p zz); echo "elem: ${d%%=*}" ) 2>&1
( set -a; zz=(1 2); d=$(declare -p zz); echo "compound: ${d%%=*}" ) 2>&1
( set -a; declare -A mm; mm[k]=9; d=$(declare -p mm); echo "assoc elem: ${d%%=*}" ) 2>&1
( set -a; read 'zz[1]' <<< 9; d=$(declare -p zz); echo "read elem: ${d%%=*}" ) 2>&1
( set -a; read -a zz <<< '1 2'; d=$(declare -p zz); echo "read -a: ${d%%=*}" ) 2>&1
( set -a; printf -v 'zz[1]' 9; d=$(declare -p zz); echo "printfv elem: ${d%%=*}" ) 2>&1
( set -a; : ${zz[1]:=5}; d=$(declare -p zz); echo "default elem: ${d%%=*}" ) 2>&1
( set -a; declare -A mm; : ${mm[k]:=5}; d=$(declare -p mm); echo "default key: ${d%%=*}" ) 2>&1

echo '=== a write the name turns away makes no variable, so marks none either'
( set -a; read SECONDS <<< 5; d=$(declare -p SECONDS); echo "read: ${d%%=*}" ) 2>&1
( set -a; SECONDS=5; d=$(declare -p SECONDS); echo "assign: ${d%%=*}" ) 2>&1
( set -a; ((BASH_SUBSHELL=7)); d=$(declare -p BASH_SUBSHELL); echo "arith: ${d%%=*}" ) 2>&1
( set -a; LINENO=99; d=$(declare -p LINENO); echo "lineno: ${d%%=*}" ) 2>&1
( set -a; : ${RANDOM:=5}; d=$(declare -p RANDOM); echo "default: ${d%%=*}" ) 2>&1
( set -a; export SECONDS; d=$(declare -p SECONDS); echo "explicit: ${d%%=*}" ) 2>&1

echo '=== a declaration builtin is judged by the words it used'
( set -a; declare zz=5; d=$(declare -p zz); echo "scalar: ${d%%=*}" ) 2>&1
( set -a; declare zz=(1 2); d=$(declare -p zz); echo "compound: ${d%%=*}" ) 2>&1
( set -a; declare -a zz=(1 2); d=$(declare -p zz); echo "named -a: ${d%%=*}" ) 2>&1
( set -a; declare -A mm=([k]=1); d=$(declare -p mm); echo "named -A: ${d%%=*}" ) 2>&1
( set -a; declare zz[1]=5; d=$(declare -p zz); echo "subscript: ${d%%=*}" ) 2>&1
( set -a; f() { declare zz=(1 2); declare -p zz; }; f ) 2>&1
( set -a; f() { local zz=(1 2); declare -p zz; }; f ) 2>&1
( set -a; f() { local zz=5; declare -p zz; }; f ) 2>&1
( set -a; f() { declare -g zz=5; }; f; d=$(declare -p zz); echo "global: ${d%%=*}" ) 2>&1

echo '=== a nameref marks its target, and marks itself when declared'
( set -a; declare -n r=q; d=$(declare -p r); echo "declared: ${d%%=*}" ) 2>&1
( declare -n r=q; set -a; r=5; d=$(declare -p q); echo "through: ${d%%=*}" ) 2>&1
( declare -n r=q; set -a; r[1]=5; d=$(declare -p q); echo "through elem: ${d%%=*}" ) 2>&1
( x=(1 2); declare -n r=x[1]; set -a; r=9; d=$(declare -p x); echo "at element: ${d%%=*}" ) 2>&1
( set -a; f() { local -n r=q; declare -p r; }; f ) 2>&1

echo '=== the mark stays once made, and an existing one is not taken away'
( export zz=1; set -a; zz[1]=9; d=$(declare -p zz); echo "widened: ${d%%=*}" ) 2>&1
( set -a; zz=5; set +a; zz=6; d=$(declare -p zz); echo "kept: ${d%%=*}" ) 2>&1

echo still here
