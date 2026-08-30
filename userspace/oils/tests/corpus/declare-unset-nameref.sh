# `declare +n` names two different variables at once: the reference, for the
# letter itself, and the target it leads to, for everything else in the operand.
#
# The refusals are grouped through `e` rather than redirected per command,
# because osh emits some of them before the command's own redirections are in
# place (TD-OILS-DECL-DIAGNOSTIC-ESCAPES-REDIRECTION).
e() { sed 's/^.*: line [0-9]*: //'; }
p() { echo -n "$1 -> "; declare -p "$2" 2>&1; }

echo "=== every other letter follows the reference"
( w=5; declare -n r=w; declare -x +n r; p 'w' w; p 'r' r ) 2>&1 | e
( w=5; declare -n r=w; declare -i +n r; p 'w' w; p 'r' r ) 2>&1 | e
( w=5; declare -n r=w; declare -a +n r; p 'w' w; p 'r' r ) 2>&1 | e
( w=5; declare -n r=w; declare -r +n r; p 'w' w; p 'r' r ) 2>&1 | e
( w=5; declare -n r=w; declare -u +n r; p 'w' w; p 'r' r ) 2>&1 | e
( w=5; declare -n r=w; declare -t +n r; p 'w' w; p 'r' r ) 2>&1 | e

echo "=== …including the ones that take an attribute away"
( w=5; declare -x w; declare -n r=w; declare +x +n r; p 'w' w; p 'r' r ) 2>&1 | e

echo "=== and so does the value"
( w=5; declare -n r=w; declare +n r=9; p 'w' w; p 'r' r ) 2>&1 | e
( w=5; declare -n r=w; declare +n r=; p 'w' w; p 'r' r ) 2>&1 | e

echo "=== a subscript is not a request, so +n alone leaves the target alone"
# …where the same subscript under any other letter makes the target's array.
( w=5; declare -n r=w; declare +n r; p 'w' w; p 'r' r ) 2>&1 | e
( w=5; declare -n r=w; declare +n 'r[1]'; p 'w' w; p 'r' r ) 2>&1 | e
( w=5; declare -n r=w; declare -x +n 'r[1]'; p 'w' w; p 'r' r ) 2>&1 | e
( w=5; declare -n r=w; declare +n 'r[1]=9'; p 'w' w; p 'r' r ) 2>&1 | e
# Nor is `-g`, which only says which binding the declaration writes.
( w=5; declare -n r=w; declare -g +n r; p 'w' w; p 'r' r ) 2>&1 | e

echo "=== the target need not exist"
( declare -n r=w; declare +n r; p 'w' w; p 'r' r ) 2>&1 | e
( declare -n r=w; declare -x +n r; p 'w' w; p 'r' r ) 2>&1 | e

echo "=== the letter comes off the *last* reference in the chain"
( w=5; declare -n m=w; declare -n r=m; declare -x +n r
  p 'w' w; p 'm' m; p 'r' r ) 2>&1 | e
( w=5; declare -n c=w; declare -n m=c; declare -n r=m; declare +n r
  p 'w' w; p 'c' c; p 'm' m; p 'r' r ) 2>&1 | e
( w=5; declare -n m=w; declare -n r=m; declare +n r=9
  p 'w' w; p 'm' m; p 'r' r ) 2>&1 | e
# Asked about the middle of a chain, the walk starts there.
( w=5; declare -n m=w; declare -n r=m; declare -x +n m
  p 'w' w; p 'm' m; p 'r' r ) 2>&1 | e

echo "=== -n in the same command suppresses the follow"
( w=5; declare -n m=w; declare -n r=m; declare -n +n r; p 'w' w; p 'm' m; p 'r' r ) 2>&1 | e

echo "=== a local binding never follows, so it is its own name that loses it"
f1() { local w=5; local -n r=w; declare -x +n r; p 'w' w; p 'r' r; }
( f1 ) 2>&1 | e
f2() { local w=5; local -n r=w; local +n r; p 'w' w; p 'r' r; }
( f2 ) 2>&1 | e
# …and it is the whole chain's own name, not the last link's.
f3() { local w=5; local -n m=w; local -n r=m; declare -x +n r
       p 'w' w; p 'm' m; p 'r' r; }
( f3 ) 2>&1 | e
# A frame that merely *shadows* a global reference is the same story.
gw=7; declare -n gr=gw
f4() { declare -x +n gr; p 'gw' gw; p 'gr' gr; }
( f4; p 'gw' gw; p 'gr' gr ) 2>&1 | e
# …unless `-g` says the global binding is the one being written.
f5() { declare -g -x +n gr; }
( f5; p 'gw' gw; p 'gr' gr ) 2>&1 | e

echo "=== a refused operand keeps the attribute it would have lost"
( readonly w=5; declare -n r=w; declare +n r=9; echo "rc=$?"; p 'w' w; p 'r' r ) 2>&1 | e
( declare -A w=([k]=v); declare -n r=w; declare -a +n r; echo "rc=$?"; p 'w' w; p 'r' r ) 2>&1 | e
( declare -n r=GROUPS; declare +n r=9; echo "rc=$?"; p 'r' r ) 2>&1 | e

echo "=== +n on a name that is no reference declares it like any other operand"
( w=5; declare +n w; p 'w' w ) 2>&1 | e
( declare +n fresh; echo "rc=$?"; p 'fresh' fresh ) 2>&1 | e

echo "=== a reference onto an element leads the other letters to its base"
( declare -a a=(1 2); declare -n r='a[1]'; declare +n r; p 'a' a; p 'r' r ) 2>&1 | e

echo "=== done"
