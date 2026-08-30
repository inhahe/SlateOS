# `(( … ))` announces itself to the DEBUG trap, and so does each of the three
# arithmetic sections of a `for (( … ))` header — separately, in the order they
# are evaluated, the condition once more than the body runs.
#
# The text is the source between the parens, kept byte for byte, and the parens
# are set straight against it: `(( 1 + 1 ))` announces `(( 1 + 1 ))` only
# because that is how it was typed, while `((1+1))` announces `((1+1))`. This is
# a *different* printer from the one `set -x` uses, which pads the parens apart
# and so ends up doubling the spaces of an expression that already had them —
# the pair are worth reading side by side below. An omitted section of a
# `for (( … ))` is announced as the always-true `1` that stands in for it.
#
# Under `shopt -s extdebug` a refused expression is simply not evaluated: the
# `(( … ))` command leaves 0 behind whatever it would have come out as, and a
# refused section of a `for` header takes the value 0 — which for the condition
# is what ends the loop, and for the other two makes it a no-op.
e() { sed 's/^.*: line [0-9]*: //'; }
p() { echo "--- $1"; ( eval "$1"; echo "rc=$?" ) 2>&1 | e; }
b() { echo "B:<$BASH_COMMAND>"; }
c() { n=$((n+1)); echo "F$n:<$BASH_COMMAND>" >&2; [ "$n" != "$K" ]; }
r() { if [ "$BASH_COMMAND" = "$T" ]; then return 2; fi; return 0; }

echo "=== the text is the source between the parens"
trap b DEBUG
(( 1 + 1 ))
((1+1))
((   1+1   ))
(( x = 1 ))
for ((i=0;i<1;i++)); do :; done
for (( i = 0 ; i < 1 ; i++ )); do :; done
for ((;;)); do break; done
for ((i=2;i;i--)); do :; done
trap - DEBUG

echo "=== which set -x spells out differently"
set -x
(( 1 + 1 ))
((1+1))
((   1+1   ))
for ((i=0;i<1;i++)); do :; done
for (( i = 0 ; i < 1 ; i++ )); do :; done
for ((;;)); do break; done
set +x

echo "=== a refused expression is not evaluated"
p 'shopt -s extdebug; n=0; K=99; x=7; trap c DEBUG; (( x = 5 )); echo "r=$? x=$x"'
p 'shopt -s extdebug; n=0; K=1; x=7; trap c DEBUG; (( x = 5 )); echo "r=$? x=$x"'
p 'shopt -s extdebug; n=0; K=1; trap c DEBUG; (( 0 )); echo "r=$?"'

echo "=== a refused section of a for header takes the value 0"
p 'shopt -s extdebug; n=0; K=99; trap c DEBUG; for ((i=0;i<2;i++)); do echo "body $i"; done; echo "r=$?"'
p 'shopt -s extdebug; n=0; K=1; trap c DEBUG; i=9; for ((i=0;i<2;i++)); do echo "body $i"; done; echo "r=$? i=$i"'
p 'shopt -s extdebug; n=0; K=2; trap c DEBUG; for ((i=0;i<2;i++)); do echo "body $i"; done; echo "r=$?"'
p 'shopt -s extdebug; n=0; K=4; trap c DEBUG; for ((i=0;i<2;i++)); do echo "body $i"; done; echo "r=$?"'
p 'shopt -s extdebug; n=0; K=5; trap c DEBUG; for ((i=0;i<2;i++)); do echo "body $i"; done; echo "r=$?"'

echo "=== and a 2 leaves the function the expression was written in"
T='((1+1))'
f() { echo a; ((1+1)); echo b; }
shopt -s extdebug
trap r DEBUG
f
echo "fr=$?"
T='((i<1))'
g() { for ((i=0;i<1;i++)); do echo "body $i"; done; echo tail; }
g
echo "gr=$?"
trap - DEBUG
shopt -u extdebug
echo "=== done"
