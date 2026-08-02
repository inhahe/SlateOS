# `{v}>file` is the redirect that does not name its descriptor: the shell picks
# one and tells the script which by storing the number in the variable `v`. The
# number is the lowest free descriptor >= 10 — that range is reserved for these,
# so an auto-allocated fd never collides with a hand-written `exec 3>`. The
# descriptor is *persistent*: `{ :; } {v}>f` leaves it open after the command,
# exactly as `exec {v}>f` does, and only `shopt -s varredir_close` takes it back
# again when the command ends.
#
# The two halves happen in that order — open, then store — and the order is
# visible from both sides. A redirect that cannot open leaves the variable
# exactly as it was, unset if it was unset; and a *readonly* variable is refused
# only once the file has been created and truncated, with two diagnostics, and
# with the descriptor handed straight back to the next varfd.
#
# `{v}>&-` is the one form that allocates nothing: `$v`'s current value names
# the descriptor to close, and the variable is left holding it. The value is
# read as a whole-string integer — surrounding whitespace skipped, a leading `+`
# or `-` taken as a sign — and refused outside `0 … INT_MAX`. A number naming a
# descriptor that is not open is not an error; the `EBADF` is ignored, exactly
# as it is for a literal `9>&-`.
#
# Two shapes perform the redirection and then take it all back. A *null*
# command — a redirect list with no command word — creates and truncates the
# file, reports an unopenable path, and never writes the variable at all. And a
# `( … )` subshell's own list is applied in the fork, so the body sees `$v` and
# the shell that wrote the redirect never does. Both hand the number back.
#
# Deliberately absent:
#
#   * `{v}>&-` where `$v` is not a number at all. bash closes fd 0 — silently,
#     at status 0 — because the value it failed to parse is left as a 0 it then
#     treats as a descriptor. osh refuses with `v: ambiguous redirect`. See
#     TD-OILS-VARFD-CLOSE-OF-A-NON-NUMBER-CLOSES-STDIN.
#   * every redirect *failure* written on a group or loop command that is nested
#     inside another compound command. bash reports those against the line of
#     the token following the command rather than the command's own; see
#     TD-OILS-REDIR-ERROR-LINE-IS-THE-NEXT-TOKEN. Every failure below is on a
#     simple command or an `exec`, which both shells report exactly.
#
# Every persistent probe runs in a subshell so a descriptor cannot reach the
# next one. Stderr is collected and replayed at the end so it can be compared in
# a fixed place; nothing here prints a pid, so it is replayed unfiltered.
exec 4>&2 2>err

echo "=== the number is the lowest free descriptor at or above 10"
( { :; } {a}>o1;                     echo "  one     a=$a" )
( { :; } {a}>o1; { :; } {b}>o2;      echo "  two     a=$a b=$b" )
( { :; } {a}>o1 {b}>o2;              echo "  onelist a=$a b=$b" )
( exec 3>o3; { :; } {a}>o1;          echo "  past3   a=$a" )
( exec 10>o4; { :; } {a}>o1;         echo "  past10  a=$a" )
( { :; } {a}<o1;                     echo "  input   a=$a" )
( { :; } {a}>>o1;                    echo "  append  a=$a" )
( { :; } {a}>&1;                     echo "  dup     a=$a" )

echo "=== and the descriptor outlives the command that made it"
( { :; } {a}>p1; echo W >&$a; exec {a}>&-;  echo "  group  p1=[$(cat p1)]" )
( exec {a}>p2; echo W >&$a; exec {a}>&-;    echo "  exec   p2=[$(cat p2)]" )
( f() { :; }; f {a}>p3; echo W >&$a; exec {a}>&-; echo "  func   p3=[$(cat p3)]" )

echo "=== the store happens after the open, so a failed open names nothing"
( v=pre; echo W {v}>/nosuch/dir/f;   echo "  simple rc=$? v=[$v]" )
( exec {v}>/nosuch/dir/f;            echo "  exec   rc=$? v=[${v-UNSET}]" )
( v=pre; echo W {v}>/nosuch/dir/f; { :; } {w}>o1; echo "  next   w=$w" )
( v=7; { :; } {v}>ov$v; [ -e ov7 ] && echo "  target v=$v made ov7" )

