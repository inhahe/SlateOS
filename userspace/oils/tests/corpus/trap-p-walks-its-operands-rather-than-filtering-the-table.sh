# `trap -p` has two shapes that are not the same walk. With no operands it lists
# the whole trap table, in the order bash's own signal table has it. With
# operands it walks the *operand list* instead — so the lines come out in the
# order they were asked for rather than in signal order, a signal named twice is
# printed twice, and a word that names no signal is an error rather than a
# silent skip.

echo "=== no operands: signal-table order, whatever order the traps were set in"
trap 'echo U' USR1
trap 'echo I' INT
trap 'echo E' EXIT
trap -p
echo "--- and a bare trap is the same listing"
trap

echo "=== with operands: the order they were asked for"
trap -p USR1 INT EXIT
echo "---"
trap -p EXIT INT USR1

echo "=== a signal named twice is printed twice"
trap -p INT INT INT

echo "=== a word that names no signal is an error, and the rest still print"
trap -p INT BOGUS USR1; echo "  rc=$?"
echo "--- stderr only:"
trap -p INT BOGUS USR1 2>&1 >/dev/null; echo "  rc=$?"
echo "--- stdout only:"
trap -p INT BOGUS USR1 2>/dev/null; echo "  rc=$?"

echo "=== several bad ones each get their own line"
trap -p BOGUS ALSOBOGUS; echo "  rc=$?"

echo "=== an operand with no trap set contributes nothing"
trap -p QUIT; echo "  rc=$? (nothing above)"
trap -p QUIT INT QUIT

echo "=== the pseudo-signals take part too"
trap 'echo D' DEBUG
trap 'echo R' ERR
trap -p ERR DEBUG INT
trap - USR1 INT EXIT DEBUG ERR
