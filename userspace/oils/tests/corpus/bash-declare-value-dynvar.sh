# A declaration builtin's `NAME=value` operand is the ordinary assignment.
#
# bash reaches the same code `name=value` does, so everything that path does
# happens here too: the integer attribute evaluates the value, the case
# attributes fold it, `set -a` marks the name for export — and a name the shell
# *computes* keeps its assign function, so `declare SECONDS=7` really does
# rebase the clock and `declare BASH_SUBSHELL=9` really does move the counter,
# rather than laying an ordinary variable over the top of them.
#
# `local` is the exception, and not by way of the store: a `local` of one of
# these names makes an *ordinary* variable in the frame, because bash's value
# and assign functions belong to the global. Inside the function the name
# neither computes on a read nor reaches the counter on a write; the shell's own
# comes back when the frame pops. `declare -g` names the global and so keeps it.

echo "=== the store is the assignment's"
( declare -i q; declare q=3+4; echo "[$q]" ) 2>&1
( declare -u q; declare q=ab;  echo "[$q]" ) 2>&1
( declare -i q=10; declare q+=3+4; echo "[$q]" ) 2>&1
( set -a; declare q=1; declare -p q ) 2>&1
# A malformed expression discards the command, and every operand after it.
( declare -i q; declare q=3+ r=1; echo "rc=$?"; echo "[${r-UNSET}]"; declare -p q ) 2>&1

echo "=== so a computed name keeps computing"
( declare BASH_SUBSHELL=9; echo "[$BASH_SUBSHELL]"; ( echo "[$BASH_SUBSHELL]" ) ) 2>&1
( declare SECONDS=7; echo "[$SECONDS]" ) 2>&1
( declare -i SECONDS=3+4; echo "[$SECONDS]" ) 2>&1
( declare LINENO=7; echo "[$LINENO]" ) 2>&1
( typeset BASH_SUBSHELL=4; echo "[$BASH_SUBSHELL]" ) 2>&1
# Reseeding is the assign function's own side effect, so it has to have run.
( declare RANDOM=1; a=$RANDOM; declare RANDOM=1; b=$RANDOM
  if [ "$a" = "$b" ]; then echo "reseeded"; else echo "not reseeded"; fi ) 2>&1

echo "=== but a local of one is an ordinary variable"
( f() { local BASH_SUBSHELL=9; echo "[$BASH_SUBSHELL]"; }; f; echo "[$BASH_SUBSHELL]" ) 2>&1
# What the *outer* read has to show is that the shell's own live counter is back
# rather than the 9 the frame held. Its actual value is however long this script
# has been running — 0 or 1 depending on the machine, which made this case flake
# — so ask whether it is a small elapsed count instead of printing it.
( f() { local SECONDS=9; echo "[$SECONDS]"; }; f; echo "[$(( SECONDS < 5 ))]" ) 2>&1
( f() { local SECONDS; SECONDS=7; echo "[$SECONDS]"; }; f; echo "[$(( SECONDS < 5 ))]" ) 2>&1
( f() { local SECONDS; echo "[${SECONDS-UNSET}]"; }; f ) 2>&1
( f() { local LINENO=7; echo "[$LINENO]"; }; f ) 2>&1
# Only `-g` names the global, which is the computed one; a bare `declare` in a
# frame is a `local` and shadows it just the same.
( f() { declare -g BASH_SUBSHELL=9; }; f; echo "[$BASH_SUBSHELL]" ) 2>&1
( f() { declare BASH_SUBSHELL=9; echo "[$BASH_SUBSHELL]"; }; f; echo "[$BASH_SUBSHELL]" ) 2>&1

echo "=== and the trace shows one line here where export shows two"
( set -x; declare q=1 ) 2>&1
( set -x; typeset q=1 ) 2>&1
( set -x; f() { local q=1; }; f ) 2>&1
( set -x; export q=1 ) 2>&1
( set -x; readonly q=1 ) 2>&1
