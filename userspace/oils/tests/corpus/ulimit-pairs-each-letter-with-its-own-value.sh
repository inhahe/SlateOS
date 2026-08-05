# `ulimit` does not collect flags and then read one operand. Every limit letter
# carries its *own* optional value, taken from the rest of its own word or from
# the next word when that word does not begin with `-`. A letter is settled only
# when the next one arrives, or at the very end — which is what lets a trailing
# operand reach back across `--`, and what lets a later `-a` or a later bad
# option abandon a set that was already spelled out.
#
# Only `-c` is *set* here, and always through `-S`, so the hard limits stay
# where they started: lowering a hard limit is irreversible, and this file has
# to be replayable. `-a`'s listing is not compared — the set of resources a
# kernel offers is the host's business, not the shell's.

echo '--- a value belongs to the letter it follows'
ulimit -c
ulimit -S -c 9 -f
ulimit -c

echo '--- the value may be the rest of the letter own word'
ulimit -Sc7
ulimit -c

echo '--- so combined letters are not split: the `f` is `-c`s value'
ulimit -Scf -f
echo "rc=$?"
ulimit -c

echo '--- a word starting with `-` is an option, not a value'
ulimit -c -f

echo '--- the pending letter reaches across `--` for its value'
ulimit -S -c -- 6
ulimit -c

echo '--- and takes only the first leftover word; the rest are dropped'
ulimit -S -c 5 4
ulimit -c
ulimit -S -c 4 junk -f
echo "rc=$?"
ulimit -c

echo '--- with no letter at all the leftover is the file limits value'
ulimit -- abc
echo "rc=$?"

echo '--- a bare `-` is a leftover word, not an option'
ulimit -c -
echo "rc=$?"
ulimit -
echo "rc=$?"

echo '--- a later -a abandons the pending set'
ulimit -S -c 3 -a > /dev/null
echo "rc=$?"
ulimit -c

echo '--- as does a later bad option'
ulimit -S -c 3 -z
echo "rc=$?"
ulimit -c

echo '--- and a pair that fails ends the call where it stands'
ulimit -S -c abc -f
echo "rc=$?"

echo '--- repeats are applied in order, and the last one wins'
ulimit -S -c 3 -c 2
ulimit -c
ulimit -c -c

echo '--- -H and -S are read when the letter is settled, not where they stand'
ulimit -c -H
ulimit -H -c
ulimit -HS -c
ulimit -SH -c

echo '--- a value of no bytes is the number zero'
ulimit -c --
ulimit -Sc ''
ulimit -c

echo '--- `hard` and `unlimited` name limits rather than counting'
ulimit -Sc hard
ulimit -c
ulimit -Sc unlimited
ulimit -c

# Only the soft half is read back. Without -H or -S bash asks the kernel to
# move both, but whether the hard limit actually moves is the host's business —
# MSYS keeps the hard core limit `unlimited` however hard bash pushes, where
# Linux lowers it. That half is pinned against osh's own model in the unit test
# `each_ulimit_letter_takes_its_own_value` instead.
echo '--- without -H or -S a set moves both'
ulimit -c 0
ulimit -c
echo done
