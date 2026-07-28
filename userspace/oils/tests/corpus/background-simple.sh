# A `&` job is a *forked child* in bash, so everything the command carries —
# its assignment prefixes, its redirections, the expansions in its words — is
# applied exactly once, inside the job. Nothing here inspects a pid, so the
# output is deterministic; every job is drained with `wait` before its effect is
# read.

echo "=== the word expansions run once, not once per lookup attempt"
# `true` is a builtin, so a shell that expands the words to find *that* out and
# then expands them again to run the command would append twice.
: > cnt
true $(printf x >> cnt; echo y) & wait
echo "builtin: $(wc -c < cnt)"
: > cnt
cat $(printf x >> cnt; echo /dev/null) & wait
echo "external: $(wc -c < cnt)"

echo "=== assignment prefixes reach the job's environment"
V=zz sh -c 'echo "V=$V"' & wait
V=zz env > job.out & wait
grep -c '^V=zz$' job.out
# …and do not leak back out of it.
echo "after: [${V-unset}]"

echo "=== redirections apply to the job"
sh -c 'echo out; echo err >&2' > job.out 2> job.err & wait
echo "out=[$(cat job.out)] err=[$(cat job.err)]"
nosuch_bg_cmd 2> job.err & wait
sed 's/^[^ ]*: //' job.err

echo "=== a name that resolves nowhere fails inside the job"
# bash forks first, so the diagnostic and the 127 belong to the child: the
# shell's own status is 0 and `wait` on the job reports 127.
nosuch_bg_cmd & echo "shell rc=$?"
wait $!; echo "job rc=$?"
# A pathname that does not exist reports the OS error, still from the child.
/nonexistent/nosuch_bg_cmd & wait $!; echo "path rc=$?"
# A name that resolves to something unexecutable is a 126, not a 127. (stderr
# dropped: the host OS words "cannot execute a directory" differently for bash's
# MSYS runtime — `Is a directory` — than for a native `CreateProcess`.)
mkdir -p adir
( ./adir & wait $!; echo "dir rc=$?" ) 2>/dev/null

echo "=== command_not_found_handle runs in the job, and its status is the job's"
command_not_found_handle() { echo "handled:$1:$2"; return 9; }
nosuch_bg_cmd arg & wait $!; echo "handler rc=$?"
unset -f command_not_found_handle

echo "=== builtins and functions background too"
f() { echo "fn:$1"; return 3; }
f one & wait $!; echo "fn rc=$?"
echo builtin-out & wait $!; echo "builtin rc=$?"
{ echo group; false; } & wait $!; echo "group rc=$?"
( ! true ) & wait $!; echo "subshell-negated rc=$?"
