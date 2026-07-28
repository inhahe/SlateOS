# An `exit` inside a DEBUG handler unwinds before the announced command runs.
trap 'exit 5' DEBUG
echo a
echo b
