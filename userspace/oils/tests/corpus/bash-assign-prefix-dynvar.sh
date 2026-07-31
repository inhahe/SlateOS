# An assignment prefix binds a name in a scope of its own, so for the command's
# duration one the shell would otherwise *compute* is an ordinary variable.
#
# bash's value and assign functions belong to the global variable, and the
# prefix's binding is not it — so while the prefix stands the name reads back
# what was written rather than what the shell computes, an assignment to it
# writes that binding rather than moving the counter, and when the command ends
# the shell's own comes back untouched. `local` does the same thing by the same
# route; the prefix is that rule applied to a command instead of a frame.
#
# What the prefix does *not* do is parse or fold the value: it is the string as
# written, however the global was declared.

# `$SECONDS` after the prefix is compared rather than printed throughout: the
# point is that the shell's own clock is back, not which second it has reached,
# and the `sleep 1` below sets it running.
back() { [ "$SECONDS" -lt 100 ] && echo "[back]" || echo "[still $SECONDS]"; }

echo "=== the binding is what the command reads"
( SECONDS=100 eval 'echo "[$SECONDS]"'; back ) 2>&1
( LINENO=9 eval 'echo "[$LINENO]"' ) 2>&1
( BASH_SUBSHELL=9 eval 'echo "[$BASH_SUBSHELL]"'; echo "[$BASH_SUBSHELL]" ) 2>&1
( BASHPID=5 eval 'echo "[$BASHPID]"' ) 2>&1
( EPOCHSECONDS=5 eval 'echo "[$EPOCHSECONDS]"' ) 2>&1
( BASH_ARGV0=zz eval 'echo "[$BASH_ARGV0]"' ) 2>&1
# …and it reads the same however often, where the computed one would not.
( RANDOM=5 eval 'echo "[$RANDOM] [$RANDOM]"' ) 2>&1
( LINENO=9 eval 'echo "[$LINENO]"; echo "[$LINENO]"' ) 2>&1
( SECONDS=100 eval 'sleep 1; echo "[$SECONDS]"' ) 2>&1

echo "=== a function's body is inside it too, and a subshell of one"
( f() { echo "[$SECONDS]"; }; SECONDS=100 f; back ) 2>&1
( f() { echo "[$BASH_SUBSHELL]"; }; BASH_SUBSHELL=9 f; echo "[$BASH_SUBSHELL]" ) 2>&1
( SECONDS=100 eval 'f() { echo "[$SECONDS]"; }; f; ( echo "[$SECONDS]" )' ) 2>&1

echo "=== an assignment under it writes the binding, not the counter"
( SECONDS=100 eval 'SECONDS=5; echo "[$SECONDS]"'; back ) 2>&1
( BASH_SUBSHELL=9 eval 'BASH_SUBSHELL=5; echo "[$BASH_SUBSHELL]"'; echo "[$BASH_SUBSHELL]" ) 2>&1

echo "=== unset empties the scope, so what it hid comes back at once"
( q=1; q=2 eval 'unset q; echo "[${q-UNSET}]"'; echo "[${q-UNSET}]" ) 2>&1
( SECONDS=100 eval 'unset SECONDS; back' ) 2>&1
( BASH_SUBSHELL=9 eval 'unset BASH_SUBSHELL; echo "[${BASH_SUBSHELL-UNSET}]"' ) 2>&1
( LINENO=9 eval 'unset LINENO; echo "[$LINENO]"' ) 2>&1
# …where unsetting a `local` of one leaves it unset for the frame, and the
# shell's own is still there when the frame pops: the value function belongs to
# the global either way, and only the shadow was ever removed.
( f() { local SECONDS=9; unset SECONDS; echo "[${SECONDS-UNSET}]"; }; f; back ) 2>&1
( f() { local BASH_SUBSHELL=9; unset BASH_SUBSHELL; echo "[${BASH_SUBSHELL-UNSET}]"; }
  f; echo "[$BASH_SUBSHELL]" ) 2>&1

echo "=== and the string is taken as written"
( SECONDS=zz eval 'echo "[$SECONDS]"' ) 2>&1
( SECONDS=3+4 eval 'echo "[$SECONDS]"' ) 2>&1
( declare -i SECONDS; SECONDS=3+4 eval 'echo "[$SECONDS]"' ) 2>&1

echo "=== because the binding is a fresh, exported variable of its own"
# `-x` always — it *is* the command's environment — and none of the global's
# attributes come with it, which is the same fact as the string above being
# taken as written: the `-i` never reached the value.
( q=2 eval 'declare -p q' ) 2>&1
( q=1; q=2 eval 'declare -p q'; declare -p q ) 2>&1
( declare -i q; q=3+4 eval 'declare -p q'; declare -p q ) 2>&1
( declare -u q=ab; q=cd eval 'declare -p q'; declare -p q ) 2>&1
( declare -a q=(1 2); q=zz eval 'declare -p q'; declare -p q ) 2>&1
( SECONDS=100 eval 'declare -p SECONDS' ) 2>&1

echo "=== a prefix on a special builtin does not persist either"
( SECONDS=100 :; back ) 2>&1
( SECONDS=100 eval :; back ) 2>&1
( BASH_SUBSHELL=9 :; echo "[$BASH_SUBSHELL]" ) 2>&1

echo "=== but an external really is given it"
( SECONDS=100 env | grep '^SECONDS=' ) 2>&1
( BASH_SUBSHELL=9 env | grep '^BASH_SUBSHELL=' ) 2>&1
