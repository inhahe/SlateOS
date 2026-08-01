# A coproc's body is a background job like any other: it sits in the job table
# under the pid published as `NAME_PID`, so `wait` reaps it and answers its
# status, and `jobs` lists it for as long as it runs.
#
# The two descriptors in `NAME` are deliberately not printed — bash hands out
# high numbers near the process's fd limit (63, 62, …) and osh allocates from
# 10 up, so the numbers are a property of the host, not of the shell. What is
# compared is that they *work*.
#
# `NAME_PID` does not outlive the body's reaping (see
# `coproc-is-disposed-of-when-it-is-reaped.sh`), and *when* the reaping happens
# is a race between the body and the next command — one a shell whose bodies are
# cheap threads rather than processes loses far more often. So nothing here is
# allowed to depend on it: a pid that is still needed after the body may have
# ended is copied into an ordinary variable first, and a body that has to still
# be running when a `wait` is reached is made to sleep rather than merely
# assumed to be slow.
echo "=== a coproc that has already finished is still waitable"
# A whole second is long enough that the body has ended and been reaped in
# either shell, so the `wait` below is answered from the remembered status of a
# job that is already gone rather than by blocking on a live one.
coproc C { exit 7; }
cpid=$C_PID
sleep 1
wait "$cpid"; echo "waitC=$?"

echo "=== and nothing of it is left in the table"
jobs

echo "=== \$! names it, and jobs lists it while it runs"
coproc D { read x; echo "got $x"; sleep 1; }
dpid=$D_PID
echo "  bg-is-pid=$([ "$!" = "$dpid" ] && echo yes || echo no)"
jobs
echo hello >&"${D[1]}"
read -r line <&"${D[0]}"; echo "line=[$line]"
wait "$dpid"; echo "waitD=$?"

echo "=== …and the table is empty again once it is reaped"
jobs

echo "=== a bare wait waits for one too"
coproc G { sleep 1; exit 3; }
wait; echo "waitall=$?"
jobs

echo "=== an unnamed coproc is the same job, under COPROC"
coproc { sleep 1; exit 5; }
wait "$COPROC_PID"; echo "waitCOPROC=$?"
