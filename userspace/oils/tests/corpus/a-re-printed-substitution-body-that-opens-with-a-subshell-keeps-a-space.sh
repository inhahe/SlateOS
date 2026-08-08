# A `$( … )` body is not kept as source. `parse_comsub` ends
# `tcmd = print_comsub (parsed_command); … return ret` (parse.y:4219–4241), so
# what the enclosing scan appends is the parse *re-printed* — which means the
# re-print has to be text that reads back as the same construct.
#
# One shape does not, and bash guards it by hand: a body whose re-print opens
# with `(` would make the whole thing start `$((`, an arithmetic expansion. So
# `if (tcmd[0] == '(') { ret[0] = ' '; … }` (parse.y:4221–4227) prepends a
# space, and `$( (echo a) )` comes back `$( ( echo a ))` — never `$(( echo a ))`.
#
# The guard belongs to `parse_comsub`, not to the `$` spelling, and
# `read_token_word` sends all three of `$(...)`, `<(...)` and `>(...)` through
# that one call (parse.y:5028–5042). So a process substitution is guarded too,
# and there the unguarded form would not even parse.
#
# What is *not* re-printed is left alone: a backtick body is echoed from the
# source, and a `$((` that fell back to a substitution keeps the text the
# arithmetic scan collected — its own leading `(` is the one the source wrote,
# so it needs no space and gets none.
#
# Verified against bash 5.2.37.

echo "=== a subshell body is spaced away from the \$( ==="
f1() { : $( (echo a) ); }
f2() { : $(  ( echo a )  ); }
f3() { : $( (echo a) | cat ); }
f4() { : $( (echo a); echo b ); }
f5() { : $( ((1+2)) ); }
f6() { : $( (( x=1 )) ); }
declare -f f1 f2 f3 f4 f5 f6

echo "=== every spelling parse_comsub reads, and every place one can sit ==="
g1() { : <( (echo 2) ) >( (echo 3) ); }
g2() { : "$( (echo 2) )"; }
g3() { : ${a[$( (echo 2) )]}; }
g4() { : ${x:-$( (echo 2) )}; }
g5() { : $( : $( (echo a) ) ); }
declare -f g1 g2 g3 g4 g5

echo "=== a body that opens with something else is not spaced ==="
# No `if … fi` here: bash's `print_comsub` lays a compound command out over
# lines of its own and osh's printer keeps it on one, so the row would diverge
# for a reason that has nothing to do with the space. See `known-issues.md`,
# TD-OILS-A-REPRINTED-COMPOUND-COMMAND-IS-KEPT-ON-ONE-LINE.
h1() { : $( { echo a; } ); }
h2() { : $(echo a); }
h3() { : $( ! true ); }
declare -f h1 h2 h3

echo "=== not re-printed at all, so not guarded either ==="
# A backtick body is source; a `$((` that is not an expression keeps the text
# the arithmetic scan collected, inner `(` and all.
k1() { : `( echo a )`; }
k2() { : $((echo a) ); }
declare -f k1 k2

echo "=== and the printback reads back as the same construct ==="
# This is what the space is *for*: without it the re-print says `$((`, so the
# round trip would either change meaning or stop parsing.
eval "$(declare -f f1)"
declare -f f1
eval "$(declare -f g1)"
declare -f g1

echo done
