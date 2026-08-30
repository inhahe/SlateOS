# printf's option scan is bash's `internal_getopt`, and that loops.
#
# So `printf -v a -v b` is not a misuse of a once-only option but two options,
# the way `-e -e` would be: the last one names the variable actually written,
# and the earlier ones are read, *checked*, and dropped without ever being
# created. Which means a bad name refuses the command even when it is the one
# about to be discarded.
#
# Looping also means an option word is still an option word after one has been
# seen: `--` ends the scan wherever it falls, and an invalid option after a
# `-v` is refused as readily as one before it. The one thing that does not
# loop is `-v`'s own argument — it takes the next word whatever that word
# looks like, so `printf -v -v x` reads `-v` as the *name*.
#
# And a bare `-` is not an option at all. It is the format.

r() { echo "--- $*"; "$@" 2>&1; echo "rc=$?"; }

echo "=== repeated -v: the last one wins, the earlier ones are dropped"
unset a1 a2; printf -v a1 -v a2 '%s' q; echo "rc=$? a1=${a1+set}[$a1] a2=${a2+set}[$a2]"
unset b1 b2 b3; printf -v b1 -v b2 -v b3 '%s' q; echo "rc=$? b1=${b1+set} b2=${b2+set} b3=${b3+set}[$b3]"
unset c1; printf -v c1 -v c1 '%s' q; echo "c1=[$c1]"
unset d1 d2; printf -vd1 -vd2 '%s' q; echo "d1=${d1+set} d2=[$d2]"

echo "=== every name is checked as it is read"
r printf -v 1bad -v ok '%s' q
r printf -v ok -v 1bad '%s' q
r printf -v 'a[0]' -v 'b[' '%s' q

echo "=== -- ends the scan wherever it is"
unset e1; printf -v e1 -- '%s' q; echo "rc=$? e1=[$e1]"
r printf -- -v e2 '%s' q
unset e3; printf -v e3 -- -- q; echo "rc=$? e3=[$e3]"

echo "=== an invalid option after a -v is still refused"
r printf -v x -z '%s' q
r printf -v x -vy -z '%s' q

echo "=== -v takes the next word whatever it looks like"
r printf -v -v x '%s' q
r printf -v -- '%s' q
r printf -v -z '%s' q

echo "=== a bare dash is the format"
r printf -
unset f1; printf -v f1 -; echo "rc=$? f1=[$f1]"

echo "=== a missing argument to -v"
r printf -v
r printf -v x -v

echo "=== the format is whatever is left"
unset g1; printf -v g1 -v g2 -vg3 '[%s]' a b; echo "rc=$? g1=${g1+set} g2=${g2+set} g3=[$g3]"

echo "=== a glued name that is empty"
r printf -v '' '%s' q
r printf -v0 '%s' q
