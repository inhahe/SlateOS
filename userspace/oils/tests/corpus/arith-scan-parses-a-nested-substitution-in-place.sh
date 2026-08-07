# The `$(( … ))` scan steps over most of what it meets, but not a `$( … )`.
#
# `parse_matched_pair` under `P_ARITH` sends a `$(` to `parse_dollar_word`
# (parse.y:3937) and from there to `parse_comsub` (3959) — a whole nested parse,
# run while the enclosing line is still being read. Only the *text* survives it
# (`APPEND_NESTRET`); the body is read again when the expansion runs. So the
# parse's one lasting effect is its error, and that error is the enclosing
# unit's: it fires before any arithmetic is evaluated, before a branch is
# chosen, and even when the construct around it turns out not to be arithmetic
# at all.
#
# Verified against bash 5.2.37.

e() { ( eval "$1" ) 2>&1; echo "  rc=$?"; }

echo "=== a body that will not parse kills the enclosing unit ==="
e 'echo $(( 1 + $(fi) ))'
e 'echo $[ 1 + $(fi) ]'
e 'echo $(( 1 + $[ $(fi) ] ))'
# Inside a double-quoted span too: the quotes are opaque to the *expression*,
# not to the scan, which still reaches `parse_comsub` for what is in them.
e 'echo $(( "$(fi)" ))'
# A `${ … }` is not opaque to an arithmetic scan, so the `$(` in one is met the
# same way.
e 'echo $(( ${x:-$(fi)} ))'

echo "=== …at parse time, so an untaken branch does not save it ==="
# Unlike a backtick, whose body is not read until it runs.
e 'if false; then echo $(( 1 + $(fi) )); fi; echo ok'
e 'if false; then echo $(( 1 + `fi` )); fi; echo ok'

echo "=== …and before the check that would have made it a substitution ==="
# `chk_arithsub` does not run until expansion; this parse already happened.
e 'echo $(( echo $(fi) ) )'

echo "=== an enclosing body is re-lexed, and meets it again there ==="
e 'echo A$(echo "$(( $(fi) ))")B'

echo "=== a well-formed one is only text: parsed here, run at expansion ==="
echo "sum=[$(( 1 + $(echo 2) ))]"
echo "brk=[$[ 2 * $(echo 3) ]]"
echo "dq=[$(( "$(echo 4)" + 1 ))]"
echo "two=[$(( $(echo 1) + $(echo 2) ))]"
echo "deep=[$(( 1 + $(echo $(echo 2)) ))]"
echo "thru=[$(echo "$(( 1 + $(echo 2) ))")]"
# Its here-document is gathered by that nested parse, from the enclosing input.
echo "heredoc=[$(( 1 + $(cat <<E
5
E
) ))]"
# `declare -f` prints the arithmetic text back verbatim, nested body included.
f() { echo $(( 1 + $(echo 2) )); }; declare -f f
# Counted twice would be twice as many runs; it is run once.
n=0
inc() { n=$((n+1)); echo 1; }
echo "runs=[$(( $(inc) + $(inc) ))]"

echo "=== running out of input inside the nested parse is still its error ==="
e 'echo $(( $(fi'
e 'echo $(( $(echo a'
e 'echo $(( $(fi; $(done'

echo done
