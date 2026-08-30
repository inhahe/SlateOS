# `trap` reads its operands by *count*, not by shape: "if arg is absent and a
# single signal_spec is supplied, the trap for that signal is reset to its
# original disposition". So a lone operand is a signal to reset, and only a
# second operand makes the first one an action — which is why `trap ':' USR1 INT`
# traps both signals but `trap USR1 INT` traps INT with the *command* `USR1`.
#
# A lone operand that names no signal has no reading left at all, so it falls
# through to the same usage error an action with no signals gets.
#
# (Numeric sigspecs are exercised through EXIT only: the number→name map is the
# host's, and this file has to read the same on every host.)

echo "=== a lone signal name resets that trap"
trap 'echo T' USR1
trap -p USR1
trap USR1
trap -p USR1; echo "  rc=$? (nothing above)"

echo "=== …including EXIT, which would otherwise fire at the end"
( trap 'echo E-fires' EXIT; trap EXIT; echo "  in subshell" )
( trap 'echo E-fires' EXIT; echo "  and without the reset:" )

echo "=== a lone 0 is EXIT by number"
( trap 'echo E-fires' EXIT; trap 0; echo "  in subshell" )

echo "=== two operands make the first one the action, even if it names a signal"
trap - USR1 INT
trap USR1 INT
trap -p
trap - USR1 INT

echo "=== the option terminator does not change the count"
trap 'echo T' INT
trap -- INT
trap -p INT; echo "  rc=$? (nothing above)"

echo "=== and a lone operand naming no signal is the usage error"
for a in BOGUS - '' ':' 'echo hi'; do
  ( trap "$a" ); echo "  [$a] rc=$?"
done

echo "=== a lone spec still resets when a trap was only inherited"
trap 'echo T' USR1
( trap USR1; trap -p USR1; echo "  rc=$? (nothing above)" )
trap -p USR1
trap - USR1

echo "=== resetting one of several leaves the others"
trap 'echo A' USR1
trap 'echo B' USR2
trap 'echo C' INT
trap USR2
trap -p
trap - USR1 USR2 INT
