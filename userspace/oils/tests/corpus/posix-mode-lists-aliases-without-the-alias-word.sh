# POSIX spells an alias listing as a bare `name=value`, where bash normally
# writes a whole `alias name=value` command that would re-enter it. So posix
# mode drops the `alias ` word — but only from a listing that was *not* asked
# for reusably: `-p` means "print in a form that can be reused as input", and
# that form still needs the command word whatever the mode.
#
# The `-- ` that guards a name beginning with `-` goes with the word, since it
# exists only to stop the name being read back as an option and means nothing
# with no command to read it back into.

alias ll='ls -l'
alias 'q=echo "hi"'
alias e=''
alias -- -foo='x'

echo "=== outside, every listing is a command"
alias
echo "--- alias -p:"; alias -p
echo "--- a named query:"; alias ll
echo "--- and the dash-leading one:"; alias -- -foo

echo "=== inside, a listing with no -p is bare"
set -o posix
alias
echo "--- a named query is bare too:"; alias ll
echo "--- and so is the dash-leading one, -- and all:"; alias -- -foo

echo "=== but -p is still reusable, and for named operands as well"
alias -p
echo "--- alias -p NAME dumps the table first, then the operand:"
alias -p ll

echo "=== a not-found operand is unaffected by any of this"
alias NOPE; echo "  rc=$?"
alias -p NOPE 2>/dev/null | tail -1; echo "  rc=$?"

echo "=== defining still prints nothing, and is still not a listing"
alias z=1; echo "  rc=$?"
echo "--- a definition and a query in one command:"
alias w=2 ll

echo "=== and the word comes back when posix mode goes"
set +o posix
alias ll
