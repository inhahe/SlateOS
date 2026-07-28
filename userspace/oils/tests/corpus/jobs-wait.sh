# Background jobs and `wait`. Everything here is written to be *deterministic*:
# output is either produced after an explicit `wait`, or funnelled through a
# file that is read only once all writers have been reaped. Nothing depends on
# how fast the OS schedules a child, so this case is safe to diff.

# `cmd &` returns immediately with status 0 — the status of the *fork*, never
# of the command — and puts the child's pid in $!.
false & bg1=$!
echo "amp-status=$?"
wait "$bg1"; echo "waited-false=$?"

# $! survives until the next background command replaces it. A targeted `wait`
# on an already-reaped pid is *repeatable*: the shell remembers the terminated
# job's status and keeps answering with it, silently, rather than complaining
# that the pid is not a child.
true & pid=$!
wait "$pid"; echo "waited-true=$?"
wait "$pid" 2>/dev/null; echo "rewait=$?"
( exit 6 ) & p6=$!
wait "$p6" >/dev/null 2>&1
wait "$p6" 2>/dev/null; echo "remembered=$?"
wait "$p6" 2>/dev/null; echo "remembered-again=$?"
# That memory is not in the `jobs` table, though — a reaped job disappears from
# the listing immediately even while `wait` still answers for it.
jobs; echo "jobs-empty=$?"
# An argument-less `wait` is the purge point: after it, the same pid is once
# again "not a child of this shell".
wait
wait "$p6" 2>/dev/null; echo "after-purge=$?"

# `wait` with no arguments waits for *all* children and always reports 0, even
# when some of them failed.
( exit 3 ) &
( exit 0 ) &
wait
echo "wait-all=$?"

# `wait` with no children at all is also 0 — not an error.
wait; echo "wait-none=$?"

# A background job's exit status is preserved exactly, including values above
# 128 and the 8-bit wrap of `exit 256`.
( exit 42 ) & wait $!; echo "s42=$?"
( exit 200 ) & wait $!; echo "s200=$?"
( exit 256 ) & wait $!; echo "s256=$?"

# Several jobs at once: reap them in launch order and each keeps its own
# status. `wait pid1 pid2` reports the status of the *last* pid listed.
( exit 1 ) & p1=$!
( exit 2 ) & p2=$!
( exit 3 ) & p3=$!
wait "$p1"; s1=$?
wait "$p2"; s2=$?
wait "$p3"; s3=$?
echo "each=$s1,$s2,$s3"
( exit 4 ) & q1=$!
( exit 5 ) & q2=$!
wait "$q1" "$q2"; echo "wait-two=$?"

# A background job runs in a subshell, so its variable assignments never reach
# the parent — the same isolation as an explicit ( ).
v=parent
v=child & wait
echo "v=$v"

# The parent's variables *are* inherited by the child, though.
outer=visible
( echo "child-sees=$outer" > seen.txt ) & wait
cat seen.txt

# Background jobs write to the same stdout; serialise them through per-job
# files so the diff is stable, then read after the barrier.
for i in 1 2 3; do
    ( echo "job$i" > "j$i.txt" ) &
done
wait
cat j1.txt j2.txt j3.txt

# `$!` inside the background job itself refers to the *parent's* last job, not
# to the job's own pid — one of the more surprising corners.
( exit 0 ) & first=$!
( [ "$!" = "$first" ] && echo "inherits-bang=yes" || echo "inherits-bang=no" ) > bang.txt &
wait
cat bang.txt

# A background job at the end of a pipeline backgrounds the whole pipeline, and
# `$!` is the pid of the *last* stage.
printf 'b\na\n' | sort > sorted.txt &
wait $!
tr '\n' ' ' < sorted.txt; echo

# Backgrounding does not disturb `$?` of the previous command.
false
true & wait $! >/dev/null 2>&1
: # keep $? from the wait above out of the way
false; bgnoise=$?
true &
echo "prev-status=$bgnoise"
wait

# `wait` on a pid that was never a child of this shell: status 127 plus a
# message. Pid 1 exists but is not ours.
wait 1 2>/dev/null; echo "wait-stranger=$?"

# Command substitution waits for its own subshell, so a `&` inside one still
# completes before the substitution's value is taken — provided the job's
# output is captured through a file the substitution reads after `wait`.
val=$( { echo captured > c.txt; } & wait; cat c.txt )
echo "cmdsub-bg=$val"

# `wait -n` returns as soon as *any one* job finishes, with that job's status.
# With a single job outstanding it is deterministic.
( exit 9 ) &
wait -n; echo "wait-n=$?"

# Traps still fire for a backgrounded subshell's own EXIT trap, and the output
# lands in the file rather than racing the parent's stdout.
( trap 'echo "bg-exit-trap" >> t.txt' EXIT; echo "bg-body" >> t.txt ) &
wait
cat t.txt
