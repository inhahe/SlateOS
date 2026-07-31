# `set -x` and pipelines.
#
# bash traces a pipeline's stages *as it expands them*, left to right, before
# any of them has run — so the trace comes out in pipeline order even though the
# stages then execute concurrently and their output can interleave any which
# way. Each stage traces exactly as a lone simple command would: one line per
# temporary assignment, then the command word and its arguments.
#
# Only all-external pipelines are exercised here. A stage that is a builtin or a
# function is traced from a different execution path in osh, and that path does
# not yet order its stages the way bash does — see known-issues.md
# TD-OILS-XTRACE-PIPE-ORDER.
#
# Every stage is arranged to write nothing at all (input is /dev/null, and no
# stage can fail and complain), so the only bytes on the merged stream are the
# trace lines and their order is not a race.

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
