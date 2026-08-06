# fd 0 is a descriptor like any other, so `>&0` is a *write* to it.
#
# Whether that write lands depends on how fd 0 was opened, not on the fact that
# it is fd 0. A plain `< file` (or a here-document, or a pipe) opens it for
# reading only, and POSIX answers a write to such a descriptor with `EBADF` —
# which the shell reports against the *builtin* that produced the bytes, as
# `echo: write error: Bad file descriptor`, status 1. An `<>` open is
# read+write, so the same `>&0` succeeds and, because it is one open file
# description, it writes at exactly the position the last read left.
#
# The failure is a property of the description, so it survives a dup: after
# `exec 3>&0` the *dup* succeeds — fd 0 is open — and it is the later write
# through fd 3 that fails.
#
# fd 2 is the one place where the failure is invisible: `2>&0` on a read-only
# fd 0 means a diagnostic has nowhere to go, so it is dropped and only the exit
# status survives.
#
# An *external* child is handed the read-only description itself, not an empty
# stream that would take the bytes: the dup succeeds — bash's does too, which is
# why this is not a redirect-time error — and it is the child's own write that
# fails, with its own `EBADF`. That is the whole difference at fd 2, where the
# diagnostic is lost either way and only the status can tell the two apart.

echo "=== a write to a read-only fd 0 fails, and names the builtin"
printf 'l1\nl2\n' > f
{ echo X >&0; } < f; echo "  st=$?"
{ printf 'P\n' >&0; } < f; echo "  st=$?"
echo "  f=[$(tr '\n' / < f)]"

echo "=== ... and so does one to a here-document or a pipe"
{ echo X >&0; } <<'H'
h1
H
echo "  st=$?"
echo p1 | { echo X >&0; }; echo "  st=$?"

echo "=== an <> open is writable, and shares the read position"
printf 'abcdefghij\n' > g1
{ echo A >&0; } <> g1; echo "  st=$? g1=[$(cat g1)]"

printf 'l1\nl2\nl3\n' > g2
{ read a; echo XX >&0; } <> g2
echo "  a=[$a] g2=[$(tr '\n' / < g2)]"

printf '0123456789\n' > g3
{ echo A >&0; echo B >&0; } <> g3
echo "  g3=[$(tr '\n' / < g3)]"

echo "=== ... including a persistent exec 0<>"
printf 'abcdefghij\n' > g4
exec 0<> g4; echo YY >&0; exec 0<&-
echo "  g4=[$(cat g4)]"

echo "=== a dup of fd 0 inherits its access mode"
{ exec 3>&0; echo Z >&3; } < f; echo "  st=$?"

printf 'abcdefghij\n' > g5
{ exec 3>&0; echo Q >&3; } <> g5; echo "  st=$? g5=[$(cat g5)]"

echo "=== a diagnostic sent to a read-only fd 0 is simply lost"
{ cd /nosuchdir 2>&0; } < f; echo "  st=$?"
{ unset -f 2>&0; } < f; echo "  st=$?"

echo "=== reopening fd 0 read-only drops the write half again"
printf 'abcdefghij\n' > g6
exec 0<> g6; exec 0< f; echo W >&0; echo "  st=$?"
exec 0<&-
echo "  g6=[$(cat g6)]"

# Everything from here on runs a child. Each probe is a subshell so an `exec`
# inside it cannot reach the next one, and stderr is collected and replayed at
# the end so it can be compared in a fixed place. The child's own diagnostic
# names the command as it was started — on this host the resolved path rather
# than the word (TD-OILS-HOST-ARGV0, a `std::process` limitation, not a target
# one) — so the leading directories are filtered off at the replay.
exec 4>&2 2>err

echo "=== an external child is handed the read-only fd 2, not an empty stream"
# Both lose the diagnostic; only the first makes the child's own write fail,
# and at fd 2 the status is the only place that can show it.
( exec 3< f; sh -c 'echo E >&2' 2>&3 );         echo "  cmd rc=$?"
( exec 3< f; exec 2>&3; sh -c 'echo E >&2' );   echo "  exec rc=$?"
( exec 3< f; { sh -c 'echo E >&2'; } 2>&3 );    echo "  group rc=$?"
( exec < f; sh -c 'echo E >&2' 2>&0 );          echo "  2>&0 rc=$?"
( exec < f; exec 2>&0; sh -c 'echo E >&2' );    echo "  exec 2>&0 rc=$?"
# A child that has its own status keeps it, and one that never writes to fd 2
# is untroubled by the whole business.
( exec 3< f; sh -c 'echo E >&2; exit 7' 2>&3 ); echo "  own status rc=$?"
( exec 3< f; sh -c 'exit 0' 2>&3 );             echo "  quiet rc=$?"

echo "=== fd 1 the same way, where the child can say so"
( exec 3< f; sh -c 'echo O' >&3 );              echo "  cmd rc=$?"
( exec < f; sh -c 'echo O' >&0 );               echo "  >&0 rc=$?"
( exec < f; exec 1>&0; sh -c 'echo O' );        echo "  exec rc=$?"

echo "=== and a dup of a read-only fd 2 into a child's fd 1"
( exec 3< f; exec 2>&3; sh -c 'echo O' >&2 );   echo "  exec rc=$?"
( exec 3< f; { sh -c 'echo O' 1>&2; } 2>&3 );   echo "  group rc=$?"

exec 2>&4 4>&-
echo "=== none of it wrote to the file"
echo "  f=[$(tr '\n' / < f)]"
echo "=== what went to stderr"
sed 's#^[^:]*/##' err
