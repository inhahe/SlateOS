# `ulimit -a` writes two fields, not one: the description in a left-aligned 20,
# then the `(unit, -x) ` token right-aligned in 21. The closing paren therefore
# lands in column 40 and the value starts in 42, whatever the unit is called —
# which is the only thing that makes a column of values line up.
#
# Which resources exist, and what they are set to, is the host's business, so
# the case keeps only the letters every host has and replaces the values. What
# is compared is the shape of the line.

k() { grep -a -E '\((-[cdfnpstuv]|[^)]*, -[cdfnpstuv])\)'; }
z() { sed -e 's/) .*$/) VALUE/'; }

echo "== 1. the whole table, values taken out"
ulimit -a | k | z

echo "== 2. the column the paren closes in"
ulimit -a | k | awk '{ print "  " index($0, ")") }'

echo "== 3. two letters at once are labelled the same way"
ulimit -c -n | z
ulimit -p -n -t | z

echo "== 4. one letter alone is the bare value, no label"
ulimit -c | sed -e 's/^\([0-9][0-9]*\|unlimited\)$/VALUE/'
ulimit -n | sed -e 's/^\([0-9][0-9]*\|unlimited\)$/VALUE/'

echo "== 5. the soft and hard spellings label alike"
ulimit -S -a | k | z
ulimit -H -a | k | z
