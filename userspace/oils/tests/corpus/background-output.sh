# A `&` job is a forked child, so it inherits fd 1 as it stood where the job was
# *started* — not the shell's terminal. Inside `$( … )` or a pipeline stage that
# is the collecting pipe, which has two visible consequences: the job's output
# lands in the substitution rather than escaping to the terminal, and the
# substitution does not finish until the job exits, because the reader only sees
# EOF once every copy of the write end is closed.
#
# Every job below is drained before its effect is read, so nothing here races.

echo "=== a job started inside a substitution writes into it"
printf 'hi\n' > src.txt
x=$(cat src.txt &); echo "bare=[$x]"
x=$(cat src.txt & wait); echo "waited=[$x]"
x=$(echo bi &); echo "builtin=[$x]"
x=$(exec cat src.txt &); echo "exec=[$x]"
# The job's writes interleave with the substitution's own in write order.
x=$(printf a; printf b & wait); echo "order=[$x]"

echo "=== …but its status is not the substitution's"
# bash's substitution ends when the pipe closes, and the job's exit status is
# never consulted on that path.
x=$(false &); echo "rc=$?"

echo "=== the wait is real: a redirection away from the sink removes it"
# With the job's own fd 1 pointed elsewhere there is no copy of the pipe left in
# the child, so the substitution collects nothing.
x=$(echo s > /dev/null &); echo "redirected=[$x]"
x=$(cat src.txt > o1.txt & wait); echo "tofile=[$x] file=[$(cat o1.txt)]"
# fd 2 is a separate inheritance; `1>&2` in the job leaves fd 1 unused.
x=$(echo t 1>&2 & wait) 2>/dev/null; echo "tostderr=[$x]"

echo "=== a compound command backgrounded whole writes there too"
x=$( { echo g1; echo g2; } & wait ); echo "group=[$x]"
x=$(for i in 1 2; do echo i$i; done & wait); echo "loop=[$x]"
f() { echo fn; }
x=$(f &); echo "func=[$x]"
# `2>&1` inside the job points its fd 2 at the same sink, as it would for any
# other command in the substitution.
x=$( { echo e >&2; } 2>&1 & wait ); echo "mergedstderr=[$x]"

echo "=== a pipeline stage is the same sink"
echo pre; { cat src.txt & wait; } | tr a-z A-Z
echo pre; { cat src.txt & } | tr a-z A-Z

echo "=== and so is a file fd 1 was bound to"
( exec > ex.out; cat src.txt & wait ); cat ex.out
{ echo p & wait; } > po.txt; cat po.txt
# The redirection belongs to the group, so it is still in force for a job the
# group leaves running behind it.
{ echo q & } > qo.txt; wait; cat qo.txt
