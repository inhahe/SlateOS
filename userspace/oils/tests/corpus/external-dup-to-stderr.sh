# `cmd >&2` on an *external* command: fd 1 becomes a dup of fd 2, so the child's
# output has to reach whatever fd 2 names at that moment — the shell's stderr, a
# file an enclosing `exec`/compound redirect put there, or a command
# substitution's buffer, which has no descriptor to hand a child at all.
#
# Every line here uses `cat`, which is not a builtin, because the builtin and
# the spawn paths resolve a dup target separately.
printf 'x\n' > f
printf 'y\n' > g

echo "=== bare, to the shell's stderr"
cat f >&2
echo "  rc=$?"

echo "=== the order of >&2 and 2> decides which one moves"
cat f >&2 2>/dev/null
cat g 2>/dev/null >&2

echo "=== an enclosing compound redirect is what fd 2 names"
{ cat f >&2; } 2> cap
echo "  cap=[$(cat cap)]"

echo "=== and so is exec's"
( exec 2> cap2; cat g >&2 )
echo "  cap2=[$(cat cap2)]"

echo "=== into a command substitution's buffer"
v=$( { cat f >&2; } 2>&1 )
echo "  v=[$v]"

echo "=== a pipeline's stderr is not in the pipe"
cat f >&2 | cat
echo "  after=$?"

echo "=== and 2>&1 puts it back in"
cat f >&2 2>&1 | cat
