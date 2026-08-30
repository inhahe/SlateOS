# A shell that forks carries its job table into the child, and the child can
# read it — but it can never wait for what is in it. Measured against bash 5.2.
#
#   * `$( … )` and a plain pipeline stage are forks and nothing more, so `jobs`
#     inside either lists what the shell itself would list, `+`/`-` markers and
#     all. That is the whole point: a listing is nearly always read through one
#     or the other;
#   * the copy is a snapshot. What the child does to it — waiting, disowning,
#     reporting — never reaches the shell that forked it, and the shell's own
#     later listing is unchanged;
#   * a job of the child's own is numbered *after* the ones it inherited;
#   * `execute_in_subshell` empties the table, and that is the same boundary
#     `$BASH_SUBSHELL` counts: a `( … )`, an `&` job, and a pipeline stage that
#     is a compound command or a function call all start with no jobs, while a
#     stage that is a plain command does not;
#   * a child cannot wait for its parent's children, and each `wait` says so its
#     own way: an operand-less one returns 0 without blocking, `wait -n` answers
#     0 and takes a row away, and a targeted `wait` hands back the -1 its failure
#     returned — `$?` is signed and prints it as written, though a *substitution*
#     of that same shell is an exit status, and eight bits wide, so 255;
#   * and asking is what settles it: the first `wait` of any kind writes the
#     whole inherited table off as finished, so a later `jobs` in that same child
#     shows `Done` for jobs that are still running perfectly well. A targeted
#     `wait` reports the row it named but does not sweep it — the sweep belongs
#     to a reap that never happened — so three in a row all answer.
#
# The jobs are `sleep`s long enough that none of them can end while the case
# runs: every `Done` below is one of those write-offs, not a job that finished.
# Their output goes to `/dev/null` for two reasons — a job that holds the
# collecting pipe of a `$( … )` keeps the substitution from finishing, and one
# that outlives the shell would hold the harness's own pipe open just as well.
#
# `$!` and the pids in a `jobs -l` differ per run and per shell, so both are
# folded away; the pid column is right-aligned in a fixed field, so folding it
# has to keep the width rather than just the digits.
sleep 30 >/dev/null 2>&1 &
sleep 31 >/dev/null 2>&1 &
sleep 32 >/dev/null 2>&1 &
bg=$!
p() { sed -e 's/^\(\[[0-9]*\][-+ ]\) *[0-9]*/\1 PID/' -e "s/\\b$bg\\b/BANG/g"; }

echo "=== a fork can read the table it was forked with"
echo "$( jobs )"
echo "$( jobs -l )" | p
echo "$( jobs -p )" | wc -l
jobs | wc -l

echo "=== and the shell that forked it is untouched by what the fork did"
echo "$( disown %1; jobs )"
jobs | wc -l

echo "=== a job of the child's own is numbered after the inherited ones"
echo "$( sleep 3 >/dev/null 2>&1 & jobs )"

echo "=== entering a subshell is what empties it"
echo "[$( ( jobs ) )]"
( jobs ) | wc -l
{ jobs; } | wc -l
f() { jobs; }
f | wc -l
{ jobs; } & wait $!
( echo "[$( jobs )]" )

echo "=== a child cannot wait for its parent's children"
echo "$( wait; echo "rc=$?" )"
echo "$( wait -n; echo "rc=$?" )"
echo "$( wait %1; echo "rc=$?" )"
echo "$( wait $bg; echo "rc=$?" )"
echo "$( wait %2; wait %2; wait %2; echo "rc=$?" )"
# …and the substitution's own status is that same -1 as an exit status.
x=$( wait %1 ); echo "sub=$?"

echo "=== asking once writes the whole table off"
echo "$( wait %1 >/dev/null; jobs )"
# An operand-less `wait` reports every job it wrote off except the one holding
# `$!` — which by now is the `{ jobs; }` above, long gone, so none is spared.
echo "[$( wait; jobs )]"

echo "=== the shell itself still has all three, still running"
jobs
kill %1 %2 %3
