# A handler with no `exit` of its own leaves the status untouched.
trap 'echo bye' EXIT
exit 2
