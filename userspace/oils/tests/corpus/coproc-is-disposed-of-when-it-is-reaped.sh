# A coproc is disposed of the moment the shell notices its body has ended: the
# two descriptors are closed and `NAME`/`NAME_PID` are unset. That is why the
# usual idiom reads the coproc *before* anything else runs — a read that waits
# until after the next command boundary finds nothing left to read from.
#
# Only the coproc the shell is tracking is disposed of this way. One that a
# later `coproc` displaced keeps its variables for good, which is the other
# half of the warning `execute_coproc` prints.
#
# Stderr is collected and replayed, pid masked, at the end: see
# `coproc-second-warns-about-the-first.sh`.
exec 3>&2 2>err

echo "=== waiting for it is enough to lose it"
coproc P { read -r x; }
echo "before: set=${P+y} pid=${P_PID:+y}"
echo p >&"${P[1]}"
wait "$P_PID"; echo "waitP=$?"
echo "after:  set=${P+y} pid=${P_PID:+y}"

echo "=== so is letting it finish and running anything at all"
coproc Q { echo qq; }
sleep 1
true
echo "after:  set=${Q+y} pid=${Q_PID:+y}"
wait

echo "=== the descriptors are closed too, not just forgotten"
coproc R { echo rr; }
# Kept by hand, because disposal takes `R` itself away.
rfd=${R[0]} wfd=${R[1]}
sleep 1
true
read -r line <&"$rfd"; echo "read: rc=$? line=[$line]"
echo x >&"$wfd"; echo "write: rc=$?"
wait

echo "=== reading one that has not been disposed of"
coproc S { echo ss; }
read -r line <&"${S[0]}"; echo "rc=$? line=[$line]"
wait

echo "=== a displaced coproc keeps everything"
coproc T { read -r x; }
coproc U { read -r x; }
echo t >&"${T[1]}"; echo u >&"${U[1]}"
wait
echo "T: set=${T+y} pid=${T_PID:+y}"
echo "U: set=${U+y} pid=${U_PID:+y}"

exec 2>&3 3>&-
echo "=== what went to stderr"
sed 's/\[[0-9]*:/[PID:/' err
