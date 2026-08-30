# `-n` re-declares what a name *means*: what it holds is another variable's
# name. So the value attributes the name arrived with go — they would fold or
# arithmetically mangle that name — while `-x`, `-t` and `-r`, which describe
# the binding rather than what is in it, stay. This command's own letters then
# apply on top, and they apply to the name the reference holds.
#
# `-i` is the one that cannot be honoured, since it reduces what is stored to a
# number and a number is no name to refer by: bash keeps the attributes it can,
# declines to make the reference, and refuses the assignment silently — the
# value it judged was one it computed rather than one that was written down.
# The arithmetic still runs, though, so a malformed target name is still a
# syntax error. And an array kind named beside `-n` simply takes the name for
# itself.
#
# The nameref questions are asked whatever else the command says, because bash
# asks them while the letter is being read.

echo "=== the attributes the name arrived with"
( declare -iu q; declare -n q=t; declare -p q ) 2>&1
( declare -l q; declare -n q=t; declare -p q ) 2>&1
( declare -x q; declare -t q; declare -n q=t; declare -p q ) 2>&1
( declare -r q; declare -n q=t; declare -p q ) 2>&1
( declare -i q; declare -n +n q=t; declare -p q ) 2>&1

echo "=== …and the ones this command names, on the name it refers by"
( declare -i q; declare -nu q=t; declare -p q ) 2>&1
( declare -nl v=TQ; declare -p v ) 2>&1
( declare -nc v=tq; declare -p v ) 2>&1
( declare -nu v='m[aB]'; declare -p v ) 2>&1
( declare -nu v=t; declare -nu v+=q; declare -p v ) 2>&1
( declare -nu v=t; v=5; declare -p T; declare -p t ) 2>&1
( declare -nu v=t; declare -p v; echo "rc=$?" ) 2>&1

echo "=== -i leaves the reference unmade, and says nothing"
( declare -ni v=t; echo "rc=$?"; declare -p v ) 2>&1
( declare -ni v=t; v=3+4; declare -p v ) 2>&1
( declare -x v; declare -ni v=t; declare -p v ) 2>&1
( f() { local -nix v=t; echo "rc=$?"; declare -p v; }; f ) 2>&1
( declare -n v=t; declare -ni v=w; declare -p v ) 2>&1
( declare -ni +i v=t; declare -p v ) 2>&1
( declare -ni v; declare -p v ) 2>&1
( declare -ni v=t ok=w; echo "rc=$?"; declare -p ok ) 2>&1

echo "=== …but the arithmetic still runs"
# …and a malformed target name is then the ordinary bad-`-i` failure: it
# discards the rest of the parse unit and leaves the name created-but-unset.
t='q+'
declare -ni zz=t ok2=1; echo NOT-REACHED
echo after
declare -p zz; echo "rc=$?"
declare -p ok2 2>&1; echo "rc=$?"

echo "=== an array kind beside -n takes the name for itself"
( declare -na v=t; declare -p v ) 2>&1
( declare -na v; declare -p v ) 2>&1
( declare -nA v=t; declare -p v ) 2>&1
( declare -n v=t; declare -na v=q; declare -p v ) 2>&1
( declare -i q; declare -na q=t; declare -p q ) 2>&1
( declare -na v='(1 2)'; echo "rc=$?" ) 2>&1

echo "=== the questions are asked whatever else is named"
( declare -a q; declare -n q=t; echo "rc=$?"; declare -p q ) 2>&1
( declare -A q; declare -nA q=t; echo "rc=$?" ) 2>&1
( declare -a q; declare -nA q=t; echo "rc=$?" ) 2>&1
( declare -ar q=(1); declare -n q=t; echo "rc=$?" ) 2>&1
( declare -a q; declare -nu q; echo "rc=$?" ) 2>&1
( declare -n BASH_SOURCE=t; echo "rc=$?" ) 2>&1
( declare -n BASH_LINENO=t; echo "rc=$?" ) 2>&1
( declare -a q; declare -n q=3; echo "rc=$?" ) 2>&1
( declare -nu 'r[1]'=t; echo "rc=$?" ) 2>&1

echo "=== a local shadows the array; a readonly name outranks it"
( declare -a q=(1); f() { local -n q=t; declare -p q; }; f; declare -p q ) 2>&1
( declare -ar q=(1); f() { local -n q=t; echo "rc=$?"; }; f ) 2>&1
( declare -r q=z; declare -nu q=t; echo "rc=$?"; declare -p q ) 2>&1
