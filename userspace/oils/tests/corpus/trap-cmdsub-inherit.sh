# Trace traps reach a subshell only under functrace/errtrace.
f() { echo body; }
trap 'echo RET' RETURN
x=$(f)
echo "no-functrace=[$x]"
trap 'echo E' ERR
y=$(false; echo body)
echo "no-errtrace=[$y]"
set -E
z=$(false; echo body)
echo "errtrace=[$z]"
