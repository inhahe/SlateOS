# The history *file*: `history -a/-n/-r/-w`, `HISTFILE`, and the truncation
# `HISTFILESIZE` performs on assignment.
#
# `HISTFILE` is deliberately set to a file in the case's own directory
# throughout: with it unset these options fall back to `$HOME/.history`, and a
# corpus case has no business writing there.
#
# See known-issues.md TD-OILS-MISSING-INTERACTIVE-BUILTINS.

set -o history

echo "=== -w writes the whole list, one entry per line"
history -c
: one
: two
history -w saved.txt
echo "  rc=$?"
cat saved.txt

echo "=== -r appends the file's lines to the list"
history -c
history -r saved.txt
history

echo "=== ... and appends them again on a second read"
history -r saved.txt
history | tail -3

echo "=== an empty line in the file is not an entry; a blank one is"
printf '%s\n' 'x1' '' '  ' 'x2' > blanks.txt
history -c
history -r blanks.txt
history

# A `-n` whose starting point is already at or past the end of the file is
# deliberately not exercised: readline counts the skipped lines by looking one
# byte past its own buffer there, so bash's answer swings by one with unrelated
# heap state (renaming $HISTFILE flips it). See known-issues.md.
echo "=== -n reads only what was appended since the last read"
printf '%s\n' n1 n2 > grow.txt
history -c
history -r grow.txt
history
printf '%s\n' n3 >> grow.txt
history -n grow.txt
history

echo "=== -a appends only what this session added since the last save"
history -c
: alpha
history -a appended.txt
echo "  --"; cat appended.txt
: beta
history -a appended.txt
echo "  --"; cat appended.txt

echo "=== with no operand the file is \$HISTFILE"
HISTFILE=hf.txt
history -c
: kept
history -w
echo "  rc=$?"; cat hf.txt
history -c
history -r
history

echo "=== -c with a file option still runs it, so -c -w empties the file"
history -c -w hf.txt
echo "  rc=$? bytes=[$(wc -c < hf.txt)]"

echo "=== a file that cannot be read or written fails quietly"
history -r nope.txt; echo "  r rc=$?"
history -n nope.txt; echo "  n rc=$?"
history -w missing/x; echo "  w rc=$?"
history -a missing/x; echo "  a rc=$?"

echo "=== two different file options are an error, the same one twice is not"
history -a -w f; echo "  rc=$?"
history -aw f; echo "  rc=$?"
history -aa hf.txt; echo "  rc=$?"

echo "=== HISTFILESIZE truncates \$HISTFILE to its last lines when assigned"
printf '%s\n' l1 l2 l3 l4 l5 > hf.txt
HISTFILESIZE=2
echo "  [$(cat hf.txt | tr '\n' '/')]"
HISTFILESIZE=1
echo "  [$(cat hf.txt | tr '\n' '/')]"

echo "=== ... but not for a value that is not a non-negative number"
printf '%s\n' m1 m2 m3 > hf.txt
HISTFILESIZE=-1; echo "  -1 [$(wc -l < hf.txt)]"
HISTFILESIZE=abc; echo "  abc [$(wc -l < hf.txt)] HISTFILESIZE=[$HISTFILESIZE]"
HISTFILESIZE=; echo "  empty [$(wc -l < hf.txt)]"
unset HISTFILESIZE; echo "  unset [$(wc -l < hf.txt)]"

echo "=== ... and 0 empties it"
HISTFILESIZE=0
echo "  bytes=[$(wc -c < hf.txt)]"

echo "=== assigning HISTFILE alone truncates nothing"
HISTFILESIZE=1
printf '%s\n' n1 n2 n3 > other.txt
HISTFILE=other.txt
echo "  [$(wc -l < other.txt)]"

echo "=== -w does not apply HISTFILESIZE"
unset HISTFILESIZE
history -c
: p; : q; : r
history -w other.txt
echo "  [$(wc -l < other.txt)]"

echo "=== -s and -p each un-record their own line; -s wins over both -p and -d"
history -c
: a
history -p one
history
history -c
: a
history -s replacement
history
history -c
: a
history -s -p one
history
history -c
: a
history -d 1 -s foo
history

echo "=== -p and -s with no arguments keep their own line"
history -c
history -p
history -s
history

echo "=== options cluster, and -d takes an attached offset"
history -c
: a
: b
history -d1
history
history -cs stored
history
