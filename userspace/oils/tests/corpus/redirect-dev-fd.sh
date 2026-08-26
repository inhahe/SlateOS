# bash's *special redirection filenames*: `/dev/stdin`, `/dev/stdout`,
# `/dev/stderr` and `/dev/fd/N`. `> /dev/stderr` must reach fd 2, not create a
# file called `/dev/stderr`.
#
# They are commonly described as naming "a descriptor to duplicate", and the
# bash manual's own wording invites that reading — but on a host with
# `/dev/fd` (`HAVE_DEV_FD`, which is every Linux) bash does not interpret the
# names at all. It passes them to `open()` and lets the kernel resolve the
# path, which yields a *new open file description* on the same file. The
# difference is observable six ways, each probed below: the new description
# starts at offset 0 and leaves the shell's own cursor alone; `>` truncates;
# `set -C` has a real file to protect; the descriptor's access mode does not
# constrain the redirect; an unlinked file re-opens from its live inode; and a
# pipe re-opens as the same pipe, with no rewind.
#
# Every probe here keeps the shell's fd 1 on something the shell itself made —
# a command substitution, a pipeline, a file — never the harness's own pipe,
# so the case reads the same however it is invoked. `/dev/stdin` is left to the
# in-process tests in `interp.rs` for the same reason: fd 0 is the harness's.

echo "=== /dev/stdout reaches fd 1's file"
x=$(echo hi > /dev/stdout); echo "  x=[$x]"
{ echo hi > /dev/stdout; } | sed 's/^/  piped: /'
echo "  [$(echo one > /dev/stdout; echo two)]"
echo "--- including where fd 1 currently points, not where it started"
( exec > g.txt; echo hi > /dev/stdout ); echo "  g=[$(cat g.txt)]"
echo "--- and it is an open, so > truncates what fd 1 already wrote"
( exec > g.txt; echo AAAAAAAA; echo B > /dev/stdout ); echo "  g=[$(cat g.txt)]"
echo "--- while >> appends to it"
( exec > g.txt; echo AAAAAAAA; echo B >> /dev/stdout ); echo "  g=[$(cat g.txt)]"

echo "=== /dev/stdout as a stderr target merges the two streams"
x=$( { echo E >&2; } 2>/dev/stdout ); echo "  x=[$x]"
echo "--- interleaved in write order, not stdout-then-stderr"
x=$( { echo E >&2; echo O; } 2>/dev/stdout ); echo "  x=[$x]"
echo "--- left to right: 2>/dev/stdout then >file leaves fd 2 behind"
x=$( { echo E >&2; echo O; } 2>/dev/stdout >f.txt ); echo "  x=[$x] f=[$(cat f.txt)]"

echo "=== /dev/fd/N opens descriptor N's file afresh"
exec 3>f.txt; echo hi >/dev/fd/3; exec 3>&-; echo "  f=[$(cat f.txt)]"
printf 'from-fd3\n' > f2.txt
exec 3<f2.txt; read l </dev/fd/3; exec 3<&-; echo "  l=[$l]"
echo "--- the new description starts at 0, where a dup would carry the cursor"
printf 'one\ntwo\n' > f2.txt
exec 3<f2.txt; read -u 3 l; echo "  reopen=[$(cat </dev/fd/3 | tr '\n' ' ')]"
echo "  dup=[$(cat <&3 | tr '\n' ' ')]"
exec 3<&-
echo "--- and reading it does not move the shell's own cursor"
exec 3<f2.txt; read -u 3 l; cat </dev/fd/3 >/dev/null; read -u 3 m; exec 3<&-
echo "  l=[$l] m=[$m]"
echo "--- > through it truncates the file, >> appends"
exec 3>f.txt; echo AAAAAAAA >&3; echo B >/dev/fd/3; exec 3>&-; echo "  f=[$(cat f.txt)]"
exec 3>f.txt; echo AAAAAAAA >&3; echo B >>/dev/fd/3; exec 3>&-; echo "  f=[$(cat f.txt)]"
echo "--- so noclobber has a file to protect, and >| still overrides it"
exec 3>f.txt; echo AAAAAAAA >&3
x=$( { set -C; echo B >/dev/fd/3; } 2>&1 ); echo "  rc=$? x=[${x##*: }]"
( set -C; echo B >|/dev/fd/3 ); exec 3>&-; echo "  f=[$(cat f.txt)]"
echo "--- the descriptor's access mode does not constrain the redirect"
printf 'ZORK\n' > f2.txt
exec 3<f2.txt; echo ZZZ >/dev/fd/3; exec 3<&-; echo "  f2=[$(cat f2.txt)]"
exec 3>f.txt; echo AAAAAAAA >&3; echo "  back=[$(cat </dev/fd/3)]"; exec 3>&-
echo "--- exec through it binds a second, independent description"
printf 'one\ntwo\n' > f2.txt
exec 3<f2.txt; read -u 3 l; exec 4</dev/fd/3; read -u 4 m; read -u 3 n
exec 3<&- 4<&-; echo "  l=[$l] m=[$m] n=[$n]"

echo "=== a path the kernel cannot resolve is an ordinary open failure"
echo "--- an unbound descriptor is reported against the path"
x=$( { echo hi >/dev/fd/9; } 2>&1 ); echo "  rc=$? x=[${x##*: /dev}]"
x=$( { cat </dev/fd/9; } 2>&1 ); echo "  rc=$? x=[${x##*: /dev}]"
x=$( { exec 3</dev/fd/9; } 2>&1 ); echo "  rc=$? x=[${x##*: /dev}]"
echo "--- a descriptor the same list closed is gone before the open"
printf 'one\n' > f2.txt
exec 3<f2.txt; x=$( { cat </dev/fd/3 3<&-; } 2>&1 ); echo "  rc=$? x=[${x##*: /dev}]"
exec 3<&-
echo "--- 7> /dev/fd/7 cannot bootstrap itself: fd 7 is not open yet"
x=$( { echo hi 7>/dev/fd/7; } 2>&1 ); echo "  rc=$? x=[${x##*: /dev}]"
echo "--- an unlinked file re-opens from its live inode: the path is gone"
printf 'one\n' > f3.txt
exec 3<f3.txt; rm f3.txt; x=$( { cat </dev/fd/3; } 2>&1 ); echo "  rc=$? x=[${x##*: /dev}]"
exec 3<&-
echo "--- and a directory behind the descriptor is EISDIR on the read"
mkdir -p dd
exec 3<dd; x=$( { cat </dev/fd/3; } 2>&1 ); echo "  rc=$? x=[${x##*: }]"
exec 3<&-

echo "=== a non-numeric or absent fd part is an ordinary filename"
mkdir -p dev/fd
echo hi > dev/fd/x; echo "  dev/fd/x=[$(cat dev/fd/x)]"

rm -rf f.txt f2.txt f3.txt g.txt dd dev
