# Under `shopt -s extdebug` a DEBUG trap's exit status stops being a status and
# becomes an instruction about the command it was announcing. A non-zero one
# takes the command away — it never runs, and leaves 0 behind as if it had run
# and succeeded — which is how a debugger steps over a line without the script's
# cooperation. A 2 goes further and leaves the innermost function or sourced
# script as well, with status 2, which is how it steps out of one; where there
# is no such body to leave, at the top level of a script, the 2 is merely
# non-zero and only the one command goes. Without extdebug none of this happens:
# the status is just a status and the command runs whatever the handler thought
# of it.
#
# The trap actions here are *functions* so that the decision can be written
# plainly. A handler's own commands do not re-enter the trap, so `$BASH_COMMAND`
# still names the command being announced while one runs.
e() { sed 's/^.*: line [0-9]*: //'; }
p() { echo "--- $1"; ( eval "$1"; echo "rc=$?" ) 2>&1 | e; }
d() { [ "$BASH_COMMAND" != "$T" ]; }
r() { if [ "$BASH_COMMAND" = "$T" ]; then return 2; fi; return 0; }

echo "=== a non-zero status takes the command away, but only under extdebug"
T='echo two'
p 'trap d DEBUG; echo one; echo two; echo three'
p 'shopt -s extdebug; trap d DEBUG; echo one; echo two; echo three'
p 'set -T; trap d DEBUG; echo one; echo two; echo three'

echo "=== and the command leaves 0 behind"
p 'shopt -s extdebug; trap d DEBUG; false; echo two; echo "s=$?"'
p 'shopt -s extdebug; trap d DEBUG; (exit 5); echo two; echo "s=$?"'

echo "=== what it can take away"
T='x=1'
p 'shopt -s extdebug; trap d DEBUG; x=0; x=1; echo "[$x]"'
T='f'
p 'shopt -s extdebug; f() { echo in; }; trap d DEBUG; f; echo "r=$?"; echo after'
T='echo in'
p 'shopt -s extdebug; f() { echo in; }; trap d DEBUG; f; echo "r=$?"; echo after'
T='echo sub'
p 'shopt -s extdebug; trap d DEBUG; echo "x$(echo sub)"; echo after'
p 'shopt -s extdebug; trap d DEBUG; ( echo sub ); echo after'

echo "=== a 2 leaves the function it was announcing a command in"
T='echo two'
f() { echo one; echo two; echo three; }
g() { echo a; f; echo c; }
shopt -s extdebug
trap r DEBUG
f
echo "f r=$?"
g
echo "g r=$?"
trap - DEBUG
shopt -u extdebug

echo "=== with no body to leave it only takes the command"
T='echo mid'
shopt -s extdebug
trap r DEBUG
echo before
echo mid
echo "top r=$?"
trap - DEBUG
shopt -u extdebug

echo "=== and it leaves a sourced script too"
mkdir -p lib
printf 'echo s1\necho two\necho s3\n' > lib/s.sh
T='echo two'
p 'shopt -s extdebug; trap r DEBUG; . lib/s.sh; echo "r=$?"; echo after'
p 'shopt -s extdebug; trap d DEBUG; . lib/s.sh; echo "r=$?"; echo after'
p 'trap r DEBUG; . lib/s.sh; echo "r=$?"; echo after'
echo "=== done"
