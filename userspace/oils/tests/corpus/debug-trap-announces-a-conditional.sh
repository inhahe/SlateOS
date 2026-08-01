# A `[[ … ]]` conditional announces itself to the DEBUG trap, under a printer
# that reads the expression back from the parse tree rather than from the
# source: the spacing is normalised (`[[ -n    x ]]` announces `[[ -n x ]]`),
# and a bare word is spelled out as the `-n` test it is (`[[ x ]]` announces
# `[[ -n x ]]`). Everything else is kept as written — the operator's own
# spelling, the quotes around a word, an expansion left unexpanded, and a
# redundant `( … )` group, which has to survive because dropping it would
# regroup the expression.
#
# `set -x` shows something else entirely: it traces each sub-test *as it is
# evaluated*, with the words expanded and requoted, so `[[ -n x && -z "" ]]`
# traces as two lines and `[[ -n $(echo hi) ]]` traces the substitution and
# then `[[ -n hi ]]`. The pair are worth reading side by side below.
#
# The announcement comes first, before anything is done about the command —
# the trace of it included, so a handler that itself traces has all of its own
# output out of the way before the `+ [[ … ]]` line appears. `(( … ))` is
# announced on the same terms.
#
# Under `shopt -s extdebug` a refused conditional is not evaluated at all — an
# expansion in it does not even run — and leaves 0 behind whatever it would
# have come out as.
e() { sed 's/^.*: line [0-9]*: //'; }
p() { echo "--- $1"; ( eval "$1"; echo "rc=$?" ) 2>&1 | e; }
b() { echo "B:<$BASH_COMMAND>"; }
c() { n=$((n+1)); echo "F$n:<$BASH_COMMAND>" >&2; [ "$n" != "$K" ]; }
r() { if [ "$BASH_COMMAND" = "$T" ]; then return 2; fi; return 0; }
s() { if [ -e o.txt ]; then cat o.txt; else echo "(no o.txt)"; fi; }

echo "=== the text a conditional announces"
trap b DEBUG
[[ -n x ]]
[[ -n    x ]]
[[ x ]]
[[ ! -n x ]]
[[ -n x && -z "" ]]
[[ -n x || -z y ]]
[[ ( -n x ) ]]
[[ ( -n x || -z y ) && -n z ]]
[[ a == a* ]]
[[ a != b ]]
[[ abc =~ ^a.c$ ]]
[[ 1 -lt 2 ]]
[[ -n $(echo hi) ]]
[[ "a b" == "a b" ]]
[[ 'a$b' == 'a$b' ]]
v=x
[[ -v v ]]
[[ -o errexit ]]
trap - DEBUG

echo "=== which set -x spells out differently"
set -x
[[ -n x ]]
[[ -n    x ]]
[[ x ]]
[[ -n x && -z "" ]]
[[ ( -n x ) ]]
[[ -n $(echo hi) ]]
set +x

echo "=== the announcement lands before the trace"
trap b DEBUG
set -x
[[ -n x ]]
[[ -n x && -z "" ]]
(( 1 + 1 ))
set +x
trap - DEBUG

echo "=== where a conditional gets announced from"
trap b DEBUG
if [[ -n x ]]; then echo yes; fi
w=q
while [[ -n "$w" ]]; do w=; done
[[ -n x ]] && echo t
[[ -z x ]] || echo f
trap - DEBUG

# A pipeline stage that is not a simple command is not announced by the shell
# that forks for it, and the fork itself has no DEBUG trap without `functrace`
# — so a `[[ … ]]` stage is announced by nobody, and only `cat` shows up.
echo "=== but not as a pipeline stage"
trap b DEBUG
[[ -n x ]] | cat
trap - DEBUG

echo "=== a refused conditional is not evaluated"
p 'shopt -s extdebug; n=0; K=99; trap c DEBUG; [[ -n x ]]; echo "r=$?"'
p 'shopt -s extdebug; n=0; K=1; trap c DEBUG; [[ -n x ]]; echo "r=$?"'
p 'shopt -s extdebug; n=0; K=1; trap c DEBUG; [[ -z x ]]; echo "r=$?"'
p 'shopt -s extdebug; n=0; K=99; rm -f o.txt; trap c DEBUG; [[ -n $(echo hi >o.txt) ]]; echo "r=$?"; s'
p 'shopt -s extdebug; n=0; K=1; rm -f o.txt; trap c DEBUG; [[ -n $(echo hi >o.txt) ]]; echo "r=$?"; s'

echo "=== nor is a refused one traced"
p 'shopt -s extdebug; set -x; n=0; K=1; trap c DEBUG; [[ -n x ]]; (( 1 + 1 )); set +x'
p 'shopt -s extdebug; set -x; n=0; K=2; trap c DEBUG; [[ -n x ]]; (( 1 + 1 )); set +x'

echo "=== and a 2 leaves the function the conditional was written in"
T='[[ -n x ]]'
f() { echo a; [[ -n x ]]; echo b; }
shopt -s extdebug
trap r DEBUG
f
echo "fr=$?"
trap - DEBUG
shopt -u extdebug
rm -f o.txt
echo "=== done"
