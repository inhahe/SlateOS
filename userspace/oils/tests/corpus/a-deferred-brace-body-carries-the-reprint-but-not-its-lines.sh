# A `${ … }` body that is kept as *text* carries bash's re-print of any `$( … )`
# inside it, not what was written. `parse_matched_pair` reads the body under
# `P_DOLBRACE` and hands a nested `$(` to `parse_comsub` (parse.y:3929 → 3959),
# which keeps `print_comsub`'s re-print and disposes the parse — so the bytes the
# body travels with are the deparse, exactly as for a substitution body itself.
#
# Only the verdicts that never build operand *words* from the body can show it:
# the ones that answer the whole expansion with a runtime `bad substitution`
# without looking inside. Where the body does become words, its substitutions are
# ordinary nodes that the same printer re-prints anyway, so those agree for a
# different reason and prove nothing here.
#
# The lines are the other half of it, and they do *not* follow the re-print. A
# fragment sitting after a substitution that re-prints to three lines where the
# source had one is still blamed to its physical line. So the body is carried in
# two coordinate systems at once — the text is the re-print, the numbering is the
# source — and bash keeps them apart. That is the whole point of the last section
# below, and it is what makes the splice a text-only change: nothing downstream
# of a deferred body derives a position from it.
e() { ( eval "$1" ) 2>&1; echo "  rc=$?"; }

echo "=== a deferred body quotes the re-print, not the source"
# `$( (echo 2) )` comes back as `$( ( echo 2 ))`: the subshell gets the guard
# space that stops `$((`, and the closing delimiters are not spaced apart.
e ': ${#x:-$( (echo 2) )}'
e 'x=1; : ${x@Z$( (echo 2) )}'
e 'a=(1); : ${a[@]@Z$( (echo 2) )}'

echo "=== and so does declare -f, from the same bytes"
f() { : ${#x:-$( (echo 2) )}; : ${x@Z$( (echo 2) )}; : ${a[@]@Z$( (echo 2) )}; }
declare -f f

echo "=== a compound command re-prints over lines, inside the diagnostic too"
# The message is not one line any more: the body it quotes back holds the
# printer's own layout, newlines and indentation included.
e ': ${#x:-$(if true; then echo a; fi)}'
e ': ${#x:-$(for i in a b; do echo $i; done)}'
g() { : ${#x:-$(while false; do echo a; done)}; }
declare -f g

echo "=== a body that builds words is re-printed too, for its own reason"
# The control: here the substitution becomes a real operand word, so it is
# re-printed as a node rather than spliced as text. Same answer, different path.
h() { : ${x:-$( (echo 2) )}; }
declare -f h

echo "=== but the body's lines stay on the source"
# The failing backtick is on the body's *second* line, and the substitution
# before it re-prints to three lines where the source had one. Numbering against
# the spliced text would push the blame two lines further on; bash reports the
# physical line, and reports it two lines apart for the two probes below purely
# because that is how far apart they are written.
unset x
echo "${x:-$(if true; then echo a; fi)
`fi`}"
echo "=== same layout, a substitution that re-prints to one line"
unset x
echo "${x:-$(echo a)
`fi`}"
echo "=== and the enclosing script's own numbering is untouched"
echo $LINENO
