# `set -x` and pipelines.
#
# bash traces a pipeline's stages *as it expands them*, left to right, before
# any of them has run — so the trace comes out in pipeline order even though the
# stages then execute concurrently and their output can interleave any which
# way. Each stage traces exactly as a lone simple command would: one line per
# temporary assignment, then the command word and its arguments.
#
# Both of osh's pipeline executors are exercised: the all-external one (real OS
# pipes and child processes) and the threaded one (any stage that is a builtin, a
# function or a compound command). The two arrive at the order differently — the
# first expands its stages in order before spawning any, the second hands each
# stage a "you may begin" signal from its predecessor.
#
# Every stage is arranged to write nothing to stdout (input is /dev/null, and no
# stage can fail and complain), so the only bytes on the merged stream are the
# trace lines.
#
# What is ordered is each stage's *start* — its first trace line. A stage that
# traces more than once (a function body, a loop) races with the stages
# downstream of it for everything after that, in bash as much as here: a stage
# cannot be made to finish before the next one begins without deadlocking on a
# full pipe. So the pipelines below that trace more than one line in a stage are
# arranged so that no interleaving is observable — either the extra lines are
# identical to the next stage's, or the multi-line stage is the last one.

echo "=== stages trace in pipeline order"
( set -x; cat /dev/null | cat ) 2>&1
( set -x; cat /dev/null | cat | cat ) 2>&1

echo "=== a temporary assignment traces on its own line, before its command"
( set -x; A=1 cat /dev/null | B=2 cat ) 2>&1
( set -x; A=1 B=2 cat /dev/null | cat ) 2>&1

echo "=== arguments are quoted as anywhere else in the trace"
( set -x; cat -- /dev/null | cat -u ) 2>&1
( set -x; cat /dev/null | grep -e 'a b' ) 2>&1

echo "=== PS4 applies to every stage, and a pipeline is not an expansion"
( PS4='T '; set -x; cat /dev/null | cat ) 2>&1
( PS4=; set -x; cat /dev/null | cat ) 2>&1
( set -x; v=$(cat /dev/null | cat) ) 2>&1

echo "=== a builtin stage puts the pipeline on the threaded executor"
( set -x; true | cat ) 2>&1
( set -x; cat /dev/null | true ) 2>&1
( set -x; true | true ) 2>&1
( set -x; true | cat | true ) 2>&1
( set -x; A=1 true | B=2 true ) 2>&1

echo "=== function and compound stages trace their bodies, still in order"
# `f | true`, not `f | cat`: the body's `+ true` and the next stage's trace are
# concurrent, so they are spelled the same to keep the bytes deterministic.
( set -x; f() { true; }; f | true ) 2>&1
( set -x; f() { true; }; cat /dev/null | f ) 2>&1
( set -x; { true; } | { true; } ) 2>&1
( set -x; ( true ) | ( true ) ) 2>&1
( set -x; for i in 1 2; do true; done | true ) 2>&1

echo "=== a stage that runs no command at all does not hold up the next"
( set -x; x=1 | true ) 2>&1
( set -x; true | x=1 ) 2>&1
