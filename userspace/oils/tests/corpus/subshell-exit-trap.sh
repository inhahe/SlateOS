# The subshell's EXIT handler sees the body's status and can replace it.
( trap 'echo saw=$?' EXIT; exit 2 )
echo "seen=$?"
( trap 'exit 9' EXIT; exit 2 )
echo "replaced=$?"
