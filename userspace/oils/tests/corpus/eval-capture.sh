# `x=$(eval …)` captures, and `exit` inside an eval unwinds the shell.
x=$(eval echo hi)
echo "[$x]"
eval 'echo a; exit 3'
echo unreachable
