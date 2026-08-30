# An `exit N` inside the EXIT trap replaces the shell's exit status.
trap 'echo bye; exit 9' EXIT
exit 2
