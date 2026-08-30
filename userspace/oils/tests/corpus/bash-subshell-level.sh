# `$BASH_SUBSHELL` counts fewer forks than the shell actually makes.
#
# bash forks for every stage of a pipeline, but the counter lives in the code
# that *enters* a subshell, and a stage only goes through it when the stage
# re-enters the evaluator: a compound command, a function call, or one of the
# builtins that runs code (`eval`, `.`, `source`). A plain simple command's
# words were expanded before that point, so nothing it can print has seen the
# increment — and it reports the level it was forked from.
#
# The other forks — `( … )`, a command substitution, an `&` job, a process
# substitution — all count, and they nest.

echo "=== a simple command as a pipeline stage does not count"
echo "[$BASH_SUBSHELL]" | cat
printf '[%s]\n' "$BASH_SUBSHELL" | cat
q=1 echo "[$BASH_SUBSHELL]" 2>/dev/null | cat
! echo "[$BASH_SUBSHELL]" | cat
: | echo "[$BASH_SUBSHELL]" | cat
echo "[$BASH_SUBSHELL]" | sed 's/^/mid /'

echo "=== a compound stage does"
{ echo "[$BASH_SUBSHELL]"; } | cat
if :; then echo "[$BASH_SUBSHELL]"; fi | cat
while :; do echo "[$BASH_SUBSHELL]"; break; done | cat
for i in 1; do echo "[$BASH_SUBSHELL]"; done | cat
case x in x) echo "[$BASH_SUBSHELL]";; esac | cat

echo "=== a subshell stage is one fork, counted once"
( echo "[$BASH_SUBSHELL]" ) | cat
( ( echo "[$BASH_SUBSHELL]" ) ) | cat
( echo "[$BASH_SUBSHELL]" | cat )

echo "=== so does a stage that re-enters the shell"
f() { echo "[$BASH_SUBSHELL]"; }
f | cat
f
eval 'echo "[$BASH_SUBSHELL]"' | cat
eval 'echo "[$BASH_SUBSHELL]"'
command eval 'echo "[$BASH_SUBSHELL]"' | cat
builtin eval 'echo "[$BASH_SUBSHELL]"' | cat
echo 'echo "[$BASH_SUBSHELL]"' > src.sh
. ./src.sh | cat
source ./src.sh | cat
. ./src.sh
rm -f src.sh

echo "=== only the outermost command of a stage may claim the fork"
eval 'eval "echo \"[\$BASH_SUBSHELL]\""' | cat
eval 'f' | cat
{ eval 'echo "[$BASH_SUBSHELL]"'; } | cat
( eval 'echo "[$BASH_SUBSHELL]"' ) | cat
( f | cat )
# The words of the `eval` were expanded before any of that.
eval "echo \"[$BASH_SUBSHELL]\"" | cat

echo "=== the forks that always count"
( echo "[$BASH_SUBSHELL]" )
echo "[$(echo "[$BASH_SUBSHELL]")]"
echo "[`echo "[$BASH_SUBSHELL]"`]"
echo "[$(echo "[$BASH_SUBSHELL]" | cat)]"
{ echo "[$BASH_SUBSHELL]"; } &
wait
echo "[$BASH_SUBSHELL]" &
wait
f &
wait

echo "=== a process substitution counts, and they nest"
cat < <(echo "[$BASH_SUBSHELL]")
cat < <(cat < <(echo "[$BASH_SUBSHELL]"))
cat < <(f)
cat < <(echo "[$BASH_SUBSHELL]") | cat
echo "[$BASH_SUBSHELL]"

echo "=== assigning the name moves the counter it is read from"
BASH_SUBSHELL=9
echo "[$BASH_SUBSHELL]"
echo "[$BASH_SUBSHELL]" | cat
{ echo "[$BASH_SUBSHELL]"; } | cat
( echo "[$BASH_SUBSHELL]" )
f | cat
