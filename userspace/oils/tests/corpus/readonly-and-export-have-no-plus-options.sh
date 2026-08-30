# `declare`/`typeset`/`local` read a `+x` word as "take that attribute off".
# `readonly` and `export` do not read it at all: to them it is an *operand*, so
# it draws `` `+x': not a valid identifier ``, it fails the command, and — being
# a non-option word — it ends the option scan, leaving every flag written behind
# it an operand as well.
#
# The attribute the builtin is *named* for goes on regardless, which is the part
# a `+`-reading shell would get wrong twice over: `export +x e=(1)` still
# exports, and `readonly +a q=(1)` still leaves an ordinary indexed array rather
# than refusing to "destroy" one.
#
# `-n` is the one letter these two spell their own way (see
# readonly-export-n-flag.sh); it is a `-` word, so it is unaffected by any of
# this.

echo "=== a plus word is an operand, not a flag"
v1=1; readonly +x v1; echo "  readonly +x: rc=$?"; declare -p v1
v2=1; export +n v2;   echo "  export +n:   rc=$?"; declare -p v2
readonly +p;          echo "  readonly +p: rc=$?"

echo "=== and the declare family still reads it"
declare -a x=(1); declare +a x; echo "  declare +a: rc=$?"
declare -x y=1;   declare +x y; echo "  declare +x: rc=$?"; declare -p y

echo "=== with a compound operand, where the flag scan is the same scan"
readonly +a q=(1); echo "  readonly +a q=(1): rc=$?"; declare -p q
export +x e=(1);   echo "  export +x e=(1):   rc=$?"; declare -p e

echo "=== a plus word ends the scan, so a flag behind it is an operand too"
readonly -a +x s=(1); echo "  readonly -a +x: rc=$?"; declare -p s
readonly +x -a t=(1); echo "  readonly +x -a: rc=$?"; declare -p t

echo "=== and a flag written behind a compound operand was already one"
readonly u=(1) +x; echo "  readonly u=(1) +x: rc=$?"; declare -p u

echo "=== the plain -a/-A flags are untouched"
readonly -A m=([k]=v); echo "  readonly -A: rc=$?"; declare -p m
export -a n=(1 2);     echo "  export -a:   rc=$?"; declare -p n
