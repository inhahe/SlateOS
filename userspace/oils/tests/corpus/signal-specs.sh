# `trap` and `kill` read a signal spec the same way, because bash keeps one
# table covering the real signals and the pseudo ones alike. Only specs the two
# shells can agree on appear here: a signal's *number* is the platform's own
# business, and the two disagree from SIGBUS upward, so the numbers named below
# are the handful every Unix numbers alike.

echo "=== a number is answered with a name, and a name with a number"
kill -l 1 2 3 4 5 6 8 9 11 13 14 15
kill -l HUP SIGINT quit SIGKILL term
# The prefix belongs to the signal's own spelling, so it may be dropped…
kill -l SIGTERM TERM
# …but never added to a spec that never had one.
kill -l SIGEXIT; echo "rc=$?"

echo "=== signal zero is the EXIT the table starts with"
kill -l 0
kill -l EXIT exit
# `trap` reaches that same entry by either spelling.
(trap 'echo bye' 0; :)
(trap 'echo bye' EXIT; :)
trap 'echo x' SIGEXIT; echo "rc=$?"

echo "=== the pseudo signals have numbers too, above where the signals stop"
# Which numbers is again the platform's business, so only the answering is
# checked: a name the table knows is answered, one it does not is refused.
for s in DEBUG ERR RETURN; do kill -l "$s" > /dev/null; echo "$s rc=$?"; done
for s in SIGDEBUG NOSUCH ""; do kill -l "$s" > /dev/null; echo "[$s] rc=$?"; done

echo "=== a number is read the way any number is read"
kill -l " 9" "9 " "  15  "
kill -l +9 09
# A name is not: it has to be matched exactly.
kill -l " TERM"; echo "rc=$?"
# And a word that merely starts as a number is no number at all.
kill -l 0x9 9x; echo "rc=$?"
# `trap` reads one the same way, so the spacing does not reach the listing.
trap 'echo x' " 2"; trap -p; trap - 2

echo "=== the exit status a signal produces names it as well"
kill -l 129 143 137
# 128 itself is no such status, and nothing past the end of the table is either.
kill -l 128; echo "rc=$?"
kill -l 256 99; echo "rc=$?"
