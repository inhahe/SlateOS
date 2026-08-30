# An unterminated quote/substitution is reported on the line the construct
# *opened* on, and only after every complete logical line before it has already
# run: bash reads, parses, and executes one logical line at a time, so it does
# not discover the bad quote until it reads that line. Nothing on the failing
# line runs, not even a command that precedes the quote on it.
#
# The error necessarily ends the script, so each rule needs its own case file.
# This one pins the opening-line rule plus both execution-granularity rules.
# The `$( … )` exception (reported at end of input instead) is in
# lex-error-line-cmdsub.sh — process substitution `<( … )` shares that rule —
# and innermost-construct-wins is in lex-error-line-nested.sh.
#
# Verified against bash 5.2.37: earlier lines print, `four`/`five` do not, and
# the diagnostic names the last line (where `'` opened), not line 1.

echo one
f() { echo "in f"; }
f
for i in 1 2; do echo "loop $i"; done
if true; then echo "in if"; fi
echo two; echo three
echo four; echo five 'abc
