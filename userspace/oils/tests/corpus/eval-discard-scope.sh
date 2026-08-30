# Two different aborts wear the same name. A word-expansion error ends the
# string an `eval` was given and nothing more, but a special-builtin *usage*
# error unwinds past `eval` entirely and takes the rest of the caller's parse
# unit with it — in bash because `no_args()` clears the handler `eval` installed
# before it jumps. `break`/`continue` with two operands is the one such error
# reachable here.
#
# What makes the difference visible is that the rest of the caller's unit is not
# the rest of the caller's *line*: the `echo` below sits on the line after the
# `eval`, and is still discarded, because the newline between them is inside the
# eval'd string.
#
# The `$( )` shape is deliberately absent: osh handles it correctly, but bash
# blames a line number past the end of the file for it, so the case could only
# live here under a waiver. See TD-OILS-CMDSUB-ABORT-LINENO in known-issues.md.

echo "=== an expansion error ends only the string it happened in"
eval "echo a; echo \$((1/0)); echo b
echo c"; echo "  done=$?"

echo "=== a usage error unwinds past the eval, and past the rest of the unit"
eval "for i in 1; do break 1 2; done
echo next"; echo "  NOT REACHED done=$?"
echo "  next unit runs, and sees $?"

echo "=== the same through a sourced file"
cat > inner.sh <<'INNER'
for i in 1; do continue 1 2; done
echo inner-next
INNER
. ./inner.sh; echo "  NOT REACHED done=$?"
echo "  next unit runs"

echo "=== a subshell keeps it to itself"
# The unwind stops at the subshell boundary: the child fails, the parent is
# untouched and carries straight on along the same line.
( for i in 1; do break 1 2; done; echo sub-next ); echo "  sub=$? and the line continues"

echo "=== without an eval in the way it is the plain top-level discard"
for i in 1; do break 1 2; done; echo "  NOT REACHED done=$?"
echo "  next unit runs"
