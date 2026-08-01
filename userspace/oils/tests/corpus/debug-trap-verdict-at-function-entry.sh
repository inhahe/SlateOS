# Under `extdebug` a function call is announced twice — once at the call site
# and once again on entry, from inside the new frame — and the two refusals
# mean different things.
#
# Refusing at the call site steps over the call: the frame is never pushed and
# `$?` is 0, as if the call had happened and succeeded. Refusing on entry steps
# over the *body*: the frame exists, so the RETURN trap goes with it, and what
# is left in `$?` is the handler's own status rather than 0.
#
# A third firing appears only when a RETURN trap is set: bash announces once
# more immediately before running the RETURN action. Refusing that one takes
# the action away and nothing else — the function has already run and its
# status stands.
e() { sed 's/^.*: line [0-9]*: //'; }
p() { echo "--- $1"; ( eval "$1"; echo "rc=$?" ) 2>&1 | e; }
# Announce every firing, and refuse the Kth.
c() { n=$((n+1)); echo "F$n:<$BASH_COMMAND>" >&2; [ "$n" != "$K" ]; }
# …the same, but refusing with a chosen status rather than 1.
v() { n=$((n+1)); echo "F$n:<$BASH_COMMAND>" >&2; [ "$n" != "$K" ] || return $V; }
R='n=0; shopt -s extdebug'
G='f() { echo body1; echo body2; }'
E='st=$?; trap - DEBUG; echo "st=$st"'

echo "=== which firing is which"
p "$R; $G; K=99; trap c DEBUG; f; $E"

echo "=== refusing each in turn"
for k in 1 2 3 4; do
  p "$R; $G; K=$k; trap c DEBUG; f; $E"
done

echo "=== with a RETURN trap present, which adds a firing before the action"
for k in 1 2 3 4 5; do
  p "$R; $G; K=$k; trap 'echo RET' RETURN; trap c DEBUG; f; trap - RETURN; $E"
done

echo "=== the entry refusal leaves the handler's own status"
for V in 1 3 7; do
  p "$R; $G; V=$V; K=2; trap v DEBUG; f; $E"
done

echo "=== …where the call-site refusal always leaves 0"
for V in 1 3 7; do
  p "$R; $G; V=$V; K=1; trap v DEBUG; f; $E"
done

echo "=== a nested call, so the frame a refusal unwinds has an outer one"
H='g() { f; echo after-f; }'
for V in 1 3; do
  p "$R; $G; $H; V=$V; K=3; trap v DEBUG; g; $E"
done

echo "=== a sourced script has no separate entry firing"
# `. script` is announced once, and the commands inside it are announced under
# functrace — which `extdebug` turns on. The firing before the RETURN action
# sees `$BASH_COMMAND` back at the `. script` word: bash restores it around the
# re-read, where a function body's last announcement stands.
printf 'echo s1\necho s2\n' > inc.sh
for k in 1 2 3 4 5; do
  p "$R; K=$k; trap 'echo RET' RETURN; trap c DEBUG; . ./inc.sh; trap - RETURN; $E"
done

echo "=== and the sourced body's own status survives a refused RETURN action"
printf 'echo s1\n(exit 5)\n' > inc2.sh
p "$R; K=99; trap 'echo RET' RETURN; trap c DEBUG; . ./inc2.sh; trap - RETURN; $E"
p "$R; K=4;  trap 'echo RET' RETURN; trap c DEBUG; . ./inc2.sh; trap - RETURN; $E"
