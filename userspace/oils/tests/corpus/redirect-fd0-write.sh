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
