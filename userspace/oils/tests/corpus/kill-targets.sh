# What `kill` will take as a target. Nothing is signalled here either: every pid
# named is one no shell can reach, and the one real job is only asked whether it
# still exists.

echo "=== a target is a job spec or a pid, and a word that is neither says so"
kill abc; echo "rc=$?"
kill 9x; echo "rc=$?"
kill 1.5; echo "rc=$?"
kill "a b"; echo "rc=$?"
kill " "; echo "rc=$?"
# A number too large to be one is not a pid either.
kill 99999999999999999999999; echo "rc=$?"
# The empty word is the exception: with no spelling to fault, it is turned away
# as neither the one thing nor the other.
kill ""; echo "rc=$?"
# Each target is judged on its own, so a bad one does not stop the rest.
kill abc 999999; echo "rc=$?"

echo "=== a pid is read as a number, and answered for by its value"
kill " 999999"; echo "rc=$?"
kill +999999; echo "rc=$?"
kill 0999999; echo "rc=$?"

echo "=== a %spec is a job spec and nothing else"
kill %nosuch; echo "rc=$?"
kill %99; echo "rc=$?"
sleep 0.3 & kill -0 %1; echo "rc=$?"; wait
