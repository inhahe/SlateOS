# `2>&1` is a *dup* of fd 1 taken at the instant the redirect is applied — not
# a standing instruction for fd 2 to track fd 1 from then on. Measured against
# bash 5.2.
#
# The distinction is invisible while fd 1 never moves again, so every probe
# here moves it afterwards, in one of the three ways it can move:
#
#   * a `>file` later in the same redirect list (`2>&1 >f`), which redirects
#     apply left to right;
#   * a runtime `exec > file` inside the redirect's own scope;
#   * an inner compound command's `>file`.
#
# In all three the earlier `2>&1` keeps fd 2 on the *original* stdout. The
# probes also run the writer inside a pipeline stage, a command substitution
# and an external child, because those each get their own fd table in bash and
# must still see the enclosing dup.
#
# Nothing is merged here: the harness compares stdout and stderr separately, so
# a line that escapes to the real stderr — or one that ends up in a file — is
# visible as a difference rather than being hidden by a shared terminal.
child() { "$BASH" -c 'echo X >&2' </dev/null; }

echo "=== the dup does not follow a later >file"
{ echo A; echo X >&2; } 2>&1 >h.txt; echo "  h=[$(cat h.txt)]"
( echo X >&2 ) 2>&1 >h.txt; echo "  h=[$(cat h.txt)]"
echo "--- whereas >file first sends both onward"
{ echo A; echo X >&2; } >h.txt 2>&1; echo "  h=[$(cat h.txt)]"

echo "=== the dup does not follow a runtime exec > file"
( exec >g.txt; echo X >&2 ) 2>&1; echo "  g=[$(cat g.txt)]"
{ exec >g.txt; echo X >&2; } 2>&1; echo "  g=[$(cat g.txt)]"
( exec >g.txt; { echo X; } >&2 ) 2>&1; echo "  g=[$(cat g.txt)]"
( exec >g.txt; child ) 2>&1; echo "  g=[$(cat g.txt)]"
( exec >g.txt; { echo A; echo X >&2; echo B; } ) 2>&1; echo "  g=[$(cat g.txt)]"

echo "=== nor an inner compound command's >file"
{ { echo X >&2; } >h.txt; } 2>&1; echo "  h=[$(cat h.txt)]"

echo "=== and it reaches every kind of inner fd table"
{ { echo X >&2; } | sed 's/^/piped: /'; } 2>&1
{ child | sed 's/^/piped: /'; } 2>&1
{ echo "[$( { echo X >&2; } )]"; } 2>&1
{ echo "[$(child)]"; } 2>&1
f() { echo X >&2 | sed 's/^/piped: /'; }; f 2>&1

echo "=== including when the enclosing fd 2 is itself a pipe or a capture"
{ { echo X >&2; } | sed 's/^/piped: /'; } 2>&1 | cat
x=$( { { echo X >&2; } | sed 's/^/piped: /'; } 2>&1 ); echo "  x=[$x]"
{ { echo X >&2; } | sed 's/^/piped: /'; } 2>h.txt | cat; echo "  h=[$(cat h.txt)]"

echo "=== a builtin's own redirect covers what the builtin runs"
eval '{ echo X >&2; } | sed s/^/piped:/' 2>&1
eval '{ echo X >&2; } | sed s/^/piped:/' 2>h.txt; echo "  h=[$(cat h.txt)]"
eval 'x=$( { echo X >&2; } ); echo "  x=[$x]"' 2>&1
eval 'exec >g.txt; echo X >&2' 2>&1; echo "  g=[$(cat g.txt)]"

echo "=== an inner 2> of its own still wins for its own command"
{ { echo X >&2; } 2>h.txt; } 2>&1; echo "  h=[$(cat h.txt)]"

echo "=== ordering is preserved on the shared sink"
{ echo a; echo X >&2; echo b; } 2>&1

echo "--- including when that sink is a command substitution's capture"
# fd 2 becomes a second writer on fd 1's *own* buffer, so the two interleave in
# write order. osh used to collect fd 2 separately and append it afterwards,
# which put a leading stderr line last.
x=$( { echo X >&2; echo a; } 2>&1 ); echo "  x=[$x]"
x=$( { echo a; echo X >&2; echo b; } 2>&1 ); echo "  x=[$x]"
x=$(f() { echo X >&2; echo a; }; f 2>&1); echo "  x=[$x]"
x=$(eval 'echo X >&2; echo a' 2>&1); echo "  x=[$x]"
x=$( { echo a; child; echo b; } 2>&1 ); echo "  x=[$x]"
echo "--- and when a redirect failure is the thing being merged"
x=$( { echo a; } 2>&1 > /nonexistent-dir-xyz/f ); echo "  rc=$? x=[${x##*: }]"

rm -f g.txt h.txt
