# `1< file` is a legal redirect. Nothing reserves fd 1 for writing: the
# redirect binds it to a description opened read-only, exactly as `3< file`
# binds fd 3, and the descriptor that results answers the three questions a
# descriptor is asked separately —
#
#   * it is *open*, so a `3>&1` beside it copies something rather than being
#     refused;
#   * it has a *read half*, so `read <&1` finds the file's bytes, and finds
#     them from one shared cursor;
#   * it has no *write half*, so every write through it is
#     `echo: write error: Bad file descriptor` and status 1.
#
# The same for fd 2, with one difference that is only visible because fd 2 is
# where diagnostics go: a failure that cannot be reported on fd 2 is dropped
# and only the status survives. `cd /nosuchdir 2<in` exits 1 in silence, and so
# — since the shell sends its own command-level diagnostics to the command's
# fd 2, not to its own — does `nosuchcmd 2<in`, at 127.
#
# A dup is one `dup2`, so `2<&3` after a `3< file` carries the missing write
# half across and `3>&2` after a `2<in` carries it back; the arrow either was
# written with picks nothing. `2>&1` is that same dup, so it too follows fd 1
# into having no write half. `<>` is the source that has both halves, and a
# dup of it keeps both.
#
# Which source it is does not enter into it: a here-document on fd 1 is as
# read-only as a file is.
#
# Which fd 1 a dup copies is, as ever, a question of where it sits: `1<in >out`
# writes to `out`, and `>out2 1<in` fails.
#
# Deliberately absent:
#
#   * every shape whose command is *external*. `sh -c 'echo W' 1<in` is
#     `EBADF` in the child under bash; osh hands the child the ambient stdout
#     instead, as it does for every descriptor it models but does not pass on.
#     See TD-OILS-EXTERNAL-CHILD-HAS-NO-FD-3.
#   * `1>&0 <>rw` and `1>&0 0<&-`, where the entry *after* the dup changes what
#     fd 0 is. osh resolves fd 0's write half against the finished plan rather
#     than against the point in the list the dup sat at, so it copies the `<>`
#     and the close rather than the ambient fd 0 both shells started with. See
#     TD-OILS-DUP-OF-STDOUT-IS-NOT-THE-LIST-SO-FAR.
#   * `<>` beyond the two shapes below that show it keeps both halves. It is
#     the source with a write half as well as a read one, so the questions it
#     raises are about the *offset* the two share rather than about a missing
#     half; they have a case of their own,
#     `a-read-write-source-on-a-std-fd-keeps-one-offset.sh`.
#
# Every persistent probe runs in a subshell so an `exec` cannot reach the next
# one. Stderr is collected and replayed at the end so it can be compared in a
# fixed place; nothing here prints a pid, so it is replayed unfiltered.
printf 'one\ntwo\n' > in
printf 'A\nB\n' > rw
printf 'A\nB\n' > rw2
exec 4>&2 2>err

echo "=== a read-only fd 1 has no write half"
( { echo W; } 1<in;                  echo "  1<in rc=$?" )
( { printf 'W\n'; } 1<in;            echo "  printf rc=$?" )
( { echo W; } 1<&0 <in;              echo "  1<&0 rc=$?" )
( exec 1<in; echo W );               echo "  exec rc=$?"

echo "=== and a read-only fd 2 has none either, with nowhere to say so"
( { echo W >&2; } 2<in;              echo "  2<in rc=$?" )
( { echo W >&2; } 2<&0 <in;          echo "  2<&0 rc=$?" )
( exec 2<in; echo W >&2 );           echo "  exec rc=$?"
( { cd /nosuchdir; } 2<in;           echo "  cd rc=$?" )
( nosuchcmd 2<in;                    echo "  notfound rc=$?" )

echo "=== but both are open, and reads through them find bytes"
( { read -r l <&1; } 1<in;           echo "  1<in rc=$? l=[$l]" )
( { read -r l <&2; } 2<in;           echo "  2<in rc=$? l=[$l]" )
( { read -r a <&1; read -r b <&1; } 1<in; echo "  shared a=[$a] b=[$b]" )
( exec 2<in; read -r l <&2;          echo "  exec rc=$? l=[$l]" )
( { read -r l <&2; } 3<in 2<&3;      echo "  chain rc=$? l=[$l]" )

echo "=== a dup of one carries the same missing write half"
( { echo W >&3; } 2<in 3>&2;         echo "  3>&2 rc=$?" )
( { read -r l <&3; } 2<in 3<&2;      echo "  3<&2 rc=$? l=[$l]" )
( { echo W >&3; } 1<in 3>&1;         echo "  3>&1 rc=$?" )

echo "=== and a dup onto fd 1 of a source with none is made, not refused"
( { echo W; } 3<in 1>&3;             echo "  3<in rc=$?" )
( { echo W; } 3<in 1<&3;             echo "  arrow rc=$?" )
( exec 0<in; { echo W; read -r l <&1; } 1>&0; echo "  1>&0 rc=$? l=[$l]" )
( { echo W; } 3<&- 1>&3;             echo "  closed rc=$?" )

echo "=== a here-document is a source like any other, on fd 1 as on fd 0"
( { echo W; } 1<<<'zz';              echo "  fd1 rc=$?" )
( { read -r l <&1; } 1<<<'zz';       echo "  fd1-read rc=$? l=[$l]" )
( { echo W >&2; } 2<<'H'
zz
H
echo "  fd2 rc=$?" )
( { read -r l <&2; } 2<<'H'
zz
H
echo "  fd2-read rc=$? l=[$l]" )

echo "=== and 2>&1 after it copies a fd 1 that cannot be written either"
( { echo W; } 1<in 2>&1;             echo "  2>&1 rc=$?" )
( { read -r l <&2; } 1<in 2>&1;      echo "  2>&1-read rc=$? l=[$l]" )
( { echo W; } 2<in 1>&2;             echo "  1>&2 rc=$?" )
( { echo W; } 2>&1 1<in;             echo "  before rc=$?" )

echo "=== an <> source is the case that keeps both"
( { echo W >&2; } 3<>rw 2<&3;        echo "  rw rc=$?"; cat rw )
( { echo W; } 3<>rw2 1>&3;           echo "  rw2 rc=$?"; cat rw2 )

echo "=== the order in the list decides, as ever"
( { echo W; } 1<in >out;             echo "  read-then-write rc=$? out=[$(cat out)]" )
( { echo W; } >out2 1<in;            echo "  write-then-read rc=$? out2=[$(cat out2)]" )

echo "=== every body that carries its own redirect list answers alike"
( f() { echo W; }; f 1<in;                  echo "  func   rc=$?" )
( for i in 1; do echo W; done 1<in;         echo "  loop   rc=$?" )
( if :; then echo W; fi 1<in;               echo "  if     rc=$?" )
( while :; do echo W; break; done 1<in;     echo "  while  rc=$?" )

exec 2>&4 4>&-
echo "=== what went to stderr"
cat err
