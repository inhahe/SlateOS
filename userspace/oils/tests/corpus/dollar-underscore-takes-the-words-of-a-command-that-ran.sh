# `$_` records what just *ran*. A command discarded during word expansion never
# ran, so it leaves the previous binding alone — and a declaration builtin's
# compound operand contributes the bare name it declares, not the word.

shopt -s failglob

echo '=== an expansion error discards the command, and the binding with it'
echo alpha beta
echo "boom $(( 1/0 ))"
echo "  arith [$_]"
echo gamma delta
echo *.nopenope
echo "  failglob [$_]"
echo eta theta
arr=(*.nopenope)
echo "  failglob in a value [$_]"
echo cc dd
v=$(( 1/0 )) echo hi
echo "  prefix assignment [$_]"
echo mu nu
readonly c=1
c=2
echo "  readonly rejection [$_]"

echo "=== but a command that merely fails has run"
echo ii jj
nosuchcommandxyz arg1 arg2
echo "  not found [$_]"
echo kk ll
echo mm > nosuchdir/f
echo "  bad redirect [$_]"

echo "=== a command with no words binds the empty string"
echo oo pp
x=$( echo sub )
echo "  assignment [$_]"
echo qq rr
> outf
echo "  null command [$_]"
echo ss tt
< nosuchfile
echo "  failed null redirect [$_]"

echo "=== a compound operand contributes the name it declares"
declare -a a=(1 2); echo "  1 [$_]"
declare q=(1) w=2; echo "  2 [$_]"
declare w2=2 q2=(1); echo "  3 [$_]"
declare -x r=(1); echo "  4 [$_]"
export s=(1) t=9; echo "  5 [$_]"
declare -a u=(1 2) -x; echo "  6 [$_]"
