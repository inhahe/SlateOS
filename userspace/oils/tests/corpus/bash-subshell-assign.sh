# `$BASH_SUBSHELL` is a counter, not a read-only mirror of the depth.
#
# Assigning it writes the very number the shell increments on the way into a
# subshell, so the number given is what this level reads and each level further
# in reads one more:
#
#   BASH_SUBSHELL=9; echo $BASH_SUBSHELL      →  9
#   BASH_SUBSHELL=9; ( echo $BASH_SUBSHELL )  →  10
#
# The string is read as a plain decimal — blanks around a whole number and
# nothing else, anything else 0 — and it stays one even under `declare -i`,
# because the shell's own assign function for this name never asks about the
# integer attribute where `SECONDS`'s does. An *appending* assignment is
# arithmetic either way: the appended value is formed before the assign
# function is reached.
#
# Nothing ordinary is left behind by the assignment: the name goes on
# computing, which is what makes the reading move with the depth at all.

echo "=== the number given is what this level reads"
( BASH_SUBSHELL=9; echo "[$BASH_SUBSHELL]" ) 2>&1
( BASH_SUBSHELL=9; ( echo "[$BASH_SUBSHELL]" ) ) 2>&1
( BASH_SUBSHELL=9; ( ( echo "[$BASH_SUBSHELL]" ) ) ) 2>&1
( BASH_SUBSHELL=9; echo "[$(echo $BASH_SUBSHELL)]" ) 2>&1
( BASH_SUBSHELL=0; echo "[$BASH_SUBSHELL]"; ( echo "[$BASH_SUBSHELL]" ) ) 2>&1
# …and the counter a subshell moves is its own.
( ( BASH_SUBSHELL=9 ); echo "[$BASH_SUBSHELL]" ) 2>&1

echo "=== the string is read as a plain decimal"
( BASH_SUBSHELL=-3; echo "[$BASH_SUBSHELL]"; ( echo "[$BASH_SUBSHELL]" ) ) 2>&1
( BASH_SUBSHELL=' 4 '; echo "[$BASH_SUBSHELL]" ) 2>&1
( BASH_SUBSHELL=zz;   echo "[$BASH_SUBSHELL]" ) 2>&1
( BASH_SUBSHELL=3x;   echo "[$BASH_SUBSHELL]" ) 2>&1
( BASH_SUBSHELL=;     echo "[$BASH_SUBSHELL]" ) 2>&1
( BASH_SUBSHELL=0x10; echo "[$BASH_SUBSHELL]" ) 2>&1
( BASH_SUBSHELL=010;  echo "[$BASH_SUBSHELL]" ) 2>&1
( BASH_SUBSHELL=99999999999999999999; echo "[$BASH_SUBSHELL]" ) 2>&1
( BASH_SUBSHELL=3+4;  echo "[$BASH_SUBSHELL]" ) 2>&1

echo "=== the integer attribute does not reach the value"
( declare -i BASH_SUBSHELL; BASH_SUBSHELL=3+4; echo "[$BASH_SUBSHELL]" ) 2>&1
( declare -i BASH_SUBSHELL; BASH_SUBSHELL=7;   echo "[$BASH_SUBSHELL]" ) 2>&1
# …where the same on `SECONDS`, whose assign function does ask, evaluates.
( declare -i SECONDS; SECONDS=3+4; echo "[$SECONDS]" ) 2>&1

echo "=== but an appending one is arithmetic with it and a string without"
( declare -i BASH_SUBSHELL; BASH_SUBSHELL+=5; echo "[$BASH_SUBSHELL]" ) 2>&1
( declare BASH_SUBSHELL; BASH_SUBSHELL+=5; echo "[$BASH_SUBSHELL]" ) 2>&1
# The value cell an append runs together with is filled by a *reading*, never
# by an assignment, so a pristine one appends to nothing at all.
( BASH_SUBSHELL+=5; echo "[$BASH_SUBSHELL]" ) 2>&1
( BASH_SUBSHELL=7; BASH_SUBSHELL+=5; echo "[$BASH_SUBSHELL]" ) 2>&1
( BASH_SUBSHELL=7; : $BASH_SUBSHELL; BASH_SUBSHELL+=5; echo "[$BASH_SUBSHELL]" ) 2>&1

echo "=== the assignment leaves nothing ordinary behind"
( BASH_SUBSHELL=9; declare -p BASH_SUBSHELL ) 2>&1
( declare -i BASH_SUBSHELL; declare -p BASH_SUBSHELL ) 2>&1
( BASH_SUBSHELL=9; unset BASH_SUBSHELL; echo "[$BASH_SUBSHELL]"; ( echo "[$BASH_SUBSHELL]" ) ) 2>&1
( export BASH_SUBSHELL=9; echo "[$BASH_SUBSHELL]" ) 2>&1
( readonly BASH_SUBSHELL=9; echo "rc=$?"; echo "[$BASH_SUBSHELL]" ) 2>&1
( declare -n r=BASH_SUBSHELL; r=9; echo "[$BASH_SUBSHELL]" ) 2>&1
