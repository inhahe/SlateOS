# A redirection on `eval` or `source` belongs to the commands they run. Neither
# builtin writes anything itself, so unlike `echo hi > f` there is nothing for
# the redirect to catch at the moment the builtin produces output — it has to
# stand for the whole body, the way a `{ …; } > f` group's does.

echo "=== stdout goes to the file for every command in the body"
eval 'echo one; echo two' > f; echo "f=[$(cat f)]"
eval 'echo three' >> f; echo "f=[$(cat f)]"
# The file wins over an enclosing capture: the bytes never reach the caller.
x=$(eval 'echo cap' > g); echo "x=[$x] g=[$(cat g)]"

echo "=== and stdin comes from the redirect, so there is something to read"
eval cat <<< "here-string"
eval 'read v; echo "v=$v"' <<< "read-me"

echo "=== the two output streams can be merged or kept apart"
eval 'echo out; echo err >&2' > h 2>&1; echo "h=[$(cat h)]"
eval 'echo out; echo err >&2' > i 2>j; echo "i=[$(cat i)] j=[$(cat j)]"
# A command the body itself redirects is not dragged along.
eval 'echo body > k; echo rest' > l; echo "k=[$(cat k)] l=[$(cat l)]"

echo "=== \`source\` is redirected the same way"
printf 'echo from-file\n' > s.sh
source s.sh > m; echo "m=[$(cat m)]"
printf 'read v; echo "got=$v"\n' > r.sh
. r.sh <<< "sourced-stdin"

echo "=== a redirect that cannot be established runs none of the body"
eval 'echo nope' > nodir/x; echo "rc=$?"
