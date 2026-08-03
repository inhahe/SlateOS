# `let` evaluates each of its operands as a separate arithmetic expression, left
# to right, and reports a status derived from the **last** one: zero becomes 1
# and anything else becomes 0. That inversion is the whole point of the builtin
# and the one thing about it that trips people up — `let x=0` "fails" even
# though the assignment happened, and `let 1 0` fails while `let 0 1` succeeds.
#
# Around that:
#
#   * Each operand is one *word*, so quoting decides how much of it is one
#     expression: `let "y = 2 + 3"` is a single expression, `let y = 2 + 3` is
#     four. Word splitting applies to an unquoted expansion, so `e='4+4'; let
#     z=$e` still works only because the pieces do not contain blanks.
#   * An expression **error** — a syntax error, a bad token, a division by zero
#     — is reported and yields 1, but does not abandon anything: `let` and
#     `(( ))` are the two arithmetic contexts that merely fail. (An *integer
#     assignment*, `declare -i k=1/0`, is the one that discards the command.)
#   * `let` with no operands at all is `expression expected`, status 1.
#   * Because a false result is a failing command, both `let n=0` and `(( 0 ))`
#     are fatal under `set -e`.
#   * The assignment writes through everything a normal one does — an array
#     subscript, the `-i` attribute, a nameref.

echo "=== the status is the last expression's value, inverted"
let 1;     echo "  let 1         -> $?"
let 0;     echo "  let 0         -> $?"
let 1 0;   echo "  let 1 0       -> $?"
let 0 1;   echo "  let 0 1       -> $?"
let -1;    echo "  let -1        -> $?"
let x=5;   echo "  let x=5       -> $? x=$x"
let x=0;   echo "  let x=0       -> $? x=$x"
let 2>&1;  echo "  let           -> $?"

echo "=== each operand is its own expression, left to right"
let a=1 b=a+1 c=b+1; echo "  a=$a b=$b c=$c"
# The earlier operands still run when a later one is false…
let d=7 0; echo "  let d=7 0 -> $? d=$d"
# …and when a later one *errors*, the earlier ones have already been applied.
let g=8 'h=1/0' i=9 2>&1; echo "  rc=$? g=$g h=$h i=${i:-unset}"

echo "=== an operand is one word, so quoting decides the grouping"
let "y = 2 + 3"; echo "  quoted:   y=$y"
e="4+4"; let z=$e; echo "  expanded: z=$z"
let 'w = 1 , 2'; echo "  comma:    w=$w rc=$?"
let "s = 1 + 2" "t = s * 2"; echo "  two:      s=$s t=$t"

echo "=== errors report and fail, but abandon nothing"
let 'q = 1 +' 2>&1; echo "  rc=$?"
let '1bad='   2>&1; echo "  rc=$?"
let 'x/0'     2>&1; echo "  rc=$?"
let 'v = )'   2>&1; echo "  rc=$?"; echo "  the line after continues"

echo "=== let and (( )) agree on the inversion"
(( 0 )); echo "  (( 0 ))   -> $?"
(( 1 )); echo "  (( 1 ))   -> $?"
(( 0, 3 )); echo "  (( 0, 3 )) -> $?"

echo "=== so both are fatal under set -e"
( set -e; let n=0; echo "  NOT REACHED" ); echo "  let n=0  rc=$?"
( set -e; (( 0 )); echo "  NOT REACHED" ); echo "  (( 0 ))  rc=$?"
( set -e; let n=1; echo "  reached" );     echo "  let n=1  rc=$?"
# An erroring expression is a failure too, so errexit takes that as well.
( set -e; let 'n=1/0' 2>/dev/null; echo "  NOT REACHED" ); echo "  let 1/0  rc=$?"
# But `let` in a condition is tested, not fatal.
( set -e; if let n=0; then echo "  then"; else echo "  else"; fi ); echo "  in if rc=$?"

echo "=== the assignment writes through the usual machinery"
declare -a arr; let 'arr[2] = 7'; echo "  arr[2]=${arr[2]} arr[0]=${arr[0]:-unset}"
declare -i iv;  let 'iv = 3 * 3'; echo "  iv=$iv"
declare -n ref=target; let 'ref = 4'; echo "  target=$target"
declare -A m; let 'm[k] = 5' 2>&1; echo "  m[k]=${m[k]} rc=$?"
r=1; readonly r; let 'r = 2' 2>&1; echo "  readonly r=$r rc=$?"

echo "=== let is an ordinary builtin, so the wrappers reach it"
command let q=3; echo "  command let: q=$q rc=$?"
builtin let q=0; echo "  builtin let: q=$q rc=$?"
let -- 5 2>&1;   echo "  let -- 5   rc=$?"
