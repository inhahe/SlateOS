# Redirection mechanics: fd duplication order, `exec` on the shell's own fds,
# `&>`/`>&`, append, here-strings, /dev/null, and restoring a saved fd.
echo out-then-err > /dev/null

# `2>&1 >file` duplicates the *current* stdout (the terminal) into fd 2 before
# stdout is redirected, so the two streams end up in different places. The
# reverse order sends both to the file. Capturing with $( ) makes the split
# observable: only what stays on stdout is captured.
f=order.txt
res=$( { echo to-out; echo to-err >&2; } 2>&1 >$f )
echo "captured=[$res] file=[$(cat $f)]"
res=$( { echo to-out; echo to-err >&2; } >$f 2>&1 )
echo "captured2=[$res] file2=[$(cat $f)]"

# &> and >& are both "stdout and stderr to the same place".
{ echo a; echo b >&2; } &> both.txt
echo "both=[$(cat both.txt)]"

# Append vs truncate.
echo one > app.txt
echo two >> app.txt
echo "append=[$(cat app.txt)]"
echo three > app.txt
echo "truncate=[$(cat app.txt)]"

# Saving and restoring the shell's own stdout through exec.
exec 9>&1
exec 1> viaexec.txt
echo hidden
exec 1>&9 9>&-
echo "viaexec=[$(cat viaexec.txt)]"

# Here-string, with and without expansion.
v='a b'
cat <<< "$v"
cat <<< $v
read -r x y <<< 'p q'
echo "hs-read x=$x y=$y"

# Reading from a numbered fd opened on a file.
printf 'l1\nl2\n' > lines.txt
exec 7< lines.txt
read -r first <&7
read -r second <&7
exec 7<&-
echo "fd7 first=$first second=$second"

# A redirection on a compound command applies to everything inside it.
for i in 1 2; do echo "loop$i"; done > loop.txt
echo "loop=[$(cat loop.txt)]"

# A failed redirection aborts the command and sets a non-zero status.
echo nope > nosuchdir/file 2>/dev/null
echo "badredir-status=$?"
