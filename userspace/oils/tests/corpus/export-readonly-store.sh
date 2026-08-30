# `export NAME=v` and `readonly NAME=v` assign — through the assignment.
#
# The value half of one of these operands is not a table write with an
# attribute bolted on: bash reaches the very code a bare `NAME=value` reaches,
# so everything that path does happens here too.
#
#   declare -i q; export q=3+4     →  7, not "3+4"
#   declare -i q=1; export q+=3+4  →  8, because `+=` on an integer *adds*
#   declare -u q; export q=ab      →  AB
#   export SECONDS=100             →  the clock really is set
#   declare -a a=(1 2); export a=zz →  element 0, not a scalar beside the array
#   set -x; export q=1             →  two trace lines, the builtin's and the
#                                     assignment's
#
# …and a malformed value under `-i` is an arithmetic *syntax* error, which
# discards the whole command — the operands after it are never reached and
# neither is the rest of the script. The builtin is named in the complaint,
# because that is where the assignment was written.
#
# The refusals these two have of their own come first and are unaffected: a
# readonly target reports `NAME: readonly variable` and keeps its value while
# still taking the attribute, and a name the shell maintains keeps its value
# silently and still reports success.
#
# `PPID` is masked because it differs between two shells on the same host; the
# mask spells no double quote and no backslash of its own (see known-issues
# TD-OILS-WIN-ARG-QUOTING).
m() { sed -E 's/=.[0-9]+.$/=Q/'; }

echo "=== the integer attribute evaluates the value"
( declare -i q;   export q=3+4;    declare -p q ) 2>&1
( declare -i q;   readonly q=3+4;  declare -p q ) 2>&1
( declare -i q=1; export q+=3+4;   declare -p q ) 2>&1
( declare -i q=1; readonly q+=3+4; declare -p q ) 2>&1
( declare -i q;   export -a q=3+4; declare -p q ) 2>&1
( export q=3+4; declare -p q ) 2>&1

echo "=== the case attributes fold it"
( declare -u q; export q=ab;   declare -p q ) 2>&1
( declare -l q; readonly q=AB; declare -p q ) 2>&1
( declare -c q; export q=ab;   declare -p q ) 2>&1
( declare -u q=A; export q+=b; declare -p q ) 2>&1

echo "=== an existing array is reached at element 0"
( declare -a a=(1 2);   export a=zz;   declare -p a ) 2>&1
( declare -a a=(1 2);   export a+=zz;  declare -p a ) 2>&1
( declare -a a=(1 2);   readonly a=zz; declare -p a ) 2>&1

echo "=== a dynamic special keeps its value function"
( export SECONDS=100;  echo "[$SECONDS]" ) 2>&1
( readonly SECONDS=50; echo "[$SECONDS]" ) 2>&1
( export HISTCMD=9;    declare -p HISTCMD ) 2>&1
( export LINENO=9
  declare -p LINENO ) 2>&1
( export RANDOM=1; a=$RANDOM; export RANDOM=1; b=$RANDOM
  [ "$a" = "$b" ] && echo stable || echo varying ) 2>&1
# …and a readonly one still refuses, naming itself.
( export PPID=9; echo "rc=$?"; declare -p PPID ) 2>&1 | m

echo "=== set -x traces the assignment as well as the builtin"
( set -x; export q=1 ) 2>&1
( set -x; readonly q=1 ) 2>&1
( set -x; declare -i q; export q=3+4 ) 2>&1

echo "=== a malformed -i value discards the command"
( declare -i q; export q=3+;   echo "rc=$?"; declare -p q; echo unreachable ) 2>&1
( declare -i q; readonly q=3+; echo unreachable ) 2>&1
( declare -i q; export q=3+ ok=1; echo unreachable ) 2>&1; declare -p ok 2>&1

echo "=== the refusals these two have of their own still come first"
( readonly q=1; export q=2;   echo "rc=$?"; declare -p q ) 2>&1
( readonly q=1; export -a q=2; echo "rc=$?"; declare -p q ) 2>&1
( readonly q=1; readonly q=2; echo "rc=$?"; declare -p q ) 2>&1
( export UID=5;   echo "rc=$?"; declare -p UID ) 2>&1 | m
( readonly UID=5; echo "rc=$?" ) 2>&1

echo "=== a value that arrived expanded is not expanded again"
( HOME=/h; export Q=~/a:'~/b'; echo "$Q" ) 2>&1
( HOME=/h; readonly R=~/r;     echo "$R" ) 2>&1
( export S='$notavar'; echo "$S" ) 2>&1
( export T='a b'; declare -p T ) 2>&1
( export U='*'; declare -p U ) 2>&1
