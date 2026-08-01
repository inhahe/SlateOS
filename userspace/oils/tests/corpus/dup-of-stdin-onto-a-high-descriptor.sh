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
# All of which holds one dup further along: a descriptor the list has just
# made is a descriptor the rest of the list can copy, so `3<&0 4<&3` gives the
# same description a third name, and a `3<&-` or a `3> file` in between leaves
# nothing readable for the `4<&3` to copy.
#
# Deliberately absent: `3<&1` / `3<&2`, and `3> out 4<&3` — a dup of a
# descriptor that is open but has no read half. bash makes the dup and lets
# the *read* through it fail (`read: read error: 0: …`, naming the fd `read`
# was pointed at); osh refuses the redirect instead, naming the source. See
# TD-OILS-TRANSIENT-DUP-SOURCE-IS-NOT-THE-LIST-SO-FAR.
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

echo "=== a descriptor the list just made is one the list can copy"
( exec 0<in; { read -r l <&4; } 3<&0 4<&3;     echo "  0-3-4 rc=$? l=[$l]" )
( { read -r l <&4; } 3<in 4<&3;                echo "  file-3-4 rc=$? l=[$l]" )
( { read -r a <&3; read -r b <&4; } 3<in 4<&3; echo "  shared a=[$a] b=[$b]" )
( { read -r l; } 3<in <&3;                     echo "  back-to-0 rc=$? l=[$l]" )
( exec 0<in; { read -r l; } 3<&0 <&3;          echo "  0-3-0 rc=$? l=[$l]" )

echo "=== but a closed one is not"
( { read -r l <&4; } 3<in 3<&- 4<&3;           echo "  in-list rc=$? l=[$l]" )
( exec 3<in; { read -r l <&4; } 3<&- 4<&3;     echo "  exec rc=$? l=[$l]" )

echo "=== and the exec-installed table still answers for itself"
( exec 3<in; read -r a <&3; read -r b <&3;     echo "  a=[$a] b=[$b]" )
( exec 3<in; { read -r l <&4; } 4<&3;          echo "  4<&3 rc=$? l=[$l]" )

exec 2>&4 4>&-
echo "=== what went to stderr"
cat err
