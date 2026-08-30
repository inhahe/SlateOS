# `declare -n NAME` with no value judges the value the name already holds.
#
# A nameref's value *is* the name it refers to, so the valueless form is not a
# mere attribute tag: it declares that whatever is stored there already is a
# variable name, and bash checks it on the spot. `q=0; declare -n q` is refused
# with the same wording the assignment form uses, and refused *whole* — the
# operand is abandoned before any attribute is applied, so `declare -rn q`
# leaves a plain `declare -- q="0"` with neither letter on it.
#
# Two rules of the assignment form do not carry over:
#
#   * a self-reference is fine. `q=q; declare -n q` is accepted at global scope,
#     where `declare -n q=q` is an error — this command supplied no value, so
#     there is no circle it could have written;
#   * an empty value takes the nameref-specific wording (`` `': invalid variable
#     name for name reference ``) rather than the generic "not a valid
#     identifier" an empty assignment gets.
#
# An array cannot be a nameref whatever its elements say, and says so in its own
# words. A name with *no* binding holds nothing to judge and is simply tagged —
# including one a `local` in this very command has only just shadowed, which is
# why `q=0; f() { declare -n q; }` succeeds where the same text outside the
# function does not.
#
# Dynamic variables are judged by the value their function computes, so the
# refusal quotes a number that differs between two runs; it is masked, and so is
# the `SECONDS` the listing reports. The mask spells no double quote and no
# backslash of its own — osh cannot yet hand either to an external command on
# the Windows host it is developed on, see known-issues TD-OILS-WIN-ARG-QUOTING.
m() { sed -E -e 's/[0-9]+.: invalid/N: invalid/' -e 's/=.[0-9.]+.$/=Q/'; }

echo "=== the stored value is judged as if it had just been assigned"
( q=0;     declare -n q; echo "rc=$?"; declare -p q ) 2>&1
( q=a-b;   declare -n q; echo "rc=$?"; declare -p q ) 2>&1
( q=;      declare -n q; echo "rc=$?"; declare -p q ) 2>&1
( q=' ';   declare -n q; echo "rc=$?"; declare -p q ) 2>&1
( q='a b'; declare -n q; echo "rc=$?"; declare -p q ) 2>&1
# …and a value that can name a variable is accepted, subscript and all.
( q=zz;      declare -n q; echo "rc=$?"; declare -p q ) 2>&1
( q='a[1]';  declare -n q; echo "rc=$?"; declare -p q ) 2>&1
( q='m[$k]'; declare -n q; echo "rc=$?"; declare -p q ) 2>&1
# A name with nothing in it has nothing to judge.
( declare -n q; echo "rc=$?"; declare -p q ) 2>&1
( declare q; declare -n q; echo "rc=$?"; declare -p q ) 2>&1
# An existing nameref is re-judged by the name it already refers to.
( declare -n q=zz; declare -n q; echo "rc=$?"; declare -p q ) 2>&1

echo "=== a self-reference is allowed here, unlike in the assignment form"
( q=q; declare -n q; echo "rc=$?"; declare -p q ) 2>&1
( q=q; declare -n q=q; echo "rc=$?"; declare -p q ) 2>&1

echo "=== an array cannot be one, whatever it holds"
( declare -a q=(zz); declare -n q; echo "rc=$?"; declare -p q ) 2>&1
( declare -A q=([k]=zz); declare -n q; echo "rc=$?"; declare -p q ) 2>&1
( declare -a q; declare -n q; echo "rc=$?"; declare -p q ) 2>&1

echo "=== the refusal abandons the whole operand"
( q=0; declare -rn q;  echo "rc=$?"; declare -p q ) 2>&1
( q=0; declare -nu q;  echo "rc=$?"; declare -p q ) 2>&1
( q=0; declare -nix q; echo "rc=$?"; declare -p q ) 2>&1
# Later operands are still processed, in order.
( q=0; declare -n q z=9; echo "rc=$?"; declare -p q z ) 2>&1
( q=0; declare -n q q;   echo "rc=$?" ) 2>&1
# `+n` only removes the attribute, so it judges nothing.
( q=0; declare +n q; echo "rc=$?"; declare -p q ) 2>&1

echo "=== a local shadows the value before it can be judged"
( q=0; f() { declare -n q; echo "rc=$?"; declare -p q; }; f; declare -p q ) 2>&1
( q=0; f() { local -n q;   echo "rc=$?"; declare -p q; }; f; declare -p q ) 2>&1
# …but `-g` names the global, which still holds the value it held.
( q=0; f() { declare -gn q; echo "rc=$?"; }; f; declare -p q ) 2>&1
# A local that does hold something is judged like any other binding.
( f() { local q=0; declare -n q; echo "rc=$?"; declare -p q; }; f ) 2>&1

echo "=== the builtin names itself in the diagnostic"
( q=0; typeset -n q ) 2>&1
( q=0; f() { local -n q=1; }; f ) 2>&1

echo "=== dynamic variables are judged by the value they compute"
( declare -n SECONDS; echo "rc=$?"; declare -p SECONDS ) 2>&1 | m
( declare -n PPID; echo "rc=$?" ) 2>&1 | m
( declare -n BASH_SOURCE; echo "rc=$?" ) 2>&1
( declare -n BASH_LINENO; echo "rc=$?" ) 2>&1
# One that was unset first is an ordinary, empty name — so nothing is judged.
( unset SECONDS; declare -n SECONDS; echo "rc=$?"; declare -p SECONDS ) 2>&1
