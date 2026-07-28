# The -C callback runs in the current shell, before each element is assigned,
# and an `exit` inside it terminates the shell with the array unassigned.
printf 'a
b
c
' > lines.txt
cb() { echo "cb $1 [$2] have=${#arr[@]}"; }
mapfile -t -C cb -c 1 arr < lines.txt
echo "n=${#arr[@]} first=${arr[0]}"
cb2() { echo "cb2 $1"; exit 5; }
mapfile -t -C cb2 -c 1 brr < lines.txt
echo unreachable
