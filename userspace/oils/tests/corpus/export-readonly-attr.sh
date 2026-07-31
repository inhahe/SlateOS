# A refused `export NAME=value` still exports the name.
#
# `export` and `readonly` are assignments as well as attribute-setters, and the
# two halves fail independently. Against a readonly target the *store* is
# refused — the diagnostic is the plain `NAME: readonly variable` a bare
# `NAME=value` gives, and the status is 1 — but the attribute is applied anyway,
# because bash marks the name before it tries the value:
#
#   readonly q=1; export q=2   →   error, status 1, and `declare -rx q="1"`
#
# `export -n q=2` takes the attribute off by the same route, so the refusal is
# no obstacle in either direction. `declare -x q=2` is *not* the same thing: it
# reports the failure with the builtin's name in front and applies nothing.
#
# The one thing that changes the diagnostic is `-a`/`-A`, which sends bash
# through the declaration machinery and so tags the message with the builtin's
# name (`export: q: readonly variable`). No other flag does: `-n`, `-p` and `--`
# all keep the bare form. `readonly` splits the same way, though for it the
# attribute is already there and only the message is observable.

echo "=== the value is refused and the attribute is applied"
( readonly q=1; export q=2; echo "rc=$?"; declare -p q ) 2>&1
( readonly q=1; export q+=2; echo "rc=$?"; declare -p q ) 2>&1
# …and taken off again, from a name that already had it.
( q=1; export q; readonly q; export -n q=2; echo "rc=$?"; declare -p q ) 2>&1
# A valueless operand asks for no assignment, so nothing is refused at all.
( readonly q=1; export q; echo "rc=$?"; declare -p q ) 2>&1
# A later operand of the same command is unaffected.
( readonly q=1; export q=2 z=3; echo "rc=$?"; declare -p q z ) 2>&1

echo "=== -a/-A tag the diagnostic with the builtin's name; nothing else does"
for f in '' -n -p -- -a -A -pa -na; do
  printf '[%s] ' "$f"
  ( readonly q=1; export $f q=2 ) 2>&1 | sed 's/^.*: line [0-9]*: //'
done
# The array flag does not make the operand an array, either — the array is
# created by the assignment, and there was none.
( readonly q=1; export -a q=2; echo "rc=$?"; declare -p q ) 2>&1

echo "=== readonly splits the same way"
( readonly q=1; readonly q=2; echo "rc=$?"; declare -p q ) 2>&1
for f in '' -p -- -a -A; do
  printf '[%s] ' "$f"
  ( readonly q=1; readonly $f q=2 ) 2>&1 | sed 's/^.*: line [0-9]*: //'
done

echo "=== declare is the odd one out: it names itself, and applies nothing"
( readonly q=1; declare -x q=2; echo "rc=$?"; declare -p q ) 2>&1
( readonly q=1; declare -u q=2; echo "rc=$?"; declare -p q ) 2>&1
# A valueless `declare -u` asks for no assignment, so its attribute lands.
( readonly q=1; declare -u q; echo "rc=$?"; declare -p q ) 2>&1
( readonly q=1; f() { local -x q=2; echo "rc=$?"; }; f; declare -p q ) 2>&1
