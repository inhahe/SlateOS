# `x=$(. script)` captures the script's output.
printf 'echo in-src
' > inner.sh
x=$(. ./inner.sh)
echo "[$x]"
