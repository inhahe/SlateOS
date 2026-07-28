# STDIN: fed-to-source
# A sourced script reads the shell's stdin.
printf 'read v
echo "[$v]"
' > inner.sh
. ./inner.sh
