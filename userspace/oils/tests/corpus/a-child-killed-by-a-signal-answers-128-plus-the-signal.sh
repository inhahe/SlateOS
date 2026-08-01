# A child that dies of a signal answers `128 + sig`, wherever it is waited for.
#
# The number is the only thing read here. The *notice* a shell prints about a
# signal death goes to stderr and carries a pid, so stderr is dropped rather
# than merged — what a shell says about the death is a separate question from
# what it answers for it.
#
# `$?` is eight bits wide, so 128 leaves room for signals 1..127 above it and
# there is no overlap to worry about: a child that exits 141 of its own accord
# and one killed by signal 13 are indistinguishable to the shell, and both
# readings below are correct.
p() { echo "--- $1"; ( eval "$1"'; echo "rc=$? ps=[${PIPESTATUS[*]}]"' ) 2>/dev/null; }

echo "=== a lone child"
p 'sh -c "kill -TERM \$\$"'
p 'sh -c "kill -KILL \$\$"'
p 'sh -c "kill -INT \$\$"'
p 'sh -c "kill -HUP \$\$"'
p 'sh -c "kill -USR1 \$\$"'
p 'sh -c "kill -ALRM \$\$"'

echo "=== …against an ordinary exit, including the codes that look like one"
p 'sh -c "exit 7"'
p 'sh -c "exit 0"'
p 'sh -c "exit 255"'
p 'sh -c "exit 128"'
p 'sh -c "exit 129"'
p 'sh -c "exit 141"'

echo "=== a stage of a pipeline, which keeps its own element"
p 'sh -c "kill -TERM \$\$" | cat'
p 'cat | sh -c "kill -TERM \$\$"'
p 'sh -c "kill -TERM \$\$" | sh -c "kill -INT \$\$"'
p 'set -o pipefail; true | sh -c "kill -TERM \$\$" | true'
# A reader that stops early is the everyday way a stage dies of a signal: the
# writer gets SIGPIPE, which is 13, so 141.
p 'seq 100000 | head -n 1 > /dev/null'

echo "=== a job, reaped by wait"
p 'sh -c "kill -TERM \$\$" & wait $!'

echo "=== and everywhere else a status is read"
p 'x=$(sh -c "kill -TERM \$\$"); echo "sub=$?"'
p 'if sh -c "kill -TERM \$\$"; then echo then; else echo "else=$?"; fi'
p 'sh -c "kill -TERM \$\$" || echo "or=$?"'
p '! sh -c "kill -TERM \$\$"'

echo "=== the test a script would write on it"
# `$?` has to be saved before it is read twice: the `[` writes one of its own.
q() { echo "--- $1"; ( eval "$1"; echo "s=$s" ) 2>/dev/null; }
q 'sh -c "kill -TERM \$\$"; s=$?; [ $s -gt 128 ] && echo "signal $(( s - 128 ))"'
q 'sh -c "exit 7"; s=$?; [ $s -gt 128 ] || echo "not a signal"'
