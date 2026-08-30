# A shell tracks exactly one coproc, so starting a second one while the first
# is still running throws the first away: `execute_coproc` says so — naming the
# coproc it is about to forget — and then starts the new one anyway.
#
# What decides is whether the tracked coproc has been *reaped*, not whether a
# coproc job exists: a body that has already exited is forgotten silently, with
# no `wait` needed.
#
# Stderr is collected in a file and replayed, filtered, at the end: the message
# carries a pid, which is a property of the host and not of the shell. Landing
# in `err` at all is what shows it was written to stderr; the replay goes to
# stdout so that the comparison sees it in a fixed place.
exec 3>&2 2>err

echo "=== a second one while the first still runs"
coproc P { read -r x; echo "P<$x>"; }
coproc Q { read -r y; echo "Q<$y>"; }
# Only the shell's one slot was overwritten: `P` and `Q` are ordinary variables
# and were never touched, so both coprocs are still readable and writable.
echo qq >&"${Q[1]}"; read -r l <&"${Q[0]}"; echo "q=[$l]"
echo pp >&"${P[1]}"; read -r l <&"${P[0]}"; echo "p=[$l]"
wait; echo "waitPQ=$?"

echo "=== each new one warns about the one it replaces"
coproc A { read -r x; }
coproc B { read -r x; }
coproc C { read -r x; }
echo a >&"${A[1]}"; echo b >&"${B[1]}"; echo c >&"${C[1]}"
wait; echo "waitABC=$?"

echo "=== a reaped coproc is nothing to warn about"
coproc D { read -r x; }
echo d >&"${D[1]}"
wait "$D_PID"; echo "waitD=$?"
coproc E { read -r x; }
echo e >&"${E[1]}"
wait; echo "waitE=$?"

echo "=== nor is one that exited on its own, unwaited"
coproc F { :; }
sleep 1
coproc G { :; }
wait; echo "waitFG=$?"

echo "=== an unnamed coproc is tracked under COPROC"
# The first one's descriptors go with the array they were published in, so it
# has to be able to finish on its own.
coproc { sleep 1; }
coproc { sleep 1; }
wait; echo "waitCOPROC=$?"

echo "=== a subshell has no coproc of its own to lose"
coproc S { read -r x; }
( coproc T { read -r x; }; echo t >&"${T[1]}"; wait; echo "sub=$?" )
echo s >&"${S[1]}"; wait; echo "waitS=$?"

exec 2>&3 3>&-
echo "=== what went to stderr"
sed 's/\[[0-9]*:/[PID:/' err
