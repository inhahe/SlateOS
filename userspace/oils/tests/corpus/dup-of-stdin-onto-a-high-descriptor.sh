# `3<&0` gives fd 0's open file description a second name, and the two names
# then share one position: a `read <&3` and a following unredirected `read`
# come back with successive lines, not the same line twice. That is what makes
# it a *dup* rather than a second open of whatever fd 0 happened to be reading.
#
# Which fd 0 is copied is a question of where the dup sits in the redirect
# list, because a list is resolved left to right and each entry sees the fd 0
# the ones before it left:
#
#   * `3<&0 <in` copies the *ambient* fd 0 and only then rebinds fd 0, so fd 3
#     is not the file;
#   * `<in 3<&0` copies the file, so it is.
#
# A here-document is a source like any other on both counts — it can be the
# thing copied, and copying it shares its position.
#
# A closed fd 0 is not a descriptor to copy at all, and the failure is reported
# against the *source*: `0<&- 3<&0` is `0: Bad file descriptor`, naming fd 0
# rather than the fd 3 that was being made. Reverse the two and the dup came
# first, so it succeeds and the close only takes fd 0 away.
#
# Deliberately absent:
#
#   * `4<&3` where fd 3 was made by an earlier `3<&0` in the *same* list —
#     bash chains them, osh only looks fd 3 up in the `exec`-installed table.
#   * `3<&1` / `3<&2`, where bash makes the dup and lets the *read* through it
#     fail; osh refuses the redirect instead.
#
# Both are TD-OILS-TRANSIENT-DUP-SOURCE-IS-NOT-THE-LIST-SO-FAR.
#
# Every persistent probe runs in a subshell so an `exec` cannot reach the next
# one. Stderr is collected and replayed at the end so it can be compared in a
# fixed place; nothing here prints a pid, so it is replayed unfiltered.
printf 'one\ntwo\n' > in
printf 'A\nB\n' > in2
exec 4>&2 2>err

echo "=== a second name for fd 0"
( exec 0<in; { read -r l <&3; } 3<&0;      echo "  rc=$? l=[$l]" )
( exec 0<in; { read -r a <&3; read -r b; } 3<&0; echo "  shared a=[$a] b=[$b]" )
( exec 0<in; exec 3<&0; read -r a <&3; read -r b; echo "  exec a=[$a] b=[$b]" )

echo "=== the dup copies the fd 0 the redirects before it left"
( exec 0<in; { read -r l <&3; } 3<&0 <in2; echo "  dup-first rc=$? l=[$l]" )
( exec 0<in; { read -r l <&3; } <in2 3<&0; echo "  file-first rc=$? l=[$l]" )
( { read -r l <&3; } 3<&0 <in;             echo "  ambient rc=$? l=[$l]" )

echo "=== a here-document is a source like any other"
( { read -r a <&3; read -r b; } <<< $'x\ny' 3<&0; echo "  a=[$a] b=[$b]" )
( { read -r a <&3; } 3<&0 <<< $'x\ny';            echo "  before rc=$? a=[$a]" )

echo "=== a closed fd 0 is no descriptor to copy"
( exec 0<in; { read -r l <&3; } 0<&- 3<&0; echo "  close-first rc=$? l=[$l]" )
( exec 0<in; { read -r l <&3; } 3<&0 0<&-; echo "  dup-first rc=$? l=[$l]" )
( exec 0<&-; exec 3<&0 );                  echo "  exec rc=$?"

echo "=== 3<&3 needs no source at all"
( exec 0<in; { read -r l <&3; } 3<&3;      echo "  rc=$? l=[$l]" )

exec 2>&4 4>&-
echo "=== what went to stderr"
cat err
