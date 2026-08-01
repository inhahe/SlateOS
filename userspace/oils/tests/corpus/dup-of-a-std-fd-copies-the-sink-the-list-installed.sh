# A redirect list is resolved left to right and each entry sees the descriptors
# the ones before it left, so which fd 1 a `3>&1` copies is a question of where
# it sits: `>out 3>&1` copies `out`, and `3>&1 >out` copies the stdout the
# command started with. The same for fd 2 and `3>&2`, and — since a dup is one
# `dup2` and the arrow picks nothing — the same for `3<&1`.
#
# What it copies is the *descriptor*, not the file: two names of one open file
# share one offset, so `{ echo A >&3; echo B; } >f 3>&1` appends rather than
# clobbers. That is the difference between duplicating the handle and opening
# the path a second time, and the only thing here that a re-open would get
# wrong.
#
# Deliberately absent:
#
#   * `>oa 3>&1 >ob`, where a *second* stdout redirect follows the dup. osh's
#     redirect plan keeps one slot per standard fd, so by the time the dup is
#     applied the slot holds `ob`, and fd 3 lands there rather than on `oa`.
#     See TD-OILS-DUP-OF-STDOUT-IS-NOT-THE-LIST-SO-FAR.
#   * every shape whose command is *external*, which never sees fd 3 at all
#     under osh — not even `sh -c 'echo W >&3' 3> out`. See
#     TD-OILS-EXTERNAL-CHILD-HAS-NO-FD-3.
#
# Every persistent probe runs in a subshell so an `exec` cannot reach the next
# one. Stderr is collected and replayed at the end so it can be compared in a
# fixed place; nothing here prints a pid, so it is replayed unfiltered.
exec 4>&2 2>err

echo "=== the fd 1 a dup copies is the one the list before it left"
( { echo W >&3; } >o1 3>&1;        echo "  after  o1=[$(cat o1)]" )
( { echo W >&3; } 3>&1 >o2;        echo "  before o2=[$(cat o2)]" )
( { echo W >&3; } >o3 3<&1;        echo "  arrow  o3=[$(cat o3)]" )
( { echo W >&3; } >>o4 3>&1;       echo "  append o4=[$(cat o4)]" )

echo "=== and the same for fd 2"
( { echo W >&3; } 2>e1 3>&2;       echo "  after  e1=[$(cat e1)]" )
( { echo W >&3; } 3>&2 2>e2;       echo "  before e2=[$(cat e2)]" )

echo "=== a dup of a dup, and a dup of an ordinary scratch fd"
( { echo W >&5; } >o5 3>&1 5>&3;   echo "  chain  o5=[$(cat o5)]" )
( { echo W >&4; } 3>o6 4>&3;       echo "  scratch o6=[$(cat o6)]" )

echo "=== two names, one open file, one position"
( { echo A >&3; echo B; } >o7 3>&1; echo "  o7=[$(tr '\n' ',' < o7)]" )
( { echo A; echo B >&3; } >o8 3>&1; echo "  o8=[$(tr '\n' ',' < o8)]" )

echo "=== every body that carries its own redirect list answers alike"
( f() { echo W >&3; }; f >o9 3>&1;          echo "  func    o9=[$(cat o9)]" )
( for i in 1; do echo W >&3; done >oa 3>&1; echo "  loop    oa=[$(cat oa)]" )
( ( echo W >&3 ) >ob 3>&1;                  echo "  subshell ob=[$(cat ob)]" )
( if :; then echo W >&3; fi >oc 3>&1;       echo "  if      oc=[$(cat oc)]" )
( while :; do echo W >&3; break; done >od 3>&1; echo "  while   od=[$(cat od)]" )

echo "=== exec installs the same descriptor"
( exec >oe 3>&1; echo W >&3 );     echo "  one-statement oe=[$(cat oe)]"
( exec >of; exec 3>&1; echo W >&3 ); echo "  two-statement of=[$(cat of)]"
( exec 2>eg 3>&2; echo W >&3 );    echo "  stderr eg=[$(cat eg)]"
( exec 3>oh; exec 4>&3; echo W >&4 ); echo "  scratch oh=[$(cat oh)]"

echo "=== a capture is a sink like any other"
r=$( { echo W >&3; } 3>&1 ); echo "  capture r=[$r]"

exec 2>&4 4>&-
echo "=== what went to stderr"
cat err
