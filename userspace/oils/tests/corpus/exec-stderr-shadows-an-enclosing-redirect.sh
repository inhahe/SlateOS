# A body's `2>…` covers the whole body, and an `exec 2>…` run *inside* that
# body covers the rest of the shell. Both are true at once, so the only
# question is which of them came last — and for the part of the body after the
# `exec`, the answer is the `exec`.
#
# It is easy to answer the other way by accident, because a body's redirect is
# a *stack* the shell pushes and a persistent one is a *field* it assigns: read
# the stack first and the field is never reached, so the `exec` appears to do
# nothing until the body ends. That is exactly backwards — `{ exec 2> f; cd
# /nope; } 2>&1` must put the diagnostic in `f`, not in the `2>&1` sink.
#
# Reverse the two and the group is the later word, so it wins instead: `exec
# 2> f; { cd /nope; } 2>&1` writes to the sink and leaves `f` empty. Which is
# what makes this a question of order rather than a standing precedence.
#
# The enclosing redirect is only *shadowed*, not discarded. Tearing the body's
# redirect down at the end restores the fd 2 the `exec` replaced, so the next
# command writes where it would have written had the `exec` never run — and a
# sourced script's `exec 2>…` likewise stops at the `.` command's own redirect.
#
# None of that is about *diagnostics*: it is about which description fd 2 names,
# so it must hold for everything fd 2 reaches. An external child is handed fd 2
# rather than writing through it, and a `2>&1` inside a `$( … )` is *held* by
# fd 2 for as long as a background job has it — both must read the same order,
# or the `exec` appears to work for the shell's own messages and to do nothing
# for anyone else's.
#
# Every probe runs in a subshell (or a capture) so a persistent redirect cannot
# reach the next one. Stderr is collected and replayed at the end so it can be
# compared in a fixed place; nothing here prints a pid, so it is replayed
# unfiltered.
exec 4>&2 2>err

echo "=== the exec is the later word, so it wins"
v=$( { exec 2>f1; cd /nosuchdir; } 2>&1 ); echo "  rc=$? v=[$v]"
echo "  f1=[$(cat f1)]"
v=$( { exec 2>&-; cd /nosuchdir; } 2>&1 ); echo "  closed rc=$? v=[$v]"

echo "=== written the other way round, the group is"
v=$( exec 2>f2; { cd /nosuchdir; } 2>&1 ); echo "  rc=$? v=[$v]"
echo "  f2=[$(cat f2)]"

echo "=== and the shadowed redirect comes back when its body ends"
v=$( { { exec 2>f3; cd /nosuchdir; } 2>&1; cd /alsonosuchdir; } 2>&1 )
echo "  v=[$v]"
echo "  f3=[$(cat f3)]"
( { exec 2>f4; cd /nosuchdir; } 2>f5; cd /nodir2 ) 2>f6
echo "  f4=[$(cat f4)] f5=[$(cat f5)] f6=[$(cat f6)]"

echo "=== a sourced exec is bounded by the source command's own redirect"
printf 'exec 2>g1\ncd /nosuchdir\n' > s1.sh
( . ./s1.sh 2>e1; cd /nodir4 ) 2>e2
echo "  g1=[$(cat g1)]"
echo "  e1=[$(cat e1)]"
echo "  e2=[$(cat e2)]"

echo "=== an external child's fd 2 reads the order the same way"
( { exec 2>h1; sh -c 'echo E >&2'; } 2>h2 );  echo "  rc=$?"
echo "  h1=[$(cat h1)] h2=[$(cat h2)]"
# …and so does a child's `1>&2`, which is a dup of whichever fd 2 is in force.
( { exec 2>h3; sh -c 'echo O' >&2; } 2>h4 );  echo "  dup rc=$?"
echo "  h3=[$(cat h3)] h4=[$(cat h4)]"
# An `exec 2>&-` shadows an enclosing redirect too, so the child is handed the
# absence rather than the sink — and its own write to a missing fd 2 fails.
( { exec 2>&-; sh -c 'echo E >&2'; } 2>h5 );  echo "  closed rc=$?"
echo "  h5=[$(cat h5)]"
# Written the other way round the group is the later word, as above.
( exec 2>h6; { sh -c 'echo E >&2'; } 2>h7 );  echo "  reversed rc=$?"
echo "  h6=[$(cat h6)] h7=[$(cat h7)]"

echo "=== and a shadowed 2>&1 is no longer held by fd 2"
# A `$( … )` ends when nothing still holds its capture. `{ …; } 2>&1` puts the
# capture on fd 2, so a background job inheriting fd 2 holds it even after
# redirecting fd 1 away — but an `exec 2>…` took fd 2 off the capture first,
# and then the substitution returns at once. The job is given a lifetime
# comfortably longer than a process spawn so which of the two happened is not
# in doubt (the same reasoning as jobs-listing.sh).
( v=$( { { sleep 0.6; echo late > n1; } >/dev/null & } 2>&1 )
  echo "  held=[$(cat n1 2>/dev/null)]"; wait )
( v=$( { exec 2>/dev/null; { sleep 0.6; echo late > n2; } >/dev/null & } 2>&1 )
  echo "  shadowed=[$(cat n2 2>/dev/null)]"; wait )

exec 2>&4 4>&-
echo "=== what went to stderr"
cat err
