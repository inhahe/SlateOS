# A declaration builtin's leading words are read the way getopt reads them: a
# word that is *only* a sign ends option parsing and is an operand. Since no
# variable can be named `-` or `+`, the usual answer is a complaint about it —
# not the whole-table listing a `-p` with no operands would give.
#
# Listings are counted rather than shown wherever the table itself would be
# machine-specific; the count is what the bug changed.

e() { sed -e 's/^.*: line [0-9]*: /SH: /' -e 's/^/    /'; }
r() { echo "  rc=$? out=$(wc -l < out)"; e < err; sed -e 's/^/    /' out; }

v=V

echo "=== a lone - is an operand to -p, not a flag word"
declare -p - >out 2>err; r
declare -p - v >out 2>err; r
declare -p v - >out 2>err; r

echo "=== …and so is a lone +"
declare -p + >out 2>err; r
declare -p + v >out 2>err; r

echo "=== two of them are two operands"
declare -p - - >out 2>err; r

echo "=== -- ends the options, so the - behind it is still an operand"
declare -p -- - >out 2>err; r
declare -p -- v >out 2>err; r

echo "=== …but one written after an operand is an operand itself"
declare -p v -- >out 2>err; r

echo "=== the scan stopping also means no attribute filter is selected"
declare -pr - >out 2>err; r

echo "=== …and that the words behind the sign select nothing either"
declare - -p >out 2>err; r
declare -p - -r >out 2>err; r

echo "=== typeset spells the complaint with its own name"
typeset -p - >out 2>err; r

echo "=== export and readonly refuse it as a name"
export - >out 2>err; r
readonly - >out 2>err; r
readonly -p - >out 2>err; r

echo "=== local, in a function"
f1() { local w=W; local -p - >out 2>err; r; }
f1
f2() { local w=W; local -p v - >out 2>err; r; }
f2

echo "=== a p on a plus sign still selects listing mode"
f3() { local w=W; local +p >out 2>err; r; }
f3
f4() { local w=W; local +rp >out 2>err; r; }
f4
f5() { local +p w >out 2>err; r; }
f5

echo "=== …for declare too"
f6() { declare +p - >out 2>err; r; }
f6

echo "=== a -- ends the scan, so a -p behind one selects nothing"
declare -- -p >out 2>err; r
declare -r -- -p >out 2>err; r
declare -- -p v >out 2>err; r
typeset -- -p >out 2>err; r
f7() { local w=W; local -- -p >out 2>err; r; }
f7
f8() { local w=W; local -r -- -p >out 2>err; r; }
f8

rm -f out err
