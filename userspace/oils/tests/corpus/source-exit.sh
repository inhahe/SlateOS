# `exit` in a sourced script unwinds the whole shell.
printf 'echo in-src
exit 6
' > inner.sh
. ./inner.sh
echo unreachable
