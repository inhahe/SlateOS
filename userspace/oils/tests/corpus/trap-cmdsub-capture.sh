# A trap firing inside `$( … )` writes into the capture, because the handler
# runs in the substitution's subshell whose fd 1 is the pipe.
f() { echo body; }
set -T
trap 'echo RET-TRAP' RETURN
x=$(f)
echo "captured=[$x]"
trap - RETURN
trap 'echo DBG-TRAP' DEBUG
y=$(f)
trap - DEBUG
echo "captured2=[$y]"
trap 'echo ERR-TRAP' ERR
z=$(false)
trap - ERR
echo "captured3=[$z]"
