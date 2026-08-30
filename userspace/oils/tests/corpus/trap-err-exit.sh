# An `exit` inside an ERR handler terminates the shell, not just the handler.
trap 'echo "in ERR"; exit 7' ERR
false
echo "unreachable"
