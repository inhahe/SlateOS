# A coproc's body is a background job like any other: it sits in the job table
# under the pid published as `NAME_PID`, so `wait` reaps it and answers its
# status, and `jobs` lists it for as long as it runs.
#
# The two descriptors in `NAME` are deliberately not printed — bash hands out
# high numbers near the process's fd limit (63, 62, …) and osh allocates from
# 10 up, so the numbers are a property of the host, not of the shell. What is
# compared is that they *work*.
echo "=== a coproc that has already finished is still waitable"
coproc C { :; }
wait "$C_PID"; echo "wait=$?"

echo "=== and its status is its body's"
coproc E { exit 7; }
wait "$E_PID"; echo "waitE=$?"

echo "=== \$! names it, and jobs lists it while it runs"
coproc D { read x; echo "got $x"; }
echo "  bg-is-pid=$([ "$!" = "$D_PID" ] && echo yes || echo no)"
jobs
echo hello >&"${D[1]}"
read -r line <&"${D[0]}"; echo "line=[$line]"
wait "$D_PID"; echo "waitD=$?"

echo "=== …and nothing is left in the table once it is reaped"
jobs

echo "=== a bare wait waits for one too"
coproc G { exit 3; }
wait; echo "waitall=$?"
jobs

echo "=== an unnamed coproc is the same job, under COPROC"
coproc { exit 5; }
wait "$COPROC_PID"; echo "waitCOPROC=$?"
