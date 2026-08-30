# The empty string is a name like any other malformed one, and the declaration
# builtins refuse it rather than passing over it: `declare ""`, `local ""` and
# `typeset ""` all report `` `': not a valid identifier `` with status 1. So
# does an operand that is only a value — `declare '=5'` has an empty name too,
# and is quoted back whole as `` `=5' ``.
#
# One refused operand never stops the ones beside it: every bad name is
# reported in the order written, every good one is still bound, and the command
# reports the failure at the end.
#
# `-n` is the one flag that adds a line: the wholly empty operand — and only
# that one, not `-n '=x'` or `-n '='` — is prefixed with
# `warning: : circular name reference`, which reads as the self-reference check
# a valueless `-n` makes finding the empty name equal to the empty value an
# unbound name reads back as.

e() { sed -e 's/^.*: line [0-9]*: /SH: /' -e 's/^/    /'; }

echo "== 1. every spelling refuses it"
for cmd in 'declare ""' 'declare -- ""' 'declare -a ""' 'declare -A ""' \
           'declare -i ""' 'declare -r ""' 'declare -x ""' 'declare -g ""' \
           'declare -l ""' 'declare -u ""' 'declare -t ""' 'typeset ""'; do
  { eval "$cmd"; echo "  $cmd rc=$?"; } 2>&1 | e
done
f() { local ""; echo "  local \"\" rc=$?"; }
f 2>&1 | e
g() { typeset ""; echo "  typeset \"\" rc=$?"; }
g 2>&1 | e

echo "== 2. an operand that is only a value"
for cmd in "declare '=5'" "declare '='" "declare ' '" "declare ' =5'"; do
  { eval "$cmd"; echo "  $cmd rc=$?"; } 2>&1 | e
done
h() { local '=5'; echo "  local '=5' rc=$?"; }
h 2>&1 | e

echo "== 3. the good operands beside it are still bound"
{ declare "" v=1; echo "  rc=$?"; declare -p v; } 2>&1 | e
k() { local "" w=W; echo "  rc=$? w=$w"; }
k 2>&1 | e

echo "== 4. each bad operand speaks, in the order written"
{ declare "" 1x ""; echo "  rc=$?"; } 2>&1 | e
{ declare 1x "" 2y; echo "  rc=$?"; } 2>&1 | e

echo "== 5. -n prefixes a circular-reference warning"
{ declare -n ""; echo "  rc=$?"; } 2>&1 | e
{ declare -nr ""; echo "  rc=$?"; } 2>&1 | e
{ declare -n "" 1x; echo "  rc=$?"; } 2>&1 | e
{ declare -n 1x ""; echo "  rc=$?"; } 2>&1 | e
{ declare -n '=x'; echo "  rc=$?"; } 2>&1 | e
{ declare -n '='; echo "  rc=$?"; } 2>&1 | e
n() { local -n ""; echo "  rc=$?"; }
n 2>&1 | e

echo "== 6. the listing forms name it differently"
{ declare -p ""; echo "  rc=$?"; } 2>&1 | e
{ declare -f ""; echo "  rc=$?"; } 2>&1 | e
{ declare -F ""; echo "  rc=$?"; } 2>&1 | e
p() { local -p ""; echo "  rc=$?"; }
p 2>&1 | e

echo "== 7. the neighbours that already refused it"
{ export ""; echo "  rc=$?"; } 2>&1 | e
{ readonly ""; echo "  rc=$?"; } 2>&1 | e
{ unset ""; echo "  rc=$?"; } 2>&1 | e

echo "== 8. nothing was bound by any of it"
declare -p "" 2>/dev/null || echo "  still not found"
