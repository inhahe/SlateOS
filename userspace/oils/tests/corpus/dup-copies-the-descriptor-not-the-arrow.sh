# `N<&M` and `N>&M` are one operation. Both are `dup2(M, N)`, so the copy
# carries whatever access mode fd M had and the arrow the dup was written with
# picks nothing: `3<&1` gives fd 3 stdout's *write* end, and `4>&3` after a
# `3< file` gives fd 4 the file's *read* end — cursor and all.
#
# The consequence worth having is where the failure lands. A shell that reads
# the arrow as the mode has no fd 3 after `3<&1`, so it must refuse the
# redirect (`3: Bad file descriptor`) and the body never runs. bash makes the
# dup and lets the *read* through it fail:
# `read: read error: 0: Bad file descriptor`, naming fd 0 because that is what
# `read` was pointed at, with the body's other commands running as usual.
#
# So a descriptor is open, or has a read half, or has a write half, and those
# are three separate questions:
#
#   * open decides whether the redirect is made at all — `4<&3` after `3<&-`
#     is refused, `4<&3` after `3> out` is not;
#   * a read half decides whether `read <&N` finds bytes or `EBADF`;
#   * a write half decides whether `echo >&N` writes or reports
#     `echo: write error: Bad file descriptor`.
#
# `<>` is the case that has both, and a plain `3< file` the case that has one
# and is still a descriptor for a `4>&3` to copy.
#
# Deliberately absent: `1<&0` and `2<&0` — a dup *onto* a standard write
# descriptor from a read-only source, which is the same question asked of a
# target that is one of the shell's own streams. It has a case of its own,
# `a-std-fd-bound-to-a-read-only-source.sh`, because a std fd is where the
# three questions come apart most visibly: fd 2 with no write half is a fd 2
# with nowhere to report the failure. Absent too: `>oa 3>&1 >ob`, where a
# *second* stdout redirect follows the dup — see
# TD-OILS-DUP-OF-STDOUT-IS-NOT-THE-LIST-SO-FAR.
#
# Every persistent probe runs in a subshell so an `exec` cannot reach the next
# one. Stderr is collected and replayed at the end so it can be compared in a
# fixed place; nothing here prints a pid, so it is replayed unfiltered.
printf 'one\ntwo\n' > in
exec 4>&2 2>err

echo "=== a dup of a write descriptor is made, and the read through it fails"
( { read -r l <&3; } 3<&1;            echo "  3<&1 rc=$? l=[$l]" )
( { read -r l <&3; } 3<&2;            echo "  3<&2 rc=$? l=[$l]" )
( { read -r l <&3; } 3>out;           echo "  3>out rc=$? l=[$l]" )
( { read -r l <&4; } 3>out 4<&3;      echo "  chain rc=$? l=[$l]" )
( exec 3<&1; read -r l <&3;           echo "  exec rc=$? l=[$l]" )
( read -r l <&1;                      echo "  <&1 rc=$? l=[$l]" )
( read -r l <&2;                      echo "  <&2 rc=$? l=[$l]" )

echo "=== the message names the descriptor read was pointed at"
( read -u 3 -r l ) 3<&1;              echo "  -u3 rc=$?"
( exec 3<&1; read -u 3 -r l;          echo "  exec -u3 rc=$?" )
( mapfile -t a <&3 ) 3<&1;            echo "  mapfile rc=$? n=${#a[@]}"

echo "=== but the descriptor is there, and it is a write descriptor"
( { echo W >&3; } 3<&1;               echo "  3<&1 rc=$?" )
( exec 3<&1; echo W >&3;              echo "  exec rc=$?" )
( { echo W >&4; } 3<&1 4<&3;          echo "  4<&3 rc=$?" )

echo "=== and the same holds the other way round"
( { read -r l <&4; } 3<in 4>&3;       echo "  4>&3 rc=$? l=[$l]" )
( { echo W >&4; } 3<in 4>&3;          echo "  write rc=$?" )
( { read -r a <&3; read -r b <&4; } 3<in 4>&3; echo "  shared a=[$a] b=[$b]" )
( exec 3<in; exec 4>&3; read -r l <&4; echo "  exec rc=$? l=[$l]" )
( exec 3<in; exec 4>&3; echo W >&4;    echo "  exec write rc=$?" )

echo "=== a plain 3< file is still a descriptor with no write half"
( { echo W >&3; } 3<in;               echo "  rc=$?" )
( exec 3<in; echo W >&3;              echo "  exec rc=$?" )

echo "=== fd 0 is one of the descriptors a dup can copy either way"
( exec 0<in; { read -r l <&3; } 3>&0; echo "  3>&0 rc=$? l=[$l]" )
( exec 0<in; { read -r a <&3; read -r b; } 3>&0; echo "  shared a=[$a] b=[$b]" )
( exec 0<in; exec 3>&0; read -r l <&3; echo "  exec rc=$? l=[$l]" )
( exec 0<in; exec 3>&0; echo W >&3;    echo "  exec write rc=$?" )

echo "=== a chain that crosses direction twice"
( exec 3<in; exec 4>&3; exec 5<&4; read -r l <&5; echo "  3-4-5 rc=$? l=[$l]" )
( exec 3<&1; exec 4>&3; echo W >&4;   echo "  1-3-4 rc=$?" )

echo "=== a <> descriptor has both halves, and a dup of it keeps both"
( { echo W >&3; read -r l <&3; } 3<>rw;  echo "  rw rc=$? l=[$l]" )
( { read -r l <&4; } 3<>rw 4<&3;         echo "  dup rc=$? l=[$l]" )
( { echo X >&4; } 3<>rw2 4<&3;           echo "  dup write rc=$?"; cat rw2 )

echo "=== a closed descriptor is still no descriptor at all"
( { read -r l <&4; } 3<&1 3<&- 4<&3;  echo "  in-list rc=$? l=[$l]" )
( exec 3<&1; exec 3<&-; exec 4<&3 );  echo "  exec rc=$?"

exec 2>&4 4>&-
echo "=== what went to stderr"
cat err
