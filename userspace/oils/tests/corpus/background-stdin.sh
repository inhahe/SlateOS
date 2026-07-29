# Which fd 0 a `&` job gets.
#
# bash disconnects an asynchronous command's input — reads it from /dev/null —
# but *only in the absence of a redirection*. Its `stdin_redir` flag is set by
# `do_piping` when the command is a pipeline stage and assigned by a *compound*
# command's redirect list; when it is set, the job simply inherits fd 0 as it
# stands, sharing the file offset with the shell.
#
# The flag is per-redirect-list, not "fd 0 is not a terminal": an `exec < file`
# belongs to no command's list, so a job started afterwards still reads
# /dev/null. A compound list *assigns* the flag, so a nested `{ …; } > f` clears
# what an enclosing `< file` set. A simple command's list — a function call,
# `eval`, `.` — does not assign it at all, even though fd 0 really does change.
# And the list consulted at the fork is the *async node's own*: a redirect
# buried inside a group, or attached to one member of an and-or list, does not
# count.
#
# See known-issues.md TD-OILS-BG-STDIN-REDIR.

printf 'l1\nl2\n' > two.txt
printf 'r1\nr2\nr3\nr4\n' > four.txt

echo "=== no redirection: /dev/null, even with fd 0 rebound by exec"
( exec < two.txt
  cat & wait; echo "  unredirected=[]"
  { cat; } & wait; echo "  group=[]"
  read a & wait; echo "  read=[${a-unset}]"
  # …and the shell's own fd 0 is untouched by any of that: still at line 1.
  read b; echo "  parent=[$b]" )

echo "=== a redirection of the job's own is honoured, as always"
cat < two.txt & wait
( cat ) < two.txt & wait
cat < /dev/null & wait; echo "  explicit-null=[]"

echo "=== a pipeline stage inherits its stage input"
echo A | { cat & wait; }
echo B | ( cat & wait )
x=$(echo Z | { cat & wait; }); echo "  captured=[$x]"
# The stage's own list is applied *before* the pipe is, so a list that says
# nothing about fd 0 does not cost the stage its input …
echo C | { cat & wait; } 2>/dev/null
# … while the same list one level in does: it is that node's assignment.
echo D | { { cat & wait; } > nested.txt; }; echo "  nested=[$(cat nested.txt)]"

echo "=== an enclosing redirect scope counts as a redirection"
{ cat & wait; } < two.txt
( cat & wait ) < two.txt
{ { cat & wait; }; } < two.txt
for i in 1; do cat & wait; done < two.txt
while read -r l; do echo "  loop-body-job:"; cat & wait; break; done < two.txt

echo "=== ... and an inner scope's list can take it away again"
{ { cat & wait; } > inner.txt; } < two.txt; echo "  cleared=[$(cat inner.txt)]"
{ { cat & wait; } 2>/dev/null; } < two.txt; echo "  cleared-stderr=[]"
# Symmetrically, an inner `< file` re-establishes it under a scope that cleared.
{ { cat & wait; } < two.txt; } 2>/dev/null

echo "=== but a *simple* command's list does not count, however it changes fd 0"
f() { cat & wait; }; f < two.txt; echo "  function=[]"
eval 'cat & wait' < two.txt; echo "  eval=[]"
printf 'cat & wait\n' > inner.sh
. ./inner.sh < two.txt; echo "  dot=[]"
# The flag and the descriptor are genuinely independent: here the outer group
# sets the flag while the call rebinds fd 0, so the job reads the *inner* file.
{ f < two.txt; } < four.txt

echo "=== including a here-string and a here-document"
{ cat & wait; } <<< "hs"
{ cat & wait; } <<EOT
hd
EOT

echo "=== the offset really is shared: what the job reads, the shell does not"
{ read v & wait; read w; echo "  w=[$w]"; } < four.txt

echo "=== only the async node's *own* list counts"
# A dup of fd 0 onto itself changes nothing about the source, so it is a pure
# test of the flag: with it, the job inherits; without it, /dev/null.
( exec < two.txt; cat 0<&0 & wait )
( exec < two.txt; cat <&0 & wait )
( exec < two.txt; { cat; } 0<&0 & wait )
( exec < two.txt; cat 0<&0 | cat & wait )
echo "  --- and the shapes where it does not count:"
( exec < two.txt; cat 0<&0 && echo "  and-or=ok" & wait )
( exec < two.txt; { cat 0<&0; } & wait; echo "  inner-group=[]" )
( exec < two.txt; ( cat 0<&0 ) & wait; echo "  inner-subshell=[]" )
( exec < two.txt; cat > /dev/null & wait; echo "  write-only=[]" )

echo "=== a job reading a descriptor it was handed by exec is unaffected"
( exec 3< two.txt; cat 0<&3 & wait )
