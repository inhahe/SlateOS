# Declaring a dynamic variable does not cost it its value function.
#
# `SECONDS`, `RANDOM`, `PPID` and the rest of the family hold no stored value:
# each read calls a function that computes one. Naming one in a declaration
# builtin — `export SECONDS`, `readonly SECONDS`, `declare -u RANDOM`, even a
# bare `declare SECONDS` — does not create a variable, because there already is
# one. bash applies the attribute to the binding that is there and stops, so the
# name goes on computing its value and goes on carrying the attributes the shell
# gave it:
#
#   export SECONDS   →   declare -ix SECONDS="12"
#
# the `i` from the shell, the `x` from the command, and a value that is still
# climbing. An `export`ed one really does reach a child's environment, with the
# value its function computes at the moment the child is started.
#
# What the declaration *does* change is that the name's slot is now filled in.
# These slots start out empty — no value, and for `SECONDS` not even the `-i` —
# so the listings that walk the variable table pass over them, while
# `declare -p NAME` reports the full form all along. Naming one in a declaration
# is a lookup like any other, and a lookup runs the value function, so from then
# on the listings carry the name too, in the same full form. (A plain read does
# it as well — see dynamic-var-visible.sh, which is about nothing else; so
# nothing below expands one before it looks at a listing.)
#
# A `local` of the name is a different thing entirely: it shadows the dynamic
# binding with an ordinary, unset variable, and the shell's own comes back when
# the function returns. `unset` drops the declaration along with everything else
# (see dynamic-var-unset.sh).
#
# Values that differ between two shells on the same host are masked throughout.
# The mask spells no double quote and no backslash of its own: osh cannot yet
# hand either to an external command on the Windows host it is developed on —
# see known-issues TD-OILS-WIN-ARG-QUOTING.
m() { sed -E -e 's/=.[0-9.]+.$/=Q/' -e 's/^SECONDS=[0-9.]+$/SECONDS=Q/'; }

echo "=== the attribute is applied to the binding that is already there"
( export SECONDS; declare -p SECONDS ) 2>&1 | m
( readonly SECONDS; declare -p SECONDS ) 2>&1 | m
( declare -u SECONDS; declare -p SECONDS ) 2>&1 | m
( declare -t SECONDS; declare -p SECONDS ) 2>&1 | m
( declare -l SECONDS; declare -p SECONDS ) 2>&1 | m
( declare SECONDS; declare -p SECONDS ) 2>&1 | m
( export SECONDS; export -n SECONDS; declare -p SECONDS ) 2>&1 | m
( export PPID; declare -p PPID ) 2>&1 | m
( declare -u RANDOM; declare -p RANDOM ) 2>&1 | m
( declare -x SECONDS; declare -p SECONDS ) 2>&1 | m
( typeset -r SECONDS; declare -p SECONDS ) 2>&1 | m

echo "=== and the value function is still there"
( export SECONDS; a=$SECONDS; sleep 1; b=$SECONDS
  [ "$a" != "$b" ] && echo climbing || echo "stuck [$a]" )
( declare RANDOM; a=$RANDOM; b=$RANDOM
  [ "$a" != "$b" ] && echo varying || echo "stable [$a]" )
( readonly SECONDS; a=$SECONDS; sleep 1; b=$SECONDS
  [ "$a" != "$b" ] && echo climbing || echo "stuck [$a]" )

echo "=== so an exported one really does reach a child"
( export SECONDS; env | grep -c '^SECONDS=' )
( export RANDOM; env | grep -c '^RANDOM=' )
( export LINENO; env | grep -c '^LINENO=' )
# …and it is the computed value that goes across, not an empty one.
( export SECONDS; sleep 1; env | grep '^SECONDS=' ) | m
( export SECONDS; unset SECONDS; env | grep -c '^SECONDS=' )

echo "=== the name starts appearing in the listings"
# Untouched, `SECONDS` is missing from all three; `RANDOM` is in `declare -i`
# already, but without its value.
( declare -i | grep ' SECONDS' ; echo "rc=$?" ) | m
( set | grep '^SECONDS=' ; echo "rc=$?" ) | m
( declare -i | grep ' RANDOM' ; echo "rc=$?" ) | m
( declare SECONDS; declare -i | grep ' SECONDS' ) | m
( declare SECONDS; set | grep '^SECONDS=' ) | m
( declare RANDOM; declare -i | grep ' RANDOM' ) | m
( declare SECONDS; declare -p | grep ' SECONDS' ) | m
( export SECONDS; declare -x | grep ' SECONDS' ) | m
( export SECONDS; export -p | grep ' SECONDS' ) | m
( export SECONDS; declare -i | grep ' SECONDS' ) | m
( readonly SECONDS; readonly -p | grep ' SECONDS' ) | m
( declare -u RANDOM; declare -u | grep ' RANDOM' ) | m
# A declaration that only names the *global* form does it too.
( f() { declare -g SECONDS; }; f; declare -i | grep ' SECONDS' ) | m

echo "=== a local shadows it with an ordinary variable instead"
( f() { local SECONDS; declare -p SECONDS; echo "[${SECONDS-UNSET}]"; }; f
  declare -p SECONDS ) 2>&1 | m
( f() { local SECONDS=zz; declare -p SECONDS; }; f; declare -p SECONDS ) 2>&1 | m
( f() { local -i SECONDS; declare -p SECONDS; }; f ) 2>&1 | m

echo "=== an assignment still shadows it, declared or not"
( declare SECONDS; SECONDS=zz; declare -p SECONDS ) 2>&1 | m
( export SECONDS; SECONDS=9; declare -p SECONDS ) 2>&1 | m

echo "=== readonly means readonly"
( readonly SECONDS; SECONDS=5; echo unreachable ) 2>&1
( readonly SECONDS; unset SECONDS; echo "rc=$?" ) 2>&1
( readonly RANDOM; declare -p RANDOM ) 2>&1 | m

echo "=== and none of it escapes the subshell that did it"
( export SECONDS ); declare -p SECONDS 2>&1 | m
( declare SECONDS ); declare -i | grep -c ' SECONDS'
