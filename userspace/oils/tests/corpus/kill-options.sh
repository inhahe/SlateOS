# `kill` reads its options as a run rather than looking only at the first word,
# and what falls out of that is the subject here. Nothing is ever delivered:
# every pid named below is one no shell can reach, so none of this depends on
# what a signal actually does.

echo "=== -l may come anywhere in the run"
kill -9 -l 15
kill -s TERM -l 15
kill -L -l 15
kill -TERM -l 1

echo "=== the signal comes from the first spec, and later dashes are operands"
kill -TERM -9 999999; echo "rc=$?"
kill -l -9 15
kill -l -9 -15; echo "rc=$?"

echo "=== -s and -n take an attached spec, told apart by what follows"
# A name for -s and a number for -n — so `-sn9` is the spec `n9` while `-s9x`
# is the spec `s9x`, and `-n9x` is `9x` while `-nTERM` is `nTERM`.
kill -sTERM 999999; echo "rc=$?"
kill -n9 999999; echo "rc=$?"
kill -sn9 999999; echo "rc=$?"
kill -s9x 999999; echo "rc=$?"
kill -nTERM 999999; echo "rc=$?"
kill -n9x 999999; echo "rc=$?"
# Given alone, either takes the next word instead, and a name suits both.
kill -s TERM 999999; echo "rc=$?"
kill -n TERM 999999; echo "rc=$?"
kill -s; echo "rc=$?"
kill -n; echo "rc=$?"

echo "=== a spec is judged only once there is a signal to deliver"
kill -l -x 15
kill -s x -l 15
kill -x 999999; echo "rc=$?"
# …and the fault is the spec's, so it is named once however many targets follow.
kill -x 999999 999998; echo "rc=$?"

echo "=== -- ends the run, and -? asks for the usage"
kill -l -- 9
kill -- -l 9; echo "rc=$?"
kill -?; echo "rc=$?"
kill; echo "rc=$?"
# A signal with nothing to send it to is the usage error too, not a delivery.
kill -s TERM; echo "rc=$?"
