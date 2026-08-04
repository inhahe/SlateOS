# The -C callback runs in the current shell, before each element is assigned,
# and an `exit` inside it terminates the shell with the array unassigned.
printf 'a
b
c
' > lines.txt
cb() { echo "cb $1 [$2] have=${#arr[@]}"; }
mapfile -t -C cb -c 1 arr < lines.txt
echo "n=${#arr[@]} first=${arr[0]}"
# The callback is text `mapfile` runs, not a source of its own, so it runs at
# the *`mapfile` command's* line — and leaves the shell's line where it found
# it rather than at the callback's own last one.
mapfile -t -C 'echo cb-lineno=$LINENO' -c 1 crr < lines.txt
echo "after-lineno=$LINENO"
mapfile -t -C 'echo "$LINENO"' -c 3 drr < lines.txt; echo "same-line-lineno=$LINENO"
cb2() { echo "cb2 $1"; exit 5; }
mapfile -t -C cb2 -c 1 brr < lines.txt
echo unreachable