echo "=== a readonly variable is refused only once the file has been made"
printf 'old\n' > ro1
( readonly v; echo W {v}>ro1;        echo "  rc=$? ro1=[$(cat ro1)]" )
( readonly v; echo W {v}>ro2; { :; } {w}>o1; echo "  next w=$w" )

echo "=== the close form reads the variable rather than allocating"
( exec {v}>c1; echo A >&$v; exec {v}>&-; echo "  closed rc=$? v=[$v] c1=[$(cat c1)]" )
( exec {v}>c2; n=$v; exec {v}>&-; echo B >&$n; echo "  gone   rc=$?" )
( exec 10>c3; v=' 10 '; exec {v}>&-; echo "  spaces close=$?"; echo W >&10; echo "  spaces write=$?" )
( exec 9>c4; v=+9; exec {v}>&-; echo "  plus   close=$?"; echo W >&9; echo "  plus   write=$?" )
( v=9; exec {v}>&-;                  echo "  notopen rc=$?" )
( v=0; exec {v}>&-;                  echo "  zero    rc=$?" )
( v=-1; exec {v}>&-;                 echo "  neg     rc=$?" )
( v=2147483648; exec {v}>&-;         echo "  big     rc=$?" )
( v=2147483647; exec {v}>&-;         echo "  max     rc=$?" )
( v=; exec {v}>&-;                   echo "  empty   rc=$?" )
( exec {v}>&-;                       echo "  unset   rc=$?" )
( { :; } {v}<&-;                     echo "  arrow   rc=$?" )

echo "=== shopt -s varredir_close takes the descriptor back, not the number"
( shopt -s varredir_close; { :; } {v}>vc1; echo "  v=$v"; echo W >&$v; echo "  write rc=$?" )
( shopt -s varredir_close; { :; } {a}>vc2; { :; } {b}>vc3; echo "  reused a=$a b=$b" )
( shopt -s varredir_close; exec {v}>vc4; echo W >&$v; echo "  exec   rc=$? vc4=[$(cat vc4)]" )
( shopt -s varredir_close; { echo W >&$v; } {v}>vc5; echo "  body   rc=$? vc5=[$(cat vc5)]" )

echo "=== a null command redirects without ever naming itself"
( v=pre; {v}>nc1;                    echo "  rc=$? v=[$v] made=[$(cat nc1)]" )
( {v}>nc2;                           echo "  unset  v=[${v-UNSET}]" )
( x=1 {v}>nc3;                       echo "  assign x=$x v=[${v-UNSET}]" )
( {v}>nc4; { :; } {w}>o1;            echo "  next   w=$w" )
printf 'old\n' > nc5
( {v}>nc5;                           echo "  trunc  nc5=[$(cat nc5)]" )
( {v}>/nosuch/dir/f;                 echo "  fail   rc=$?" )

echo "=== a subshell's list belongs to the subshell"
( v=pre; ( echo "  inner  v=[$v]" ) {v}>sb1; echo "  outer  v=[$v]" )
( ( : ) {v}>sb2;                     echo "  unset  v=[${v-UNSET}]" )
( ( : ) {v}>sb3; { :; } {w}>o1;      echo "  next   w=$w" )
( ( echo W >&$v ) {v}>sb4;           echo "  bound  rc=$? sb4=[$(cat sb4)]" )
( v=pre; { echo "  group  v=[$v]"; } {v}>o1; echo "  after  v=[$v]" )

echo "=== every body that carries its own redirect list answers alike"
( f() { echo "  func   v=$v"; }; f {v}>y1 )
( for i in 1; do echo "  loop   v=$v"; done {v}>y2 )
( if :; then echo "  if     v=$v"; fi {v}>y3 )
( while :; do echo "  while  v=$v"; break; done {v}>y4 )
( case x in x) echo "  case   v=$v" ;; esac {v}>y5 )
( { echo "  group  v=$v"; } {v}>y6 )

exec 2>&4 4>&-
echo "=== what went to stderr"
cat err
