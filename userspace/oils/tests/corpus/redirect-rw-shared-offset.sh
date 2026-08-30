# `<>` opens one descriptor for *both* directions, so reads and writes through
# it share a single position.
#
# That is the whole difference between `<>` and a `<` plus a `>` on the same
# path: two opens give two file descriptions and two offsets, one open gives
# one. So `read -u 3` leaves the position just past the record it consumed and
# the next `echo >&3` overwrites from exactly there — which is how `<>` is used
# to patch a file in place without rewriting it.
#
# The position is shared with everything that names the same description: a dup
# (`exec 4<&3`, `exec 4>&3`), a subshell, and an external child. It is also
# exact after a *partial* read (`read -N 3`), which is the interesting case,
# because a record reader has to look one byte past its delimiter to know it is
# done — that byte must not be left consumed.
#
# `<>` also creates the file if it is absent and never truncates it, so a fresh
# one reads as immediate EOF.

echo "=== a write, then a read, continues from the write"
printf 'abcdefghij\n' > f1
exec 3<> f1; echo AB >&3; read -u 3 x; echo "  x=[$x]"; exec 3<&-
echo "  f1=[$(cat f1)]"

echo "=== ... and a read, then a write, overwrites from the read"
printf 'l1\nl2\nl3\n' > f2
exec 3<> f2; read -u 3 a; echo "  a=[$a]"; echo XX >&3; exec 3<&-
echo "  f2=[$(tr '\n' / < f2)]"

echo "=== ... and they keep alternating"
printf 'l1\nl2\nl3\n' > f3
exec 3<> f3; read -u 3 a; read -u 3 b; echo "  a=[$a] b=[$b]"; echo Y >&3; exec 3<&-
echo "  f3=[$(tr '\n' / < f3)]"

printf 'abcdefghijkl\n' > f4
exec 3<> f4; echo P >&3; echo Q >&3; read -u 3 z; echo "  z=[$z]"; exec 3<&-
echo "  f4=[$(cat f4)]"

echo "=== a dup names the same description"
printf 'l1\nl2\nl3\nl4\n' > g1
exec 3<> g1; exec 4<&3
read -u 3 a; read -u 4 b; echo "  a=[$a] b=[$b]"
echo ZZ >&3
exec 3<&- 4<&-
echo "  g1=[$(tr '\n' / < g1)]"

# ... including a *write* dup of a read descriptor.
printf 'l1\nl2\nl3\n' > g2
exec 3<> g2; exec 4>&3
read -u 3 a; echo QQ >&4; exec 3<&- 4>&-
echo "  a=[$a] g2=[$(tr '\n' / < g2)]"

echo "=== an external child advances it"
printf 'l1\nl2\nl3\n' > g3
exec 3<> g3; head -n 1 <&3 > /dev/null; echo WW >&3; exec 3<&-
echo "  g3=[$(tr '\n' / < g3)]"

echo "=== ... and so does a subshell"
printf 'l1\nl2\nl3\n' > g4
exec 3<> g4; ( read -u 3 v ); echo VV >&3; exec 3<&-
echo "  g4=[$(tr '\n' / < g4)]"

echo "=== a partial read leaves a partial position"
printf 'abcdefgh\n' > g5
exec 3<> g5; read -N 3 -u 3 p; echo "  p=[$p]"; printf 'XY' >&3; exec 3<&-
echo "  g5=[$(cat g5)]"

echo "=== reading the rest starts where the reads stopped"
printf 'l1\nl2\nl3\n' > g6
exec 3<> g6; read -u 3 a; cat <&3; exec 3<&-

echo "=== fd 0 works the same way"
printf 'm1\nm2\n' > g7
{ read a; read b; echo "  a=[$a] b=[$b]"; } <> g7
echo "  g7=[$(tr '\n' / < g7)]"

echo "=== it creates the file, and a fresh one is at EOF"
rm -f g8; exec 3<> g8; read -u 3 z; echo "  st=$? z=[$z]"; echo new >&3; exec 3<&-
echo "  g8=[$(cat g8)]"

echo "=== ... and never truncates an existing one"
printf 'keep me\n' > g9
exec 3<> g9; exec 3<&-
echo "  g9=[$(cat g9)]"
