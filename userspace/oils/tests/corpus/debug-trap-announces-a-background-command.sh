# A `&` job is announced to the DEBUG trap by the shell that *starts* it, in
# the parent and before the job runs — never by the job itself. That matters
# because a job runs in a subshell, and a subshell without `functrace` has no
# DEBUG trap at all: without the parent-side firing a step-debugger would never
# see a background command.
#
# What is announced is exactly what a foreground pipeline announces: each stage
# that is a simple command, left to right. An `&&`/`||` list, a compound
# command and a `time`d pipeline announce nothing — those are handed to the job
# whole.
#
# The job's own subshell does not repeat the announcement. That is only visible
# under `set -T`, where the subshell inherits the trap: `f &` announces `f`
# once in the parent, and then — from inside, as the call is entered — the
# extra firing bash makes for every function call.
e() { sed 's/^.*: line [0-9]*: //'; }
p() { echo "--- $1"; ( eval "$1"; echo "rc=$?" ) 2>&1 | e; }
q() { echo "--- $1"; ( eval "$1"; echo "rc=$?" ) 2>&1 | e | grep -v -e '^real' -e '^user' -e '^sys' -e '^$'; }
b() { echo "B:<$BASH_COMMAND>"; }
c() { n=$((n+1)); echo "F$n:<$BASH_COMMAND>" >&2; [ "$n" != "$K" ]; }
r() { if [ "$BASH_COMMAND" = "$T" ]; then return 2; fi; return 0; }

echo "=== a job is announced in the parent, once per simple-command stage"
p 'trap b DEBUG; echo one >o.txt & wait; cat o.txt'
p 'trap b DEBUG; echo a | cat >o.txt & wait; cat o.txt'
p 'trap b DEBUG; echo a | tr a b | cat >o.txt & wait; cat o.txt'
p 'trap b DEBUG; x=1 echo one >o.txt & wait; cat o.txt'
p 'trap b DEBUG; ! false & wait'
p 'trap b DEBUG; echo a >o.txt & echo b >p.txt & wait; cat o.txt p.txt'

echo "=== a stage that is not a simple command is not announced"
p 'trap b DEBUG; { echo a; echo b; } | cat >o.txt & wait; cat o.txt'
p 'trap b DEBUG; { echo a >o.txt; } & wait; cat o.txt'
p 'trap b DEBUG; ( echo a >o.txt ) & wait; cat o.txt'
p 'trap b DEBUG; for i in a b; do echo "i$i"; done >o.txt & wait; cat o.txt'
p 'trap b DEBUG; if true; then echo t >o.txt; fi & wait; cat o.txt'
p 'trap b DEBUG; case x in x) echo c >o.txt ;; esac & wait; cat o.txt'
p 'trap b DEBUG; (( 1 + 1 )) & wait'

echo "=== nor is a list, nor a timed pipeline"
p 'trap b DEBUG; true && echo x >o.txt & wait; cat o.txt'
p 'trap b DEBUG; false || echo x >o.txt & wait; cat o.txt'
q 'trap b DEBUG; time echo one >o.txt & wait; cat o.txt'

echo "=== and the job does not announce it a second time"
p 'set -T; trap b DEBUG; echo one >o.txt & wait; cat o.txt'
p 'set -T; trap b DEBUG; echo a | cat >o.txt & wait; cat o.txt'
p 'set -T; f() { echo in >o.txt; }; trap b DEBUG; f & wait; cat o.txt'
p 'set -T; f() { echo in; }; trap b DEBUG; f | cat >o.txt & wait; cat o.txt'
# ... which is the same count a foreground pipeline gets.
p 'set -T; trap b DEBUG; echo a | cat >o.txt; cat o.txt'
p 'set -T; f() { echo in; }; trap b DEBUG; f | cat >o.txt; cat o.txt'

echo "=== refusing the announcement takes the job away"
s() { if [ -e o.txt ]; then cat o.txt; else echo "(no o.txt)"; fi; }
p 'shopt -s extdebug; n=0; K=99; rm -f o.txt; trap c DEBUG; echo one >o.txt & wait; echo "r=$?"; s'
p 'shopt -s extdebug; n=0; K=1; rm -f o.txt; trap c DEBUG; echo one >o.txt & wait; echo "r=$?"; s'

echo "=== and a 2 leaves the function the job was written in"
T='echo one'
g() { echo a; echo one >o.txt & wait; echo b; }
shopt -s extdebug
trap r DEBUG
g
echo "gr=$?"
trap - DEBUG
shopt -u extdebug
echo "=== done"
