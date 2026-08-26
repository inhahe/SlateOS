# A pipeline stage's standard input is the read end of the upstream stage's
# pipe, and bash gives it no special status: it is fd 0 like any other, so
# every form that names fd 0 names *it*, and `read` leaves it positioned
# exactly one byte past the delimiter it consumed.
#
# That last part is the constraint the rest hangs off. `read` has to buffer —
# it is looking for a delimiter — but a pipe cannot seek, so a buffer it does
# not give back is bytes the next reader will never see. bash's answer
# (`zsyncfd`) is to read unseekable input one byte at a time; the visible
# consequence is that `printf 'A\nB\n' | { read -r l; cat; }` prints `B` and
# not nothing.
#
# Every probe keeps its upstream a `printf`, so the producer finishes before
# the stage runs and nothing here depends on scheduling.

echo "=== a read leaves the rest of the pipe for the next command in the stage"
printf 'A\nB\n' | { read -r l; echo "  l=[$l]"; cat | sed 's/^/  rest: /'; }
printf 'A\nB\nC\n' | { read -r l; head -1 | sed 's/^/  head: /'; }
echo "--- including a second read, which is the same descriptor again"
printf 'A\nB\n' | { read -r l; read -r m; echo "  [$l][$m]"; }
echo "--- and a child named through fd 0 explicitly"
printf 'A\nB\n' | { read -r l; cat <&0 | sed 's/^/  via0: /'; }

echo "=== <&0 duplicates it, transiently"
printf 'A\nB\n' | { read -r l <&0; read -r m; echo "  [$l][$m]"; }
printf 'A\nB\n' | { read -u 0 l; read -u 0 m; echo "  [$l][$m]"; }
echo "--- and onto a descriptor of the stage's own"
printf 'A\nB\n' | { { read -r l <&3; read -r m; echo "  [$l][$m]"; } 3<&0; }

echo "=== exec <&0 duplicates it for the rest of the stage"
printf 'A\nB\n' | { exec 3<&0; read -u 3 l; read -u 3 m; echo "  [$l][$m]"; exec 3<&-; }
echo "--- one description, so the two names share a position"
printf 'A\nB\nC\n' | { exec 3<&0; read -r l; read -u 3 m; read -r n; exec 3<&-
  echo "  [$l][$m][$n]"; }
echo "--- and a child through the duplicate reads on from there"
printf 'A\nB\n' | { exec 3<&0; read -u 3 l; cat <&3 | sed 's/^/  rest: /'; exec 3<&-; }
echo "--- the duplicate outlives a rebinding of fd 0"
printf 'A\nB\n' | { exec 3<&0; exec 0</dev/null; read -u 3 l; echo "  l=[$l]"; exec 3<&-; }
echo "--- a named descriptor is the same dup"
printf 'A\nB\n' | { exec {v}<&0; read -u "$v" l; read -u "$v" m; echo "  [$l][$m]"; }

echo "=== /dev/stdin re-opens it, and a pipe re-opens as the same pipe"
printf 'A\nB\n' | { read -r l; read -r m </dev/stdin; echo "  [$l][$m]"; }
printf 'echo A\necho B\n' | { read -r l; source /dev/stdin | sed 's/^/  sourced: /'; }
