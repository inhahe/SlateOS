# A declaration builtin reads its flags with getopt, which stops at the first
# non-option word. Everything from there on is an *operand*, so a flag-shaped
# word written behind a name is not a flag at all — it is a name, and a name
# that no identifier could spell.
#
# The refusals are grouped through `e` rather than redirected per command,
# because osh emits some of them before the command's own redirections are in
# place (TD-OILS-DECL-DIAGNOSTIC-ESCAPES-REDIRECTION).
e() { sed 's/^.*: line [0-9]*: //'; }
p() { echo -n "$1 -> "; declare -p "$2" 2>&1; }

echo "=== a scalar operand ends the flags"
{ declare k1=1 -p; echo "rc=$?"; } 2>&1 | e
{ declare k2=1 -x; p 'k2' k2; } 2>&1 | e

echo "=== so does a compound one, which is the same word to bash"
{ declare k3=(1) -x; echo "rc=$?"; p 'k3' k3; } 2>&1 | e
{ declare k4=(1) -a; echo "rc=$?"; p 'k4' k4; } 2>&1 | e
{ declare k5=(1) +x; echo "rc=$?"; } 2>&1 | e

echo "=== the loop continues, so every bad operand is named"
{ declare k6=(1) -x -y; echo "rc=$?"; } 2>&1 | e
# …and a good operand behind a bad one still binds — without the letters of
# the word that was refused.
{ declare k7=(1) -x k8=2; echo "rc=$?"; p 'k7' k7; p 'k8' k8; } 2>&1 | e

echo "=== a real leading flag still reaches all of them"
{ declare -i k9=(2+3) -x k10=4+5; echo "rc=$?"; p 'k9' k9; p 'k10' k10; } 2>&1 | e

echo "=== -- and a lone dash are operands there too"
{ declare k11=(1) --; echo "rc=$?"; p 'k11' k11; } 2>&1 | e
{ declare k12=(1) -- k13=2; echo "rc=$?"; p 'k13' k13; } 2>&1 | e
{ declare k14=(1) -; echo "rc=$?"; } 2>&1 | e

echo "=== a scalar operand written first stops it just as well"
{ declare k15=1 k16=(2) -x; echo "rc=$?"; p 'k15' k15; p 'k16' k16; } 2>&1 | e

echo "=== -p behind an operand does not print"
{ declare k17=(1) -p; echo "rc=$?"; p 'k17' k17; } 2>&1 | e

echo "=== every builtin names itself"
{ readonly k18=(1) -x; echo "rc=$?"; p 'k18' k18; } 2>&1 | e
{ export k19=(1) -x; echo "rc=$?"; p 'k19' k19; } 2>&1 | e
{ typeset k20=(1) -x; echo "rc=$?"; } 2>&1 | e
{ f() { local k21=(1) -x; echo "rc=$?"; }; f; } 2>&1 | e

echo "=== but a flag ahead of every operand is still a flag"
{ declare -x k22=(1) k23=2; echo "rc=$?"; p 'k22' k22; p 'k23' k23; } 2>&1 | e

echo "=== done"
