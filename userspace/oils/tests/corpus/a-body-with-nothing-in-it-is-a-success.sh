# `eval` and `.`/`source` re-read their operand through the same read-eval loop
# the shell reads a script with, and that loop reports *its own* result rather
# than `$?`: the result starts at success and only a unit that actually runs
# overwrites it. So a body with nothing in it — empty, blank, comments only, or
# no operand at all — is 0 however the command before it ended.
#
# It is only the *nested* loop that works this way. The shell's own reader shares
# the code but not the rule: a comment or a blank line at the top level runs
# nothing and leaves `$?` exactly as it found it, which is the contrast drawn
# below.
#
# A body that did run something reports what it ran, and a body that would not
# parse reports the syntax error, so neither of those is touched by any of this.

: > empty.sh
printf '# just a comment\n' > cmt.sh
printf '\n\n   \n' > blank.sh
printf 'true\nfalse\n' > fails.sh

echo "=== eval with nothing to run"
false; eval ""; echo "  empty string: $?"
false; eval "   "; echo "  spaces: $?"
false; eval "# c"; echo "  comment: $?"
false; eval; echo "  no operand at all: $?"
false; eval "" ""; echo "  two empty operands: $?"
false; eval '
'; echo "  a newline: $?"

echo "=== and a sourced file with nothing in it"
false; . ./empty.sh; echo "  empty file: $?"
false; . ./cmt.sh; echo "  comment only: $?"
false; . ./blank.sh; echo "  blank lines: $?"
false; . ./empty.sh arg1 arg2; echo "  with arguments: $?"
false; . /dev/null; echo "  /dev/null: $?"
false; source ./empty.sh; echo "  spelled source: $?"

echo "=== a body that did run something still reports it"
false; . ./fails.sh; echo "  last command failed: $?"
true; . ./fails.sh; echo "  and it is the body's, not the caller's: $?"
false; eval "true"; echo "  eval true: $?"

echo "=== the same inside a function, where the status is the call's"
f() { false; eval ""; }
f; echo "  function: $?"
g() { false; . ./empty.sh; }
g; echo "  function sourcing: $?"

echo "=== but a comment on a line of its own does not clear \$?"
false
# this line runs nothing at all
echo "  after a comment line: $?"
false

echo "  after a blank line: $?"

echo "=== a syntax error is still the syntax error's status"
false; eval "if"; echo "  eval if: $?"
