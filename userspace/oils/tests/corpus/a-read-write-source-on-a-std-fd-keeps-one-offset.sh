# `1<> file` is the one redirect that leaves a standard *write* descriptor with
# both halves. It is a single `open(O_RDWR|O_CREAT)`: the file is created if it
# is absent and never truncated, so `{ :; } 1<>f` leaves `f` exactly as it was —
# and the read half and the write half are two names for that one open file
# description, so they share one cursor. `{ echo W; } 1<>f` therefore writes at
# offset 0, overwriting in place, which is the whole difference from `1>>f`; and
# `{ read -r a <&1; echo XX; } 1<>f` writes where the read stopped.
#
# A dup carries the description across, both halves and the cursor with it, so
# `3>&1` after a `1<>f` writes into `f` at the shared position and `2>&1` after
# one does too. The same for fd 2 and `2<>f`.
#
# Which fd 1 the dup copies is, as ever, a question of where it sits in the
# list, and a later redirect on fd 1 still wins: `1<>f >g` writes to `g`.
#
# An external child is handed the descriptor like any other, so `sh -c 'echo W'
# 1<>f` overwrites `f`'s first line rather than appending.
#
# Deliberately absent: every shape whose `1<>` sits inside `$( … )`. bash opens
# the file there — an unopenable path still reports at redirection time — and
# then does not install it on fd 1, so `r=$( { echo W; } 1<>f )` captures `W`
# and leaves `f` untouched, while the same body in backticks or in a pipeline
# stage writes to `f`. That inconsistency is a bash bug and osh deliberately
# does not copy it; see TD-OILS-CMDSUB-RW-ON-STDOUT-DROPPED.
#
# Every persistent probe runs in a subshell so an `exec` cannot reach the next
# one. Stderr is collected and replayed at the end so it can be compared in a
# fixed place; nothing here prints a pid, so it is replayed unfiltered.
mk() { printf 'one\ntwo\nthree\n' > "$1"; }
exec 4>&2 2>err

echo "=== the write lands at offset 0, not at the end"
mk f1; ( { echo W; } 1<>f1 );                echo "  group  f1=[$(tr '\n' ',' < f1)]"
mk f2; ( echo W 1<>f2 );                     echo "  simple f2=[$(tr '\n' ',' < f2)]"
mk f3; ( printf 'W\n' 1<>f3 );               echo "  printf f3=[$(tr '\n' ',' < f3)]"
mk f4; ( { echo AA; echo BB; } 1<>f4 );      echo "  pieces f4=[$(tr '\n' ',' < f4)]"

echo "=== and the same for fd 2"
mk g1; ( { echo W >&2; } 2<>g1 );            echo "  group  g1=[$(tr '\n' ',' < g1)]"
mk g2; ( cd /nosuchdir_rw_zz 2<>g2 );        echo "  diag   rc=$? g2=[$(tr '\n' ',' < g2)]"

echo "=== the file is created if absent, and never truncated"
rm -f n1; ( { echo W; } 1<>n1 );             echo "  create n1=[$(tr '\n' ',' < n1)]"
mk t1; ( { :; } 1<>t1 );                     echo "  intact t1=[$(tr '\n' ',' < t1)]"

echo "=== the read half and the write half are one cursor"
mk r1; ( { read -r l <&1; } 1<>r1;           echo "  read   l=[$l]" )
mk r2; ( { echo W; read -r l <&1; } 1<>r2;   echo "  after  l=[$l]" );  echo "  r2=[$(tr '\n' ',' < r2)]"
mk r3; ( { read -r l <&1; echo W; } 1<>r3;   echo "  before l=[$l]" );  echo "  r3=[$(tr '\n' ',' < r3)]"
mk r4; ( { read -r a <&1; echo XX; read -r b <&1; } 1<>r4; echo "  both   a=[$a] b=[$b]" ); echo "  r4=[$(tr '\n' ',' < r4)]"
mk r5; ( { echo W >&2; read -r l <&2; } 2<>r5; echo "  fd2    l=[$l]" )

echo "=== a dup of it keeps both halves and the one cursor"
mk d1; ( { echo W >&3; read -r l <&3; } 1<>d1 3>&1; echo "  3>&1   l=[$l]" ); echo "  d1=[$(tr '\n' ',' < d1)]"
mk d2; ( { echo A; echo B >&3; } 1<>d2 3>&1 );      echo "  shared d2=[$(tr '\n' ',' < d2)]"
mk d3; ( { echo O; echo E >&2; } 1<>d3 2>&1 );      echo "  2>&1   d3=[$(tr '\n' ',' < d3)]"
mk d4; ( echo E >&2 1<>d4 2>&1 );                   echo "  simple d4=[$(tr '\n' ',' < d4)]"
mk d5; ( { echo O >&2; } 2<>d5 1>&2 );              echo "  1>&2   d5=[$(tr '\n' ',' < d5)]"
mk d6; ( { read -r l <&2; } 1<>d6 2>&1;             echo "  read   l=[$l]" )

echo "=== the order in the list decides, as ever"
mk o1; ( { echo W; } 1<>o1 >o1b );           echo "  after  o1=[$(tr '\n' ',' < o1)] o1b=[$(cat o1b)]"
mk o2; ( { echo W; } >o2b 1<>o2 );           echo "  before o2=[$(tr '\n' ',' < o2)] o2b=[$(cat o2b)]"
mk o3; ( echo W >&2 2<>o3 );                 echo "  dup1st o3=[$(tr '\n' ',' < o3)]"

echo "=== an external child is handed the same descriptor"
mk x1; ( sh -c 'echo W' 1<>x1 );             echo "  x1=[$(tr '\n' ',' < x1)]"
mk x2; ( sh -c 'echo W >&2' 2<>x2 );         echo "  x2=[$(tr '\n' ',' < x2)]"
mk x3; ( sh -c 'echo AA; echo BB' 1<>x3 );   echo "  x3=[$(tr '\n' ',' < x3)]"

echo "=== exec installs it too"
mk e1; ( exec 1<>e1; echo W; read -r l <&1; echo "  exec   l=[$l]" >&2 ); echo "  e1=[$(tr '\n' ',' < e1)]"
mk e2; ( exec 2<>e2; echo W >&2 );           echo "  e2=[$(tr '\n' ',' < e2)]"

echo "=== every body that carries its own redirect list answers alike"
mk y1; ( f() { echo W; }; f 1<>y1 );                echo "  func   y1=[$(tr '\n' ',' < y1)]"
mk y2; ( for i in 1; do echo W; done 1<>y2 );       echo "  loop   y2=[$(tr '\n' ',' < y2)]"
mk y3; ( if :; then echo W; fi 1<>y3 );             echo "  if     y3=[$(tr '\n' ',' < y3)]"
mk y4; ( while :; do echo W; break; done 1<>y4 );   echo "  while  y4=[$(tr '\n' ',' < y4)]"
mk y5; ( ( echo W ) 1<>y5 );                        echo "  sub    y5=[$(tr '\n' ',' < y5)]"
mk y6; ( { echo W; } 1<>y6 | cat > /dev/null );     echo "  pipe   y6=[$(tr '\n' ',' < y6)]"

exec 2>&4 4>&-
echo "=== what went to stderr"
cat err
