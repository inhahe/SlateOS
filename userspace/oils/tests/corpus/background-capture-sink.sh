# How long a `$( … )` waits for a `&` job started inside it, and whose output
# it ends up with.
#
# bash collects a command substitution through a pipe and reads it to EOF, so
# the substitution finishes when the *last write end closes* — not when the body
# does. A job forked inside inherits a copy of that write end, so it both keeps
# the reader waiting and gets its output into the result, however deeply nested
# its starting point was. Equally, a job whose fd 1 no longer refers to the pipe
# — because its own redirection overwrote the descriptor, or because an
# enclosing scope had already rebound fd 1 — holds nothing, and the substitution
# returns at once.
#
# See known-issues.md TD-OILS-BG-SINK-OUTLIVES-SUBSHELL.

# Report whether a substitution waited for its job, without depending on how
# long it waited. `$SECONDS` ticks on the wall clock, not on elapsed time, so a
# substitution that took no time at all still reads as 1 whenever it happens to
# straddle a second boundary — hence a two-second job and a threshold of two.
t() {
  local start=$SECONDS out
  out=$(eval "$1")
  if [ $((SECONDS - start)) -ge 2 ]; then
    echo "  waited    [$out]  <- $1"
  else
    echo "  immediate [$out]  <- $1"
  fi
}

echo "=== a job holds the substitution's fd 1 open"
t 'echo hi &'
t 'sleep 2 &'

echo "=== ... however deeply nested its starting point"
t '( echo n & )'
t '( ( echo d & ) )'
t '( sleep 2 & )'
t '{ ( echo g & ); }'
# A function call is not a fork, so this one never depended on nesting.
t 'f() { echo fn & }; f'
# Nor is a pipeline stage — but its subshell is, and it ends before the job.
t 'echo x | { echo st & }'

echo "=== ... and its status is not the substitution's"
x=$(false &); echo "  status=$? x=[$x]"

echo "=== but a job whose own redirection takes fd 1 away holds nothing"
t 'sleep 2 > /dev/null &'
t 'sleep 2 >&2 &'
# Its output is not lost, just not the substitution's — read back at the end,
# once the seconds of sleeps below have left no doubt the job has run.
t 'echo elsewhere > elsewhere.txt &'

echo "=== nor does one started where fd 1 had already been rebound"
t '{ sleep 2 & } > /dev/null'
t '( sleep 2 & ) > /dev/null'
t '{ exec > exec.txt; sleep 2 & }'

echo "=== a dup made before the overwrite keeps a copy, and counts"
t 'sleep 2 2>&1 > /dev/null &'
t 'sleep 2 3>&1 > /dev/null &'
t '{ sleep 2 > /dev/null & } 2>&1'

echo "=== the same accounting decides whose output lands in the result"
echo "  [$( { echo one; echo two & } )]"
echo "  [$( ( echo sub & ) 2>/dev/null )]"
echo "  [$( echo direct > /dev/null & )]"
echo "  elsewhere=[$(cat elsewhere.txt)]"
