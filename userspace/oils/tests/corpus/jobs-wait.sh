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
# It names no job, so it reports no job's status — not even when there is only
# one job and it failed, and not even when the job was killed. Only `wait PID`,
# `wait %n` and `wait -n` answer with a job's own status.
( exit 3 ) & wait; echo "wait-one-failed=$?"
( exit 3 ) & wait -f; echo "wait-f=$?"
( exit 3 ) & wait --; echo "wait-dashdash=$?"
sleep 5 & kill $! 2>/dev/null; wait; echo "wait-killed=$?"

# `wait` with no children at all is also 0 — not an error.
wait; echo "wait-none=$?"

# `-p VAR` is where the pid of the job whose status is reported goes, so a
# `wait` that reports no particular job leaves VAR *unset* — it is cleared
# before the wait, not left holding whatever it had.
VAR=stale; ( exit 3 ) & wait -p VAR; echo "p-noargs=$? [${VAR-unset}]"
VAR=stale; wait -n -p VAR; echo "p-nothing=$? [${VAR-unset}]"
# With a job to name, VAR holds its pid — compared against `$!` rather than
# printed, since a pid is not reproducible.
( exit 3 ) & bgp=$!
wait -p VAR "$bgp"; echo "p-named=$? [$([ "$VAR" = "$bgp" ] && echo match)]"
( exit 3 ) & bgp=$!
wait -n -p VAR; echo "p-next=$? [$([ "$VAR" = "$bgp" ] && echo match)]"
# …and it has to be a name. That is checked before anything is waited for.
( exit 3 ) & wait -p '1x'; echo "p-badname=$?"
( exit 3 ) & wait -p ''; echo "p-emptyname=$?"
wait >/dev/null 2>&1

# The options are read with getopt, so they cluster and `-p` takes either the
# rest of its own cluster or the next word.
( exit 3 ) & wait -fn; echo "cluster-fn=$?"
( exit 3 ) & wait -nf; echo "cluster-nf=$?"
VAR=stale; ( exit 3 ) & wait -npVAR; echo "cluster-npVAR=$? [$([ -n "$VAR" ] && echo set)]"
VAR=stale; ( exit 3 ) & wait -fnp VAR; echo "cluster-fnp=$? [$([ -n "$VAR" ] && echo set)]"
# An unknown letter anywhere in the cluster is the one that gets named.
wait -x; echo "opt-x=$?"
wait -nx; echo "opt-nx=$?"
wait -1; echo "opt-1=$?"
wait -p; echo "opt-p-noarg=$?"
wait -np; echo "opt-np-noarg=$?"
# A lone `-` is not an option at all — it is an operand, and an unusable one.
wait -; echo "opt-dash=$?"
wait -- -n; echo "opt-after-dashdash=$?"

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

# …and it stays 127 even while a job *numbered* 1 is running, because a bare
# number is a pid to `wait` and never a job number. bash splits its builtins
# here: `wait`, `kill` and `disown` take "jobspec or pid" and read a bare number
# as a pid, while `jobs`, `fg` and `bg` read one as a job number. A `%1` reaches
# the job from either side. (This split is what made `wait-stranger` above look
# intermittent rather than plainly wrong: it only diverged on the runs where a
# job 1 happened to still be in the table.)
#
# The job has to still be *running* for any of that to mean anything, and a
# timed one is not good enough: on a loaded machine the five probes below can
# take longer than the sleep, and then `jobs` reports the finished job, both
# shells purge it, and `wait %1` says "no such job" instead. So the job blocks
# on a file we create when we are done with it rather than on a clock.
( while [ ! -e release-job1 ]; do sleep 0.05; done ) &
wait 1 2>/dev/null; echo "live-job-not-a-pid=$?"
kill -0 1 2>/dev/null; echo "kill-bare=$?"
kill -0 %1 2>/dev/null; echo "kill-spec=$?"
disown 1 2>/dev/null; echo "disown-bare=$?"
jobs 1 >/dev/null 2>&1; echo "jobs-bare=$?"
: > release-job1
wait %1; echo "wait-spec=$?"

# Command substitution waits for its own subshell, so a `&` inside one still
# completes before the substitution's value is taken — provided the job's
# output is captured through a file the substitution reads after `wait`.
val=$( { echo captured > c.txt; } & wait; cat c.txt )
echo "cmdsub-bg=$val"

# `wait -n` returns as soon as *any one* job finishes, with that job's status.
# With a single job outstanding it is deterministic.
( exit 9 ) &
wait -n; echo "wait-n=$?"

# `-n` answers with the *next* job to finish, so a job whose status has already
# been reported is not one it can answer with — even while the row is still in
# the table for a `jobs` to read one last time. An operand-less `wait` reports
# every job it actually waited for, but spares the one holding `$!` if that job
# had already finished, so the sleeps below decide the branch rather than the
# scheduler: the spared status is still `-n`-answerable…
( exit 3 ) &
sleep 0.3
wait; echo "spared-wait=$?"
wait -n; echo "spared-n=$?"
# …until a listing announces it, after which the job does not exist as far as
# `-n` is concerned — 127 for a bare one, `no such job` for one named by pid —
# while a *targeted* `wait PID` still replays the remembered status.
( exit 5 ) &
p5=$!
sleep 0.3
wait; echo "reported-wait=$?"
jobs >/dev/null
wait -n "$p5" 2>/dev/null; echo "reported-n-pid=$?"
wait -n; echo "reported-n=$?"
wait "$p5"; echo "reported-targeted=$?"
# A reported job does not shadow a live one: `-n` waits for the live one.
( exit 7 ) &
sleep 0.3
wait; echo "shadow-wait=$?"
jobs >/dev/null
( sleep 0.2; exit 9 ) &
wait -n; echo "shadow-n=$?"

# Traps still fire for a backgrounded subshell's own EXIT trap, and the output
# lands in the file rather than racing the parent's stdout.
( trap 'echo "bg-exit-trap" >> t.txt' EXIT; echo "bg-body" >> t.txt ) &
wait
cat t.txt
